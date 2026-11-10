import React from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { hintShortcutLabel } from "./hintLabel";
import "./HintPill.css";

interface Props {
  /** The correction shortcut as stored, e.g. "ctrl+section". */
  binding: string;
  position: "top" | "bottom";
  visible: boolean;
  dir: "ltr" | "rtl";
}

// The overlay never takes keyboard focus, and neither must a click: the
// user's program keeps it, so the correction reads its selection.
const noFocus = (e: React.MouseEvent) => e.preventDefault();

/** "<shortcut> to improve the text"; a click runs the correction. */
const HintPill: React.FC<Props> = ({ binding, position, visible, dir }) => {
  const { t } = useTranslation();
  const osType = useOsType();
  const shortcut = hintShortcutLabel(binding, osType);

  return (
    <div
      dir={dir}
      className={`ov-stage ${position} ov-fade ${visible ? "show" : ""}`}
    >
      <button
        type="button"
        tabIndex={-1}
        className="hint-pill"
        onMouseDown={noFocus}
        onClick={() => void commands.correctionHintClicked()}
      >
        <kbd className="hint-key">{shortcut}</kbd>
        <span className="hint-text">{t("overlay.typingHint.text")}</span>
      </button>
    </div>
  );
};

export default HintPill;
