import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import type { CorrectionTone } from "@/bindings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Textarea } from "../../ui/Textarea";

export const MAX_TONES = 9;

interface ToneRowProps {
  tone: CorrectionTone;
  index: number;
  count: number;
  onEdit: (tone: CorrectionTone) => void;
  onMove: (direction: -1 | 1) => void;
  onDelete: () => void;
}

const ToneRow: React.FC<ToneRowProps> = ({
  tone,
  index,
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
          {index + 1}
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
          disabled={count <= 1}
          aria-label={t("settings.correction.tones.delete")}
          title={
            count <= 1
              ? t("settings.correction.tones.keepOne")
              : t("settings.correction.tones.delete")
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

interface CorrectionTonesEditorProps {
  tones: CorrectionTone[];
  onChange: (tones: CorrectionTone[]) => void;
}

export const CorrectionTonesEditor: React.FC<CorrectionTonesEditorProps> = ({
  tones,
  onChange,
}) => {
  const { t } = useTranslation();

  const edit = (index: number, tone: CorrectionTone) =>
    onChange(tones.map((current, i) => (i === index ? tone : current)));

  const move = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= tones.length) return;
    const next = [...tones];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };

  const remove = (index: number) => {
    if (tones.length <= 1) return;
    onChange(tones.filter((_, i) => i !== index));
  };

  const add = () => {
    if (tones.length >= MAX_TONES) return;
    onChange([
      ...tones,
      {
        id: `tom_${Date.now().toString(36)}`,
        name: t("settings.correction.tones.newName"),
        goal: t("settings.correction.tones.newGoal"),
        kind: "style",
      },
    ]);
  };

  return (
    <div className="px-4 p-2 space-y-3">
      <p className="text-sm text-ink-3">
        {t("settings.correction.tones.description", { max: MAX_TONES })}
      </p>
      <div className="space-y-3">
        {tones.map((tone, index) => (
          <ToneRow
            key={tone.id}
            tone={tone}
            index={index}
            count={tones.length}
            onEdit={(next) => edit(index, next)}
            onMove={(direction) => move(index, direction)}
            onDelete={() => remove(index)}
          />
        ))}
      </div>
      <div className="flex items-center gap-3">
        <Button
          variant="primary"
          size="md"
          onClick={add}
          disabled={tones.length >= MAX_TONES}
          className="flex items-center gap-1"
        >
          <Plus className="h-4 w-4" />
          {t("settings.correction.tones.add")}
        </Button>
        {tones.length >= MAX_TONES && (
          <span className="text-xs text-ink-3">
            {t("settings.correction.tones.maxReached", { max: MAX_TONES })}
          </span>
        )}
      </div>
    </div>
  );
};
