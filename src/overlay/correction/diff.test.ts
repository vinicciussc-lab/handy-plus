import assert from "node:assert/strict";
import { test } from "node:test";
import { diffWords } from "./diff";

test("marks changed words", () => {
  assert.deepEqual(diffWords("oi mano blz", "Oi, mano, beleza"), [
    { text: "oi", kind: "removed" },
    { text: "Oi,", kind: "added" },
    { text: " ", kind: "same" },
    { text: "mano", kind: "removed" },
    { text: "mano,", kind: "added" },
    { text: " ", kind: "same" },
    { text: "blz", kind: "removed" },
    { text: "beleza", kind: "added" },
  ]);
});

test("identical text has no changes", () => {
  assert.ok(
    diffWords("tudo certo", "tudo certo").every((p) => p.kind === "same"),
  );
});

test("pure insertion and empty inputs", () => {
  const parts = diffWords("a c", "a b c");
  assert.ok(!parts.some((p) => p.kind === "removed"));
  assert.equal(
    parts
      .filter((p) => p.kind !== "removed")
      .map((p) => p.text)
      .join(""),
    "a b c",
  );
  assert.ok(parts.some((p) => p.kind === "added" && p.text === "b"));
  assert.deepEqual(diffWords("", ""), []);
});
