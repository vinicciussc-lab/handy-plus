import assert from "node:assert/strict";
import { test } from "node:test";
import { hintShortcutLabel } from "./hintLabel";

test("prints the ABNT2 key left of Z as a backslash", () => {
  assert.equal(hintShortcutLabel("ctrl+section", "windows"), "Ctrl + \\");
});

test("formats other bindings like the shortcut keycaps", () => {
  assert.equal(
    hintShortcutLabel("ctrl+shift+k", "windows"),
    "Ctrl + Shift + K",
  );
  assert.equal(hintShortcutLabel("alt+space", "linux"), "Alt + Space");
  assert.equal(hintShortcutLabel("option+space", "macos"), "⌥ Space");
});
