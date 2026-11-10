import assert from "node:assert/strict";
import { describe, test } from "node:test";
import type { CorrectionTone } from "@/bindings";
import { joinTones, moveItem, splitTones } from "./tones";

const tone = (id: string, kind?: "style" | "adjust"): CorrectionTone => ({
  id,
  name: id,
  goal: `goal ${id}`,
  ...(kind ? { kind } : {}),
});

describe("splitTones", () => {
  test("splits by kind and treats a missing kind as a style", () => {
    const { styles, adjusts } = splitTones([
      tone("a", "adjust"),
      tone("b"),
      tone("c", "style"),
      tone("d", "adjust"),
    ]);
    assert.deepEqual(
      styles.map((t) => t.id),
      ["b", "c"],
    );
    assert.deepEqual(
      adjusts.map((t) => t.id),
      ["a", "d"],
    );
  });
});

describe("joinTones", () => {
  test("rebuilds styles then adjusts, keeping order and always setting kind", () => {
    const joined = joinTones([tone("b"), tone("c")], [tone("a"), tone("d")]);
    assert.deepEqual(
      joined.map((t) => [t.id, t.kind]),
      [
        ["b", "style"],
        ["c", "style"],
        ["a", "adjust"],
        ["d", "adjust"],
      ],
    );
    assert.equal(joined[0].goal, "goal b");
  });

  test("keeps lists longer than the cap", () => {
    const many = Array.from({ length: 12 }, (_, i) => tone(`s${i}`, "style"));
    assert.equal(joinTones(many, []).length, 12);
  });
});

describe("moveItem", () => {
  test("swaps neighbours and ignores out of range moves", () => {
    assert.deepEqual(moveItem([1, 2, 3], 0, 1), [2, 1, 3]);
    assert.deepEqual(moveItem([1, 2, 3], 0, -1), [1, 2, 3]);
    assert.deepEqual(moveItem([1, 2, 3], 2, 1), [1, 2, 3]);
  });
});
