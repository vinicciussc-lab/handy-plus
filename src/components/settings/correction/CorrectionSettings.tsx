import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CorrectionSettings as Settings } from "@/bindings";
import { SettingsGroup, SettingContainer, Textarea } from "@/components/ui";
import { notify } from "@/stores/noticeStore";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";
import { ShortcutInput } from "../ShortcutInput";
import { CorrectionProviderSection } from "./CorrectionProviderSection";
import { CorrectionTonesEditor } from "./CorrectionTonesEditor";

const STYLE_RULES_DEBOUNCE_MS = 800;

export const CorrectionSettings: React.FC = () => {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<Settings | null>(null);
  // Always the newest form state, so two quick edits don't overwrite each other.
  const latest = useRef<Settings | null>(null);
  // Saves go one after the other so the last edit is the one that stays.
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    commands
      .getCorrectionSettingsCmd()
      .then((loaded) => {
        if (cancelled) return;
        latest.current = loaded;
        setSettings(loaded);
      })
      .catch((error) => {
        notify({
          title: t("settings.correction.loadFailed"),
          detail: String(error),
        });
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const update = useCallback(
    (patch: Partial<Settings>) => {
      if (!latest.current) return;
      const next = { ...latest.current, ...patch };
      latest.current = next;
      setSettings(next);
      queue.current = queue.current.then(async () => {
        try {
          const result = await commands.setCorrectionSettingsCmd(next);
          if (result.status === "error") {
            notify({
              title: t("settings.correction.saveFailed"),
              detail: result.error,
            });
          }
        } catch (error) {
          notify({
            title: t("settings.correction.saveFailed"),
            detail: String(error),
          });
        }
      });
    },
    [t],
  );

  if (!settings) {
    return <div className="max-w-3xl w-full mx-auto" />;
  }

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.correction.hotkey.title")}>
        <ShortcutInput
          shortcutId="correct_selection"
          descriptionMode="tooltip"
          grouped={true}
        />
        <ShortcutInput
          shortcutId="translate_selection"
          descriptionMode="tooltip"
          grouped={true}
        />
        <ToggleSwitch
          checked={settings.typing_hint ?? true}
          onChange={(typing_hint) => update({ typing_hint })}
          label={t("settings.correction.typingHint.label")}
          description={t("settings.correction.typingHint.description")}
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.correction.sounds.title")}>
        <ToggleSwitch
          checked={settings.sounds ?? true}
          onChange={(sounds) => update({ sounds })}
          label={t("settings.correction.sounds.label")}
          description={t("settings.correction.sounds.description")}
          descriptionMode="tooltip"
          grouped={true}
        />
      </SettingsGroup>

      <CorrectionProviderSection settings={settings} onChange={update} />

      <SettingsGroup title={t("settings.correction.styleRules.title")}>
        <StyleRulesField
          value={settings.style_rules}
          onCommit={(style_rules) => update({ style_rules })}
        />
      </SettingsGroup>

      <SettingsGroup title={t("settings.correction.tones.title")}>
        <CorrectionTonesEditor
          tones={settings.tones}
          onChange={(tones) => update({ tones })}
        />
      </SettingsGroup>
    </div>
  );
};

const StyleRulesField: React.FC<{
  value: string;
  onCommit: (value: string) => void;
}> = ({ value, onCommit }) => {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(value);
  const saved = useRef(value);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const draftRef = useRef(draft);
  const commitRef = useRef(onCommit);
  commitRef.current = onCommit;

  const flush = useCallback(() => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    if (draftRef.current !== saved.current) {
      saved.current = draftRef.current;
      commitRef.current(draftRef.current);
    }
  }, []);

  // Leaving the page keeps what was typed.
  useEffect(() => flush, [flush]);

  const handleChange = (next: string) => {
    draftRef.current = next;
    setDraft(next);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(flush, STYLE_RULES_DEBOUNCE_MS);
  };

  return (
    <SettingContainer
      title={t("settings.correction.styleRules.label")}
      description={t("settings.correction.styleRules.description")}
      descriptionMode="inline"
      layout="stacked"
      grouped={true}
    >
      <Textarea
        value={draft}
        onChange={(event) => handleChange(event.target.value)}
        onBlur={flush}
        rows={8}
        className="w-full"
      />
    </SettingContainer>
  );
};
