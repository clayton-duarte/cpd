import assert from "node:assert/strict";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { installHint } from "../lib/hint.ts";

const THEME = new URL("../themes/dango.json", import.meta.url);
const LABEL = " ↓ Jump to latest message · Ctrl+End ";

type ThemeLike = { fg(c: string, t: string): string; bg(c: string, t: string): string };
const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
const themeMod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
  loadThemeFromPath(path: string, mode?: string): ThemeLike;
};
const piMod = (await import(pathToFileURL(pi).href)) as { Theme: { prototype: ThemeLike } };
const load = (): ThemeLike => themeMod.loadThemeFromPath(THEME.pathname, "256color");
const original = piMod.Theme.prototype.bg;
const patched = () => assert.notEqual(piMod.Theme.prototype.bg, original, "installHint wraps Theme.prototype.bg");
const hint = (t: ThemeLike, label = LABEL) => t.bg("selectedBg", t.fg("text", label));

test("the jump hint is drawn text-only in 8", () => {
  const off = installHint();
  try {
    const out = hint(load());
    assert.equal(out, `\x1b[90m${LABEL}\x1b[39m`);
    assert.doesNotMatch(out, /\x1b\[(4\d|10\d|49|37)m|\x1b\[48;/);
  } finally {
    off();
  }
});

test("selectedBg keeps its background everywhere else", () => {
  const off = installHint();
  try {
    patched();
    assert.equal(load().bg("selectedBg", "x"), "\x1b[48;5;4mx\x1b[49m");
  } finally {
    off();
  }
});

test("a label pi has renamed is drawn as pi draws it", () => {
  const off = installHint();
  try {
    patched();
    const t = load();
    for (const label of [" Jump to bottom ", " ↓ Jump to latest message · Ctrl+End and more ", "x"]) {
      assert.equal(hint(t, label), t.bg("selectedBg", t.fg("text", label)));
      assert.match(hint(t, label), /\x1b\[48;5;4m/);
    }
  } finally {
    off();
  }
});

test("the hint patch installs once and is removed on shutdown", () => {
  const proto = piMod.Theme.prototype;
  const off1 = installHint();
  const wrapped = proto.bg;
  const off2 = installHint();
  try {
    assert.notEqual(wrapped, original, "installHint wraps Theme.prototype.bg");
    assert.equal(proto.bg, wrapped, "a second install does not wrap again");
  } finally {
    off2();
    off1();
  }
  assert.equal(proto.bg, original, "shutdown restores the original");
});
