import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import type { CorrectionTone } from "@/bindings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Textarea } from "../../ui/Textarea";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { joinTones, MAX_PER_KIND, moveItem, splitTones } from "./tones";

interface ToneRowProps {
  tone: CorrectionTone;
  index: number;
  // The number the tone has in the panel (styles first, then adjustments).
  number: number;
  canDelete: boolean;
  keepOneTitle: string;
  count: number;
  onEdit: (tone: CorrectionTone) => void;
  onMove: (direction: -1 | 1) => void;
  onDelete: () => void;
}

const ToneRow: React.FC<ToneRowProps> = ({
  tone,
  index,
  number,
  canDelete,
  keepOneTitle,
  count,
  onEdit,
  onMove,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [name, setName] = useState(tone.name);
  const [goal, setGoal] = useState(tone.goal);

  useEffect(() => {
    setName(tone.name);
    setGoal(tone.goal);
  }, [tone.name, tone.goal]);

  // An empty name or goal would make a useless tone, so the old text comes back.
  const commit = () => {
    const nextName = name.trim() || tone.name;
    const nextGoal = goal.trim() || tone.goal;
    setName(nextName);
    setGoal(nextGoal);
    if (nextName !== tone.name || nextGoal !== tone.goal) {
      onEdit({ ...tone, name: nextName, goal: nextGoal });
    }
  };

  return (
    <div className="space-y-2 p-3 rounded-md border border-line bg-wash/50">
      <div className="flex items-center gap-2">
        <span className="text-xs font-semibold text-ink-3 w-5 shrink-0">
          {number <= MAX_PER_KIND ? number : ""}
        </span>
        <Input
          type="text"
          value={name}
          onChange={(event) => setName(event.target.value)}
          onBlur={commit}
          placeholder={t("settings.correction.tones.namePlaceholder")}
          aria-label={t("settings.correction.tones.name")}
          variant="compact"
          className="flex-1 min-w-0"
        />
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onMove(-1)}
          disabled={index === 0}
          aria-label={t("settings.correction.tones.moveUp")}
          title={t("settings.correction.tones.moveUp")}
        >
          <ArrowUp className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onMove(1)}
          disabled={index === count - 1}
          aria-label={t("settings.correction.tones.moveDown")}
          title={t("settings.correction.tones.moveDown")}
        >
          <ArrowDown className="h-4 w-4" />
        </Button>
        <Button
          variant="danger-ghost"
          size="sm"
          onClick={onDelete}
          disabled={!canDelete}
          aria-label={t("settings.correction.tones.delete")}
          title={
            !canDelete ? keepOneTitle : t("settings.correction.tones.delete")
          }
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
      <Textarea
        value={goal}
        onChange={(event) => setGoal(event.target.value)}
        onBlur={commit}
        placeholder={t("settings.correction.tones.goalPlaceholder")}
        aria-label={t("settings.correction.tones.goal")}
        variant="compact"
        rows={3}
        className="w-full"
      />
    </div>
  );
};

interface ToneListProps {
  kind: "styles" | "adjusts";
  tones: CorrectionTone[];
  firstNumber: number;
  onChange: (tones: CorrectionTone[]) => void;
}

const ToneList: React.FC<ToneListProps> = ({
  kind,
  tones,
  firstNumber,
  onChange,
}) => {
  const { t } = useTranslation();
  const key = `settings.correction.tones.${kind}`;
  const atMax = tones.length >= MAX_PER_KIND;
  // Styles can't be emptied: the panel needs a default.
  const mustKeepOne = kind === "styles" && tones.length <= 1;

  const add = () => {
    if (atMax) return;
    onChange([
      ...tones,
      {
        id: `tom_${Date.now().toString(36)}`,
        name: t(`${key}.newName`),
        goal: t(`${key}.newGoal`),
        kind: kind === "styles" ? "style" : "adjust",
      },
    ]);
  };

  return (
    <div className="px-4 p-2 space-y-3">
      <p className="text-sm text-ink-3">
        {t(`${key}.description`, { max: MAX_PER_KIND })}
      </p>
      <div className="space-y-3">
        {tones.map((tone, index) => (
          <ToneRow
            key={tone.id}
            tone={tone}
            index={index}
            number={firstNumber + index}
            count={tones.length}
            canDelete={!mustKeepOne}
            keepOneTitle={t("settings.correction.tones.styles.keepOne")}
            onEdit={(next) =>
              onChange(tones.map((c, i) => (i === index ? next : c)))
            }
            onMove={(direction) => onChange(moveItem(tones, index, direction))}
            onDelete={() => {
              if (!mustKeepOne) onChange(tones.filter((_, i) => i !== index));
            }}
          />
        ))}
      </div>
      <div className="flex items-center gap-3">
        <Button
          variant="primary"
          size="md"
          onClick={add}
          disabled={atMax}
          className="flex items-center gap-1"
        >
          <Plus className="h-4 w-4" />
          {t(`${key}.add`)}
        </Button>
        {atMax && (
          <span className="text-xs text-ink-3">
            {t(`${key}.maxReached`, { max: MAX_PER_KIND })}
          </span>
        )}
      </div>
    </div>
  );
};

interface CorrectionTonesEditorProps {
  tones: CorrectionTone[];
  onChange: (tones: CorrectionTone[]) => void;
}

export const CorrectionTonesEditor: React.FC<CorrectionTonesEditorProps> = ({
  tones,
  onChange,
}) => {
  const { t } = useTranslation();
  const { styles, adjusts } = splitTones(tones);

  return (
    <>
      <SettingsGroup title={t("settings.correction.tones.styles.title")}>
        <ToneList
          kind="styles"
          tones={styles}
          firstNumber={1}
          onChange={(next) => onChange(joinTones(next, adjusts))}
        />
      </SettingsGroup>
      <SettingsGroup title={t("settings.correction.tones.adjusts.title")}>
        <ToneList
          kind="adjusts"
          tones={adjusts}
          firstNumber={styles.length + 1}
          onChange={(next) => onChange(joinTones(styles, next))}
        />
      </SettingsGroup>
    </>
  );
};
