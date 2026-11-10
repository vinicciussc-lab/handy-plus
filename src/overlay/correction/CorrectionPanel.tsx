import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type {
  CorrectionErrorCode,
  CorrectionUpdateEvent,
  CorrectionView,
  FallbackOption,
} from "@/bindings";
import i18n from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";
import { diffWords } from "./diff";
import "./CorrectionPanel.css";

interface CorrectionPanelProps {
  update: CorrectionUpdateEvent | null;
  position: "top" | "bottom";
  visible: boolean;
}

const ERROR_KEYS: Record<CorrectionErrorCode, string> = {
  nothing_selected: "overlay.correction.errors.nothing_selected",
  text_too_long: "overlay.correction.errors.text_too_long",
  no_model_chosen: "overlay.correction.errors.no_model_chosen",
  model_missing: "overlay.correction.errors.model_missing",
  engine_failed: "overlay.correction.errors.engine_failed",
  cloud_failed: "overlay.correction.errors.cloud_failed",
  keys_held: "overlay.correction.errors.keys_held",
  clipboard_failed: "overlay.correction.errors.clipboard_failed",
};

const FALLBACK_KEYS: Record<FallbackOption, string> = {
  cloud: "overlay.correction.cloud",
  cpu: "overlay.correction.cpu",
};

const VIEW_KEYS: Record<CorrectionView, string> = {
  corrected: "overlay.correction.viewCorrected",
  changes: "overlay.correction.viewChanges",
  original: "overlay.correction.viewOriginal",
};
const VIEWS: CorrectionView[] = ["corrected", "changes", "original"];

// Three lines of the 14px / 1.5 text per arrow key press.
const SCROLL_STEP = 3 * 14 * 1.5;

const CHECK = "✓";

const percent = (value: number) =>
  Math.round(Math.min(1, Math.max(0, value)) * 100);

const formatSeconds = (ms: number) =>
  new Intl.NumberFormat(i18n.language, {
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
  }).format(ms / 1000);

interface ProgressRowProps {
  label: string;
  // null = progress unknown (animated bar), undefined = not started yet
  value: number | null | undefined;
  done?: boolean;
}

const ProgressRow: React.FC<ProgressRowProps> = ({ label, value, done }) => {
  const waiting = value === undefined && !done;
  const indeterminate = value === null && !done;
  const width = done ? 100 : value != null ? percent(value) : 0;
  return (
    <div className={`cor-phase ${waiting ? "wait" : ""}`}>
      <span className="cor-lbl">{label}</span>
      <span className="cor-bar">
        <i
          className={done ? "done" : indeterminate ? "run" : ""}
          style={indeterminate ? undefined : { width: `${width}%` }}
        />
      </span>
      <span className={`cor-pct ${done ? "ok" : ""}`}>
        {done ? CHECK : value != null ? `${width}%` : ""}
      </span>
    </div>
  );
};

const keyHint = (label: string, text: string) => (
  <>
    <span className="cor-key">{label}</span> {text}
  </>
);

const CorrectionPanel: React.FC<CorrectionPanelProps> = ({
  update,
  position,
  visible,
}) => {
  const { t } = useTranslation();
  const textRef = useRef<HTMLDivElement>(null);
  const userScrolled = useRef(false);
  const lastNonce = useRef(0);
  const phase = update?.phase;
  const view = update?.view ?? "corrected";
  const nonce = update?.scroll_nonce ?? 0;
  const scrollDir = update?.scroll_dir ?? 0;
  const streamed = phase === "generating" ? (update?.text ?? "") : "";

  // Keep the newest streamed text in view until the user scrolls by hand.
  useEffect(() => {
    if (phase !== "generating") {
      userScrolled.current = false;
      return;
    }
    const el = textRef.current;
    if (el && !userScrolled.current) el.scrollTop = el.scrollHeight;
  }, [phase, streamed]);

  // Start at the top when the preview opens or the view changes.
  useEffect(() => {
    if (phase === "preview" && textRef.current) textRef.current.scrollTop = 0;
  }, [phase, view]);

  // One arrow key press = one new nonce. The overlay can't take the wheel or
  // focus, so the keys arrive from the backend.
  useEffect(() => {
    if (nonce === lastNonce.current) return;
    lastNonce.current = nonce;
    if (nonce === 0 || (phase !== "generating" && phase !== "preview")) return;
    if (phase === "generating") userScrolled.current = true;
    textRef.current?.scrollBy({ top: scrollDir * SCROLL_STEP });
  }, [nonce, scrollDir, phase]);

  if (!update || update.phase === "hidden") return null;

  const { tones, model_label: modelLabel } = update;
  const toneIndex = update.tone_index;
  const toneModel = [tones[toneIndex], modelLabel].filter(Boolean).join(" · ");

  let card: React.ReactNode = null;
  let pill: React.ReactNode = null;

  switch (phase) {
    case "loading":
      card = (
        <>
          <ProgressRow
            label={t("overlay.correction.loadingModel")}
            value={update.load_progress}
          />
          <ProgressRow
            label={t("overlay.correction.generating")}
            value={undefined}
          />
        </>
      );
      pill = toneModel;
      break;
    case "generating":
      card = (
        <>
          <ProgressRow
            label={t("overlay.correction.loadingModel")}
            value={1}
            done
          />
          <ProgressRow
            label={t("overlay.correction.generating")}
            value={update.gen_progress}
          />
          <div className="cor-stream cor-scroll" ref={textRef}>
            {update.text}
            <span className="cor-caret" />
          </div>
          <div className="cor-hint">
            {keyHint("↑↓", t("overlay.correction.scroll"))}
            {" · "}
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
        </>
      );
      pill = toneModel;
      break;
    case "preview": {
      card = (
        <>
          <div className="cor-viewbar">
            {VIEWS.map((v) => (
              <span key={v} className={`cor-view ${v === view ? "on" : ""}`}>
                {t(VIEW_KEYS[v])}
              </span>
            ))}
          </div>
          <div className="cor-grid">
            <div className="cor-text cor-scroll" ref={textRef}>
              {view === "corrected" && update.text}
              {view === "original" && update.original}
              {view === "changes" &&
                diffWords(update.original, update.text).map((p, i) =>
                  p.kind === "same" ? (
                    <React.Fragment key={i}>{p.text}</React.Fragment>
                  ) : (
                    <span key={i} className={`cor-${p.kind}`}>
                      {p.text}
                    </span>
                  ),
                )}
            </div>
            {tones.length > 0 && (
              <div className="cor-tones cor-scroll">
                {tones.map((name, i) => (
                  <div key={i} className={i === toneIndex ? "on" : ""}>
                    <span className="cor-tone-name">{name}</span>
                    <span className="cor-n">{i + 1}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
          <div className="cor-hint">
            {keyHint("Enter", t("overlay.correction.apply"))}
            {" · "}
            {keyHint("Tab", t("overlay.correction.view"))}
            {" · "}
            {keyHint("↑↓", t("overlay.correction.scroll"))}
            {tones.length > 1 && (
              <>
                {" · "}
                {keyHint(
                  `1-${tones.length}`,
                  t("overlay.correction.otherTone"),
                )}
              </>
            )}
            {" · "}
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
        </>
      );
      pill = (
        <>
          <span className="cor-ok">{CHECK}</span>{" "}
          {t("overlay.correction.ready", {
            s: formatSeconds(update.elapsed_ms ?? 0),
          })}
        </>
      );
      break;
    }
    case "needs_fallback":
      card = (
        <>
          <div className="cor-fb-title">{t("overlay.correction.gpuFull")}</div>
          <div className="cor-fb-list">
            {update.fallback_options.map((option, i) => (
              <div key={option}>
                {keyHint(String(i + 1), t(FALLBACK_KEYS[option]))}
              </div>
            ))}
          </div>
          <div className="cor-hint">
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
        </>
      );
      pill = toneModel;
      break;
    case "applied":
      pill = (
        <>
          <span className="cor-ok">{CHECK}</span>{" "}
          {t("overlay.correction.applied")}
          {" · "}
          {t("overlay.correction.undoHint")}
        </>
      );
      break;
    case "copied":
      pill = (
        <>
          <span className="cor-ok">{CHECK}</span>{" "}
          {t("overlay.correction.copied")}
        </>
      );
      break;
    case "error":
      pill = (
        <span className="cor-err">
          {t(ERROR_KEYS[update.error ?? "engine_failed"])}
        </span>
      );
      break;
  }

  return (
    <div
      dir={getLanguageDirection(i18n.language)}
      className={`ov-stage ${position} ov-fade ${visible ? "show" : ""}`}
    >
      <div className={`cor-wrap ${position}`}>
        {card && <div className="cor-card">{card}</div>}
        {pill && <div className="cor-pill">{pill}</div>}
      </div>
    </div>
  );
};

export default CorrectionPanel;
