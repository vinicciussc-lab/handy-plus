import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { SpellCheck } from "lucide-react";
import { commands } from "@/bindings";
import { syncLanguageFromSettings } from "@/i18n";
import "./CorrectionButton.css";

/** Round button on the focused text box; a click corrects the whole box. */
const CorrectionButton: React.FC = () => {
  const { t } = useTranslation();
  const label = t("overlay.correction.button");

  useEffect(() => {
    syncLanguageFromSettings();
  }, []);

  return (
    <div className="cb-stage">
      <button
        type="button"
        className="cb-button"
        title={label}
        aria-label={label}
        // Never take the focus: the text box must keep it.
        onMouseDown={(event) => event.preventDefault()}
        // The language may have changed since the window loaded.
        onPointerEnter={() => syncLanguageFromSettings()}
        onClick={() => {
          commands.correctionButtonClicked();
        }}
      >
        <SpellCheck size={15} strokeWidth={2.25} aria-hidden="true" />
      </button>
    </div>
  );
};

export default CorrectionButton;
