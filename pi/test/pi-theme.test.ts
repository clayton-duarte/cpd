import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { test } from "node:test";

/** contract §6.1.1 "The pi theme" (hs-d534, hs-d545): `pi/themes/cpd.json` names pi's 56 colour tokens, each an index 0-15 or "" (terminal default). */

const THEME = new URL("../themes/cpd.json", import.meta.url);
const PACKAGE = new URL("../package.json", import.meta.url);

/** The table of contract §6.1.1, hard-coded. */
const TABLE: Record<string, number | ""> = {
  accent: 4,
  border: 8,
  borderAccent: 4,
  borderMuted: 8,
  success: 2,
  error: 1,
  warning: 3,
  muted: 8,
  dim: 8,
  text: 7,
  thinkingText: 7,
  selectedBg: 4,
  scrollbarTrack: "",
  scrollbarThumb: 8,
  searchMatchBg: 3,
  searchMatchText: 0,
  userMessageBg: "",
  userMessageText: 7,
  customMessageBg: "",
  customMessageText: 7,
  customMessageLabel: 5,
  toolPendingBg: "",
  toolSuccessBg: "",
  toolErrorBg: "",
  toolTitle: 7,
  toolOutput: 7,
  mdHeading: 7,
  mdLink: 15,
  mdLinkUrl: 8,
  mdCode: 6,
  mdCodeBlock: 7,
  mdCodeBlockBorder: 8,
  mdQuote: 8,
  mdQuoteBorder: 8,
  mdHr: 8,
  mdListBullet: 7,
  toolDiffAdded: 2,
  toolDiffRemoved: 1,
  toolDiffContext: 8,
  syntaxComment: 8,
  syntaxKeyword: 4,
  syntaxFunction: 6,
  syntaxVariable: 7,
  syntaxString: 2,
  syntaxNumber: 3,
  syntaxType: 6,
  syntaxOperator: 7,
  syntaxPunctuation: 8,
  thinkingOff: 8,
  thinkingMinimal: 8,
  thinkingLow: 4,
  thinkingMedium: 6,
  thinkingHigh: 3,
  thinkingXhigh: 3,
  thinkingMax: 3,
  bashMode: 2,
};

const read = (): { name: string; vars?: unknown; colors: Record<string, unknown> } => JSON.parse(readFileSync(THEME, "utf8"));

test("theme cpd names every pi colour token", () => {
  assert.equal(Object.keys(TABLE).length, 56);
  const colors = read().colors;
  assert.deepEqual(Object.keys(colors).sort(), Object.keys(TABLE).sort());
});

test("theme cpd uses only indices 0-15 or the terminal default", () => {
  const theme = read();
  assert.equal(theme.name, "cpd");
  assert.ok(!theme.vars || Object.keys(theme.vars).length === 0, "no vars: every value is an index");
  for (const [token, value] of Object.entries(theme.colors)) {
    const ok = value === "" || (typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 15);
    assert.ok(ok, `${token}: ${JSON.stringify(value)} is not 0-15 or ""`);
  }
});

test("theme cpd matches the contract token table", () => {
  assert.deepEqual(read().colors, TABLE);
});

test("theme cpd loads in pi", async () => {
  assert.ok(existsSync(THEME), "pi/themes/cpd.json exists");
  const pi = createRequire(import.meta.url).resolve("@earendil-works/pi-coding-agent");
  const mod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
    loadThemeFromPath(path: string, mode?: string): { fg(color: string, text: string): string };
  };
  const theme = mod.loadThemeFromPath(THEME.pathname, "256color");
  assert.equal(theme.fg("customMessageLabel", "x"), "\x1b[38;5;5mx\x1b[39m");
  assert.equal(theme.fg("muted", "x"), "\x1b[38;5;8mx\x1b[39m");
});

test("the adapter package declares the theme", () => {
  const pkg = JSON.parse(readFileSync(PACKAGE, "utf8")) as { pi: { themes?: string[] } };
  assert.deepEqual(pkg.pi.themes, ["./themes"]);
});
