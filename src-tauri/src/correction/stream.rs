//! Chat client for text correction: SSE streaming against OpenAI-compatible
//! endpoints (the local llama-server and cloud providers), plus a
//! non-streaming path for Anthropic.
//!
//! `base_url` is used as given and `/chat/completions` is appended. For the
//! local server pass the URL returned by `LocalEngine::ensure_loaded`
//! as-is (llama-server serves `/chat/completions` as well as `/v1/...`);
//! cloud providers carry their own `/v1` in `base_url`.

use crate::llm_client::{create_client, report_reqwest_error, send_chat_completion};
use crate::settings::{PostProcessApiFormat, PostProcessProvider};
use futures_util::StreamExt;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ChatTarget {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub format: PostProcessApiFormat,
    /// True for the local llama-server, which takes extra sampling fields.
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    Delta(String),
    Done,
    /// An `error` object sent inside the stream (some providers do this over HTTP 200).
    Error(String),
}

/// Incremental parser for a server-sent-events chat stream. Only
/// `delta.content` is surfaced; reasoning deltas and comments are dropped.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &str) -> Vec<SseItem> {
        self.buffer.push_str(&chunk.replace("\r\n", "\n"));
        let mut items = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let event: String = self.buffer.drain(..end + 2).collect();
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    items.push(SseItem::Done);
                } else if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
                        let msg = err["message"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| err.to_string());
                        items.push(SseItem::Error(msg));
                        continue;
                    }
                    if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                        if !text.is_empty() {
                            items.push(SseItem::Delta(text.to_string()));
                        }
                    }
                }
            }
        }
        items
    }

    /// Call at end of stream: flushes a final event that lacks its blank line.
    pub fn finish(&mut self) -> Vec<SseItem> {
        self.feed(
            "

",
        )
    }
}

/// Decodes as much of `pending` as is valid UTF-8 and removes it from the buffer.
/// An incomplete multi-byte tail is kept for the next chunk; genuinely invalid
/// bytes are replaced rather than stalling the stream.
fn decode_utf8_chunk(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(&String::from_utf8_lossy(&pending[..valid]));
                match e.error_len() {
                    Some(bad) => {
                        out.push(char::REPLACEMENT_CHARACTER);
                        pending.drain(..valid + bad);
                    }
                    None => {
                        pending.drain(..valid);
                        return out;
                    }
                }
            }
        }
    }
}

/// A stream is only a success if it said `[DONE]` and produced text.
fn finish_reply(full: String, done: bool) -> Result<String, String> {
    if !done {
        return Err("The reply stream ended before it was complete".to_string());
    }
    if full.trim().is_empty() {
        return Err("The model returned no text".to_string());
    }
    Ok(full)
}

fn request_body(target: &ChatTarget, system: &str, user: &str) -> Value {
    let mut body = json!({
        "model": target.model,
        "stream": true,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    if target.local {
        body["chat_template_kwargs"] = json!({ "enable_thinking": false });
        body["temperature"] = json!(1.0);
        body["top_k"] = json!(64);
        body["top_p"] = json!(0.95);
    }
    body
}

/// Sends the correction request and returns the full reply, calling
/// `on_delta` for each piece as it arrives. Anthropic is not streamed: the
/// whole reply comes in a single `on_delta` call.
pub async fn stream_chat(
    target: &ChatTarget,
    system: &str,
    user: &str,
    mut on_delta: impl FnMut(&str) + Send,
) -> Result<String, String> {
    let provider = PostProcessProvider {
        id: "correction".to_string(),
        label: "correction".to_string(),
        base_url: target.base_url.clone(),
        allow_base_url_edit: false,
        models_endpoint: None,
        supports_structured_output: false,
        api_format: target.format,
    };

    if target.format == PostProcessApiFormat::Anthropic {
        // The Anthropic path takes a single prompt, so the instructions and
        // the text go together.
        let prompt = format!("{system}\n\n{user}");
        let text = send_chat_completion(
            &provider,
            target.api_key.clone(),
            &target.model,
            prompt,
            true,
        )
        .await?
        .ok_or_else(|| "The model returned no text".to_string())?;
        on_delta(&text);
        return Ok(text);
    }

    let url = format!("{}/chat/completions", target.base_url.trim_end_matches('/'));
    let response = create_client(&provider, &target.api_key)?
        .post(&url)
        .json(&request_body(target, system, user))
        .send()
        .await
        .map_err(|e| report_reqwest_error("HTTP request failed", &e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let short: String = body.chars().take(300).collect();
        return Err(format!("API request failed with status {status}: {short}"));
    }

    let mut parser = SseParser::default();
    let mut pending: Vec<u8> = Vec::new();
    let mut full = String::new();
    let mut done = false;
    let mut stream = response.bytes_stream();
    'outer: while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| report_reqwest_error("Stream read failed", &e))?;
        pending.extend_from_slice(&chunk);
        let text = decode_utf8_chunk(&mut pending);
        for item in parser.feed(&text) {
            match item {
                SseItem::Delta(d) => {
                    on_delta(&d);
                    full.push_str(&d);
                }
                SseItem::Done => {
                    done = true;
                    break 'outer;
                }
                SseItem::Error(e) => return Err(format!("The provider reported an error: {e}")),
            }
        }
    }
    if !done {
        for item in parser.finish() {
            match item {
                SseItem::Delta(d) => {
                    on_delta(&d);
                    full.push_str(&d);
                }
                SseItem::Done => done = true,
                SseItem::Error(e) => return Err(format!("The provider reported an error: {e}")),
            }
        }
    }
    finish_reply(full, done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_target() -> ChatTarget {
        ChatTarget {
            base_url: "http://127.0.0.1:1".into(),
            api_key: String::new(),
            model: "m".into(),
            format: PostProcessApiFormat::OpenAi,
            local: false,
        }
    }

    #[test]
    fn parses_content_deltas_across_chunk_boundaries() {
        let mut p = SseParser::default();
        let mut out =
            p.feed("data: {\"choices\":[{\"delta\":{\"content\":\"Oi\"}}]}\n\ndata: {\"choi");
        out.extend(p.feed("ces\":[{\"delta\":{\"content\":\", mano\"}}]}\n\ndata: [DONE]\n\n"));
        assert_eq!(
            out,
            vec![
                SseItem::Delta("Oi".into()),
                SseItem::Delta(", mano".into()),
                SseItem::Done
            ]
        );
    }

    #[test]
    fn ignores_reasoning_deltas_and_keepalives() {
        let mut p = SseParser::default();
        let out = p
            .feed(": ping\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hm\"}}]}\n\n");
        assert!(out.is_empty());
    }

    #[test]
    fn local_request_body_disables_thinking_and_pins_sampling() {
        let b = request_body(
            &ChatTarget {
                local: true,
                ..test_target()
            },
            "S",
            "U",
        );
        assert_eq!(b["stream"], true);
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(b["temperature"], 1.0);
        assert_eq!(b["top_k"], 64);
        assert_eq!(b["top_p"], 0.95);
    }

    #[test]
    fn cloud_request_body_has_no_local_only_fields() {
        let b = request_body(
            &ChatTarget {
                local: false,
                ..test_target()
            },
            "S",
            "U",
        );
        assert!(b.get("chat_template_kwargs").is_none() && b.get("top_k").is_none());
    }

    #[test]
    fn utf8_char_split_across_chunks_is_kept_whole() {
        let bytes = "ã".as_bytes(); // 2 bytes
        let mut pending = bytes[..1].to_vec();
        assert_eq!(decode_utf8_chunk(&mut pending), "");
        assert_eq!(pending.len(), 1);
        pending.extend_from_slice(&bytes[1..]);
        assert_eq!(decode_utf8_chunk(&mut pending), "ã");
        assert!(pending.is_empty());
    }

    #[test]
    fn invalid_utf8_is_replaced_instead_of_stalling() {
        let mut pending = vec![b'a', 0xFF, b'b'];
        assert_eq!(decode_utf8_chunk(&mut pending), "a\u{FFFD}b");
        assert!(pending.is_empty());
    }

    #[test]
    fn crlf_events_parse_even_when_split_across_chunks() {
        let mut p = SseParser::default();
        let mut out = p.feed("data: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\r\n\r");
        out.extend(p.feed("\ndata: [DONE]\r\n\r\n"));
        assert_eq!(out, vec![SseItem::Delta("A".into()), SseItem::Done]);
    }

    #[test]
    fn unterminated_final_event_is_flushed_by_finish() {
        let mut p = SseParser::default();
        assert!(p
            .feed("data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}")
            .is_empty());
        assert_eq!(p.finish(), vec![SseItem::Delta("x".into())]);
    }

    #[test]
    fn in_stream_error_event_is_surfaced() {
        let mut p = SseParser::default();
        let out = p.feed("data: {\"error\":{\"message\":\"x\"}}\n\n");
        assert_eq!(out, vec![SseItem::Error("x".into())]);
    }

    #[test]
    fn stream_without_done_or_without_text_is_an_error() {
        assert!(finish_reply("partial".into(), false).is_err());
        assert!(finish_reply(String::new(), true).is_err());
        assert_eq!(finish_reply("ok".into(), true), Ok("ok".to_string()));
    }

    async fn serve(status: &str, body: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 4096];
            let _ = s.read(&mut buf).await;
            s.write_all(resp.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn stream_chat_collects_deltas_and_requires_done() {
        let ok = serve(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Oi\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;
        let t = ChatTarget {
            base_url: ok,
            ..test_target()
        };
        let mut seen = String::new();
        let r = stream_chat(&t, "S", "U", |d| seen.push_str(d)).await;
        assert_eq!(r, Ok("Oi".to_string()));
        assert_eq!(seen, "Oi");

        let cut = serve(
            "200 OK",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Oi\"}}]}\n\n",
        )
        .await;
        let t = ChatTarget {
            base_url: cut,
            ..test_target()
        };
        assert!(stream_chat(&t, "S", "U", |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn stream_chat_reports_http_and_in_stream_errors() {
        let bad = serve("500 Internal Server Error", "boom").await;
        let t = ChatTarget {
            base_url: bad,
            ..test_target()
        };
        let e = stream_chat(&t, "S", "U", |_| {}).await.unwrap_err();
        assert!(e.contains("500") && e.contains("boom"), "{e}");

        let inband = serve(
            "200 OK",
            "data: {\"error\":{\"message\":\"quota out\"}}\n\n",
        )
        .await;
        let t = ChatTarget {
            base_url: inband,
            ..test_target()
        };
        let e = stream_chat(&t, "S", "U", |_| {}).await.unwrap_err();
        assert!(e.contains("quota out"), "{e}");
    }

    const SPEC_TEXTS: [&str; 3] = [
        "oi mano blz? entao eu vi aquele video q vc mandou ontem e achei mt massa, mas acho q o cara errou umas coisa la no final sobre o processador, depois agente conversa melhor sobre isso",
        "Bom dia, gostaria de saber se é possível remarcar a reunião de amanhã pois eu tive um imprevisto, e não vou conseguir chegar no horário combinado, se puder ser na quinta seria ótimo",
        "cara esse jogo ta mt bom mas a otimização ta horrivel, minha placa fica em 100% e mesmo assim cai pra 40 fps nas cidade",
    ];

    /// Manual: HANDY_PLUS_TEST_GGUF=caminho\modelo.gguf cargo test correction::stream -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn real_local_engine_corrects_spec_texts() {
        use crate::correction::local_engine::LocalEngine;
        use crate::correction::settings::{default_tones, DEFAULT_STYLE_RULES};
        use crate::correction::text::{build_messages, clean_output};
        use std::path::PathBuf;
        use std::time::Instant;

        let model =
            PathBuf::from(std::env::var("HANDY_PLUS_TEST_GGUF").expect("set HANDY_PLUS_TEST_GGUF"));
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/llama-server/llama-server.exe");
        let engine = LocalEngine::new(bin);
        let (url, load_ms) = engine
            .ensure_loaded(&model, true, None, |_| {})
            .await
            .expect("load model");
        println!("model loaded in {load_ms} ms at {url}");

        let goal = default_tones().remove(0).goal;
        let target = ChatTarget {
            base_url: url,
            api_key: String::new(),
            model: "local".into(),
            format: PostProcessApiFormat::OpenAi,
            local: true,
        };

        let mut report = String::new();
        // Run twice: the first pass includes the first-request warm-up.
        for pass in ["first", "warm"] {
            for (i, text) in SPEC_TEXTS.iter().enumerate() {
                let (system, user) = build_messages(DEFAULT_STYLE_RULES, &goal, text);
                let started = Instant::now();
                let mut deltas = 0;
                let raw = stream_chat(&target, &system, &user, |_| deltas += 1).await;
                let ms = started.elapsed().as_millis();
                let raw = match raw {
                    Ok(r) => r,
                    Err(e) => {
                        engine.unload();
                        panic!("stream_chat failed: {e}");
                    }
                };
                let out = clean_output(text, &raw);
                let line = format!("[{pass} #{}] {ms} ms, {deltas} deltas\n{out}\n\n", i + 1);
                print!("{line}");
                report.push_str(&line);
                assert!(!out.is_empty());
                assert!(!out.contains('—') && !out.contains('–'));
                assert!(!raw.contains("<think>"));
            }
        }

        // Reasoning deltas are dropped by the parser, so look at the raw stream.
        let (system, user) = build_messages(DEFAULT_STYLE_RULES, &goal, SPEC_TEXTS[0]);
        let raw_stream = reqwest::Client::new()
            .post(format!("{}/chat/completions", target.base_url))
            .json(&request_body(&target, &system, &user))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let has_reasoning = raw_stream.contains("reasoning_content");
        println!("raw stream has reasoning_content: {has_reasoning}");
        report.push_str(&format!(
            "raw stream has reasoning_content: {has_reasoning}\n"
        ));

        if let Ok(path) = std::env::var("HANDY_PLUS_TEST_REPORT") {
            std::fs::write(path, &report).unwrap();
        }
        engine.unload();
        assert!(!has_reasoning, "model emitted reasoning deltas");
    }

    const TONES_SAMPLE: &str = "eu acho q o jogo vale a pena msm com os bugs. ontem eu joguei umas 5 horas e nao enjoei. so q o preço ta mt caro pra o q entrega entao eu esperaria uma promoção. a historia é mt boa e os grafico sao lindos. no fim das contas eu recomendo mas so na promoçao";

    /// Manual: HANDY_PLUS_TEST_GGUF=caminho\modelo.gguf HANDY_PLUS_TEST_REPORT=saida.txt cargo test correction::stream::tests::real_local_engine_tones -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn real_local_engine_tones() {
        use crate::correction::local_engine::LocalEngine;
        use crate::correction::settings::{default_tones, DEFAULT_STYLE_RULES};
        use crate::correction::text::{build_messages, clean_output};
        use std::path::PathBuf;

        let model =
            PathBuf::from(std::env::var("HANDY_PLUS_TEST_GGUF").expect("set HANDY_PLUS_TEST_GGUF"));
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/llama-server/llama-server.exe");
        let engine = LocalEngine::new(bin);
        let (url, _) = engine
            .ensure_loaded(&model, true, None, |_| {})
            .await
            .expect("load model");
        let target = ChatTarget {
            base_url: url,
            api_key: String::new(),
            model: "local".into(),
            format: PostProcessApiFormat::OpenAi,
            local: true,
        };
        let mut report = format!(
            "ORIGINAL:
{TONES_SAMPLE}

"
        );
        for tone in default_tones() {
            let (system, user) = build_messages(DEFAULT_STYLE_RULES, &tone.goal, TONES_SAMPLE);
            let raw = match stream_chat(&target, &system, &user, |_| {}).await {
                Ok(r) => r,
                Err(e) => {
                    engine.unload();
                    panic!("stream_chat failed: {e}");
                }
            };
            let out = clean_output(TONES_SAMPLE, &raw);
            report.push_str(&format!(
                "[{}]
{out}

",
                tone.name
            ));
            assert!(!out.contains('—') && !out.contains('–'));
            if tone.id == "desenvolver" {
                assert!(
                    out.chars().count() >= TONES_SAMPLE.chars().count() * 130 / 100,
                    "Desenvolver did not expand enough: {} vs {}",
                    out.chars().count(),
                    TONES_SAMPLE.chars().count()
                );
            }
            if tone.id == "mais_curto" {
                assert!(
                    out.chars().count() < TONES_SAMPLE.chars().count() * 85 / 100,
                    "Mais curto did not shorten enough: {} vs {}",
                    out.chars().count(),
                    TONES_SAMPLE.chars().count()
                );
            }
        }
        print!("{report}");
        if let Ok(path) = std::env::var("HANDY_PLUS_TEST_REPORT") {
            std::fs::write(path, &report).unwrap();
        }
        engine.unload();
    }
}
