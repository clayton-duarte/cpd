import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

/** contract §6.1.1 "The pi theme" (hs-d534, hs-d545): `pi/themes/dango.json` names pi's 56 colour tokens, each an index 0-15 or "" (terminal default). */

const THEME = new URL("../themes/dango.json", import.meta.url);
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
  searchMatchBg: "",
  searchMatchText: 3,
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

test("theme dango names every pi colour token", () => {
  assert.equal(Object.keys(TABLE).length, 56);
  const colors = read().colors;
  assert.deepEqual(Object.keys(colors).sort(), Object.keys(TABLE).sort());
});

test("theme dango uses only indices 0-15 or the terminal default", () => {
  const theme = read();
  assert.equal(theme.name, "dango");
  assert.ok(!theme.vars || Object.keys(theme.vars).length === 0, "no vars: every value is an index");
  for (const [token, value] of Object.entries(theme.colors)) {
    const ok = value === "" || (typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 15);
    assert.ok(ok, `${token}: ${JSON.stringify(value)} is not 0-15 or ""`);
  }
});

test("theme dango matches the contract token table", () => {
  assert.deepEqual(read().colors, TABLE);
});

test("theme dango loads in pi", async () => {
  assert.ok(existsSync(THEME), "pi/themes/dango.json exists");
  const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
  const mod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
    loadThemeFromPath(path: string, mode?: string): { fg(color: string, text: string): string };
  };
  const theme = mod.loadThemeFromPath(THEME.pathname, "256color");
  assert.equal(theme.fg("customMessageLabel", "x"), "\x1b[38;5;5mx\x1b[39m");
  assert.equal(theme.fg("muted", "x"), "\x1b[38;5;8mx\x1b[39m");
});

test("the User's input is drawn as text 7 on no fill", async () => {
  const pi = fileURLToPath(import.meta.resolve("@earendil-works/pi-coding-agent"));
  const mod = (await import(pathToFileURL(join(dirname(pi), "modes/interactive/theme/theme.js")).href)) as {
    loadThemeFromPath(path: string, mode?: string): { fg(color: string, text: string): string; bg(color: string, text: string): string };
  };
  const theme = mod.loadThemeFromPath(THEME.pathname, "256color");
  assert.equal(theme.fg("userMessageText", "x"), "\x1b[38;5;7mx\x1b[39m");
  assert.equal(theme.bg("userMessageBg", "x"), "\x1b[49mx\x1b[49m");
});

test("the adapter package declares the theme", () => {
  const pkg = JSON.parse(readFileSync(PACKAGE, "utf8")) as { pi: { themes?: string[] } };
  assert.deepEqual(pkg.pi.themes, ["./themes"]);
});

test("the adapter ships one theme, dango.json, named dango", () => {
  assert.deepEqual(readdirSync(new URL("../themes/", import.meta.url)), ["dango.json"]);
  const theme = JSON.parse(readFileSync(new URL("../themes/dango.json", import.meta.url), "utf8")) as { name: string };
  assert.equal(theme.name, "dango");
});

test("the contract selects the theme the adapter ships", () => {
  const lines = readFileSync(new URL("../../docs/contract.md", import.meta.url), "utf8").split("\n");
  const start = lines.findIndex((l) => l.startsWith("#### The pi theme"));
  assert.notEqual(start, -1, "docs/contract.md has a line starting `#### The pi theme`");
  let end = lines.findIndex((l, i) => i > start && l.startsWith("#"));
  if (end === -1) end = lines.length;
  const m = /"theme": "([^"]+)"/.exec(lines.slice(start + 1, end).join("\n"));
  assert.ok(m, 'the section "The pi theme" of docs/contract.md has no `"theme": "<name>"`');
  assert.equal(m[1], "dango");
  const file = new URL(`../themes/${m[1]}.json`, import.meta.url);
  assert.ok(existsSync(file), `pi/themes/${m[1]}.json exists`);
  assert.equal((JSON.parse(readFileSync(file, "utf8")) as { name: string }).name, m[1]);
});
