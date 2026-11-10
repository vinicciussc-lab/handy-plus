import type { CorrectionTone } from "@/bindings";

// Styles are rewrites of the original; adjustments are chained on the result.
export const MAX_PER_KIND = 9;

export interface SplitTones {
  styles: CorrectionTone[];
  adjusts: CorrectionTone[];
}

// A tone with no kind (an old store) is a style.
export const splitTones = (tones: CorrectionTone[]): SplitTones => ({
  styles: tones.filter((tone) => tone.kind !== "adjust"),
  adjusts: tones.filter((tone) => tone.kind === "adjust"),
});

// Styles first, then adjustments, each list in its own order, kind always set.
export const joinTones = (
  styles: CorrectionTone[],
  adjusts: CorrectionTone[],
): CorrectionTone[] => [
  ...styles.map((tone) => ({ ...tone, kind: "style" as const })),
  ...adjusts.map((tone) => ({ ...tone, kind: "adjust" as const })),
];

export const moveItem = <T>(
  list: T[],
  index: number,
  direction: -1 | 1,
): T[] => {
  const target = index + direction;
  if (index < 0 || index >= list.length || target < 0 || target >= list.length)
    return list;
  const next = [...list];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
};
