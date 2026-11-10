import React, { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import type {
  CorrectionErrorCode,
  CorrectionUpdateEvent,
  CorrectionView,
  FallbackOption,
  Lang,
  UiAction,
} from "@/bindings";
import i18n from "@/i18n";
import { getLanguageDirection } from "@/lib/utils/rtl";
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
  original: "overlay.correction.viewOriginal",
};
const VIEWS: CorrectionView[] = ["corrected", "original"];

const LANG_KEYS: Record<Lang, string> = {
  pt: "overlay.correction.langPt",
  en: "overlay.correction.langEn",
};
const LANGS: Lang[] = ["pt", "en"];

// Three lines of the 15px / 1.5 text per arrow key press.
const SCROLL_STEP = 3 * 15 * 1.5;

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

// The window never takes keyboard focus, and neither must a click.
const noFocus = (e: React.MouseEvent) => e.preventDefault();

const sendAction = (action: UiAction) => {
  void commands.correctionUiAction(action);
};

// The overlay may not receive native wheel scrolling: if the wheel did not
// move the box by the next frame, move it here.
const wheelFallback = (e: React.WheelEvent<HTMLElement>) => {
  const el = e.currentTarget;
  const before = el.scrollTop;
  const delta = e.deltaMode === 1 ? e.deltaY * 15 * 1.5 : e.deltaY;
  requestAnimationFrame(() => {
    if (el.scrollTop === before) el.scrollTop = before + delta;
  });
};

interface ButtonProps {
  label: string;
  title: string;
  action: UiAction;
  primary?: boolean;
}

const PanelButton: React.FC<ButtonProps> = ({
  label,
  title,
  action,
  primary,
}) => (
  <button
    type="button"
    tabIndex={-1}
    title={title}
    className={`cor-btn ${primary ? "primary" : ""}`}
    onMouseDown={noFocus}
    onClick={() => sendAction(action)}
  >
    {label}
  </button>
);

interface LangSwitchProps {
  lang: Lang;
  labels: Record<Lang, string>;
  title: string;
}

// PT / EN: the side on is the language shown; a click on the other side
// translates (same as T).
const LangSwitch: React.FC<LangSwitchProps> = ({ lang, labels, title }) => (
  <div className="cor-lang" title={title}>
    {LANGS.map((l) => (
      <button
        type="button"
        tabIndex={-1}
        key={l}
        className={`cor-lang-opt ${l === lang ? "on" : ""}`}
        onMouseDown={noFocus}
        onClick={() => {
          if (l !== lang) sendAction({ kind: "toggle_lang" });
        }}
      >
        {labels[l]}
      </button>
    ))}
  </div>
);

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

  // Start at the top when the choice or the preview opens or the view changes.
  useEffect(() => {
    if ((phase === "preview" || phase === "choosing") && textRef.current)
      textRef.current.scrollTop = 0;
  }, [phase, view]);

  // One arrow key press = one new nonce. The overlay can't take the wheel or
  // focus, so the keys arrive from the backend.
  useEffect(() => {
    if (nonce === lastNonce.current) return;
    lastNonce.current = nonce;
    if (
      nonce === 0 ||
      (phase !== "generating" && phase !== "preview" && phase !== "choosing")
    )
      return;
    if (phase === "generating") userScrolled.current = true;
    textRef.current?.scrollBy({ top: scrollDir * SCROLL_STEP });
  }, [nonce, scrollDir, phase]);

  if (!update || update.phase === "hidden") return null;

  const { styles, adjusts, model_label: modelLabel } = update;
  // One numbered list: the styles, then the adjustments.
  const options = [...styles, ...adjusts];
  const current = update.current_style;
  const adjust = update.current_adjust;
  const isOn = (i: number) =>
    i < styles.length ? i === current : i - styles.length === adjust;
  const running = [
    current != null ? styles[current] : null,
    adjust != null ? adjusts[adjust] : null,
  ]
    .filter(Boolean)
    .join(" + ");
  const toneModel = [running, modelLabel].filter(Boolean).join(" · ");

  const group = (label: string, names: string[], offset: number) =>
    names.length > 0 && (
      <div className="cor-group">
        <div className="cor-group-title">{label}</div>
        {names.map((name, k) => {
          const i = offset + k;
          return (
            <div
              key={i}
              className={isOn(i) ? "on" : ""}
              onMouseDown={noFocus}
              onClick={() => sendAction({ kind: "tone", value: i + 1 })}
            >
              <span className="cor-tone-name">
                {adjustsStart <= i && isOn(i) ? `${CHECK} ` : ""}
                {name}
              </span>
              <span className="cor-n">{i < 9 ? i + 1 : ""}</span>
            </div>
          );
        })}
      </div>
    );
  const adjustsStart = styles.length;
  const optionList = options.length > 0 && (
    <div className="cor-tones cor-scroll" onWheel={wheelFallback}>
      {group(t("overlay.correction.styleGroup"), styles, 0)}
      {group(t("overlay.correction.adjustGroup"), adjusts, styles.length)}
      {phase === "preview" && (
        <div className="cor-again">{t("overlay.correction.againHint")}</div>
      )}
    </div>
  );
  const langSwitch = (
    <LangSwitch
      lang={update.lang}
      labels={{ pt: t(LANG_KEYS.pt), en: t(LANG_KEYS.en) }}
      title={t("overlay.correction.langTip")}
    />
  );
  const langHint = (
    <>
      {" · "}
      {keyHint("T", t("overlay.correction.language"))}
    </>
  );
  const pickHint = options.length > 1 && (
    <>
      {" · "}
      {keyHint(
        `1-${Math.min(options.length, 9)}`,
        t("overlay.correction.otherTone"),
      )}
    </>
  );

  let card: React.ReactNode = null;
  let pill: React.ReactNode = null;

  switch (phase) {
    case "choosing":
      card = (
        <>
          {update.preloading && (
            <div className="cor-preload">
              <span>{t("overlay.correction.loadingModel")}</span>
              <span className="cor-bar">
                <i className="run" />
              </span>
            </div>
          )}
          <div className="cor-topbar">
            <div className="cor-choose">
              {styles.length > 0 &&
                t("overlay.correction.chooseStyle", { name: styles[0] })}
            </div>
            {langSwitch}
          </div>
          <div className="cor-grid">
            <div
              className="cor-text cor-scroll"
              ref={textRef}
              onWheel={wheelFallback}
            >
              {update.original}
            </div>
            {optionList}
          </div>
          <div className="cor-actions">
            <PanelButton
              label={t("overlay.correction.cancelButton")}
              title={t("overlay.correction.cancelTip")}
              action={{ kind: "cancel" }}
            />
          </div>
          <div className="cor-hint">
            {keyHint("↑↓", t("overlay.correction.scroll"))}
            {pickHint}
            {langHint}
            {" · "}
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
        </>
      );
      pill = modelLabel;
      break;
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
          <div className="cor-actions">
            <PanelButton
              label={t("overlay.correction.cancelButton")}
              title={t("overlay.correction.cancelTip")}
              action={{ kind: "cancel" }}
            />
          </div>
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
          <div
            className="cor-stream cor-scroll"
            ref={textRef}
            onWheel={wheelFallback}
          >
            {update.text}
            <span className="cor-caret" />
          </div>
          <div className="cor-hint">
            {keyHint("↑↓", t("overlay.correction.scroll"))}
            {" · "}
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
          <div className="cor-actions">
            <PanelButton
              label={t("overlay.correction.cancelButton")}
              title={t("overlay.correction.cancelTip")}
              action={{ kind: "cancel" }}
            />
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
              <button
                type="button"
                tabIndex={-1}
                key={v}
                className={`cor-view ${v === view ? "on" : ""}`}
                onMouseDown={noFocus}
                onClick={() => sendAction({ kind: "view", value: v })}
              >
                {t(VIEW_KEYS[v])}
              </button>
            ))}
            {langSwitch}
          </div>
          <div className="cor-grid">
            <div
              className="cor-text cor-scroll"
              ref={textRef}
              onWheel={wheelFallback}
            >
              {view === "corrected" && update.text}
              {view === "original" && update.original}
            </div>
            {optionList}
          </div>
          <div className="cor-actions">
            <PanelButton
              label={t("overlay.correction.cancelButton")}
              title={t("overlay.correction.cancelTip")}
              action={{ kind: "cancel" }}
            />
            <PanelButton
              label={t("overlay.correction.copyButton")}
              title={t("overlay.correction.copyTip")}
              action={{ kind: "copy" }}
            />
            <PanelButton
              label={t("overlay.correction.applyButton")}
              title={t("overlay.correction.applyTip")}
              action={{ kind: "apply" }}
              primary
            />
          </div>
          <div className="cor-hint">
            {keyHint("Enter", t("overlay.correction.apply"))}
            {" · "}
            {keyHint("Tab", t("overlay.correction.view"))}
            {" · "}
            {keyHint("↑↓", t("overlay.correction.scroll"))}
            {pickHint}
            {langHint}
            {" · "}
            {keyHint("Ctrl + C", t("overlay.correction.copy"))}
            {" · "}
            {keyHint("Esc", t("overlay.correction.cancel"))}
          </div>
        </>
      );
      pill = (
        <>
          <span className="cor-ok">{CHECK}</span>{" "}
          {update.elapsed_ms == null
            ? t("overlay.correction.readyNow")
            : t("overlay.correction.ready", {
                s: formatSeconds(update.elapsed_ms),
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
              <div
                key={option}
                className="cor-fb-opt"
                onMouseDown={noFocus}
                onClick={() => sendAction({ kind: "tone", value: i + 1 })}
              >
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
