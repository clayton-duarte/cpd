import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import type { Component } from "@earendil-works/pi-tui";
import { DEFAULT_CONFIG } from "../compact-tools/extensions/compact-tools-config.ts";
import { LOADER_FRAMES } from "../compact-tools/extensions/compact-tools-loader.ts";
import { Rendering } from "../lib/render.ts";
import type { RenderedMessage, Theme } from "../lib/pi.ts";

/** hs-d582, hs-d583: pi's icon swap. Expected glyphs are code points written here, never read back from the code. */

const G = (n: number): string => String.fromCodePoint(n);
const EXPAND = G(0xF0B26);
const COLLAPSE = G(0xF0B2C);
const TERM = G(0xF0B2A);

test("the fold toggle defaults are the chevron circles U+F0B26 and U+F0B2C", () => {
  assert.deepEqual(DEFAULT_CONFIG.toggle, { expand: EXPAND, collapse: COLLAPSE });
});

test("the config schema toggle defaults are the same two glyphs", () => {
  const url = new URL("../compact-tools/schemas/compact-tools.schema.json", import.meta.url);
  const props = JSON.parse(readFileSync(url, "utf8")).properties.toggle;
  assert.deepEqual(props.default, { expand: EXPAND, collapse: COLLAPSE });
  assert.equal(props.properties.expand.default, EXPAND);
  assert.equal(props.properties.collapse.default, COLLAPSE);
});

process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");

const theme: Theme = { fg: (_c, t) => t, bg: (_c, t) => t, bold: (t) => t };
const cpdMessage = (text: string, content = "body"): RenderedMessage =>
  ({ role: "custom", timestamp: TIME_MS, customType: "cpd", content, display: true, details: { text } }) as unknown as RenderedMessage;
const draw = (m: RenderedMessage, expanded: boolean): string[] =>
  (new Rendering().render(m, { expanded, outputPad: 1 }, theme) as Component).render(80);
const plain = (s: string): string => s.replace(/\x1b\[[0-9;]*m/g, "");

test("a cpd block closes with the chevron circles and ends with the right chevron terminator", () => {
  const m = cpdMessage("Worker of Job x", "line one\nline two");
  assert.equal(plain(draw(m, false).at(-1)!), ` ╰${EXPAND} expand · 03:11`);
  assert.equal(plain(draw(m, true).at(-1)!), ` ╰${COLLAPSE} collapse · 03:11`);
  assert.equal(plain(draw(cpdMessage("Nothing more", ""), false).at(-1)!), ` ╰${TERM}`);
});

test("the loader has the 28 moon_alt frames and the waiting glyph is not one of them", () => {
  const moons = [0xE3E3, ...Array.from({ length: 27 }, (_, i) => 0xE3C8 + i)].map(G);
  assert.deepEqual([...LOADER_FRAMES], moons);
  assert.ok(!LOADER_FRAMES.includes(G(0xF1853)));
});

const OLD = [0xF1787, 0xF0996, 0xF073A, 0xF03CC, 0xF0B95, 0xF004B, 0xF0063, 0xF005A];
const pair = (c: number): string => {
  const v = c - 0x10000;
  return `\\u${(0xD800 + (v >> 10)).toString(16)}\\u${(0xDC00 + (v & 0x3FF)).toString(16)}`;
};
const SELF = fileURLToPath(import.meta.url);
const ROOT = fileURLToPath(new URL("..", import.meta.url));

const files = (dir: string): string[] =>
  readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    if (e.name === "node_modules") return [];
    const p = join(dir, e.name);
    if (e.isDirectory()) return files(p);
    return /\.(ts|json|md|txt)$/.test(e.name) || e.name === "NOTICE" ? [p] : [];
  });

test("pi spells none of the old glyphs", () => {
  const hits: string[] = [];
  for (const f of files(ROOT)) {
    if (f === SELF) continue;
    readFileSync(f, "utf8").split("\n").forEach((line, i) => {
      for (const c of OLD) {
        const hex = c.toString(16);
        const token = new RegExp(`(?<![0-9a-f])${hex}(?![0-9a-f])`, "i");
        if (line.includes(G(c)) || token.test(line) || line.toLowerCase().includes(pair(c)))
          hits.push(`${relative(ROOT, f)}:${i + 1} U+${hex.toUpperCase()}`);
      }
    });
  }
  assert.deepEqual(hits, []);
});
