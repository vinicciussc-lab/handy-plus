export type DiffPart = {
  text: string;
  kind: "same" | "added" | "removed";
};

// Splits on whitespace but keeps the whitespace runs as tokens, so joining the
// "same" + "added" parts rebuilds the corrected text exactly.
const tokenize = (text: string): string[] =>
  text.split(/(\s+)/).filter((token) => token.length > 0);

/**
 * Word level diff (LCS over tokens). Inside a changed stretch the `removed`
 * parts come before the `added` ones.
 */
export function diffWords(before: string, after: string): DiffPart[] {
  const a = tokenize(before);
  const b = tokenize(after);

  // lcs[i][j] = LCS length of a[i..] and b[j..]
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () =>
    new Array<number>(b.length + 1).fill(0),
  );
  for (let i = a.length - 1; i >= 0; i--) {
    for (let j = b.length - 1; j >= 0; j--) {
      lcs[i][j] =
        a[i] === b[j]
          ? lcs[i + 1][j + 1] + 1
          : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }

  const parts: DiffPart[] = [];
  let removed: DiffPart[] = [];
  let added: DiffPart[] = [];
  const flush = () => {
    parts.push(...removed, ...added);
    removed = [];
    added = [];
  };

  let i = 0;
  let j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) {
      flush();
      parts.push({ text: a[i], kind: "same" });
      i++;
      j++;
    } else if (
      j < b.length &&
      (i === a.length || lcs[i][j + 1] >= lcs[i + 1][j])
    ) {
      added.push({ text: b[j], kind: "added" });
      j++;
    } else {
      removed.push({ text: a[i], kind: "removed" });
      i++;
    }
  }
  flush();
  return parts;
}
