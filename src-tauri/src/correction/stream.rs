//! Chat client for text correction: SSE streaming against OpenAI-compatible
//! endpoints (the local llama-server and cloud providers), plus a
//! non-streaming path for Anthropic.
//!
//! `base_url` is used as given and `/chat/completions` is appended. For the
//! local server pass the URL returned by `LocalEngine::ensure_loaded`
//! as-is (llama-server serves `/chat/completions` as well as `/v1/...`);
//! cloud providers carry their own `/v1` in `base_url`.

// Only Task 8 (the correction pipeline) calls this module.
#![allow(dead_code)]

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
    let mut stream = response.bytes_stream();
    'outer: while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| report_reqwest_error("Stream read failed", &e))?;
        pending.extend_from_slice(&chunk);
        // A multi-byte character can be split across network chunks.
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
        pending.drain(..valid);
        for item in parser.feed(&text) {
            match item {
                SseItem::Delta(d) => {
                    on_delta(&d);
                    full.push_str(&d);
                }
                SseItem::Done => break 'outer,
            }
        }
    }
    Ok(full)
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
}
