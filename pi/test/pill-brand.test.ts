import assert from "node:assert/strict";
import { test } from "node:test";
import { type Component } from "@earendil-works/pi-tui";
import { eventMessage as rawEventMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";
import type { Theme } from "../lib/pi.ts";
import { Rendering } from "../lib/render.ts";
import { ev, ID } from "./fake.ts";

/** contract §6.1.1 "The pi chat rows" (hs-d509, hs-d537): cpd's pill is magenta (bg 5, text 0, caps 5), holds `cpd ` and the first sentence of the summary; hs-d545: indexed colours 0-15 only. */

// The theme paints a recognisable code, so that anything left to the theme shows.
const theme: Theme = {
  fg: (color, text) => `\x1b[38;5;99m<${color}>${text}\x1b[39m`,
  bg: (_color, text) => text,
  bold: (text) => text,
};

process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const eventMessage = (...args: Parameters<typeof rawEventMessage>) => ({ ...rawEventMessage(...args), timestamp: TIME_MS });

const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";
const FOLDED = "\u{F0B26}";
const OPEN = "\u{F0B2C}";

const render = (text: string, expanded = false): string[] => {
  const m = eventMessage(ev(7, "job-merged", { text }) as unknown as CpdEvent, ID);
  const rendering = Object.assign(new Rendering(), { style: "compact" as const });
  return (rendering.render(m, { expanded, outputPad: 1 }, theme) as Component).render(200);
};
const PILL = `\x1b[35m${CAP_L}\x1b[7m\x1b[3mcpd\x1b[23m\x1b[27m\x1b[35m${CAP_R}\x1b[39m`;
// the pill holds only `cpd`; the first sentence follows it outside, text 7
const pill = (sentence: string): string => `${PILL} \x1b[37m${sentence}\x1b[39m`;

test("cpd pill is magenta, inverted, with magenta caps", () => {
  const row = render("Job api-1 merged.")[0]!;
  assert.ok(row.includes(pill("Job api-1 merged.")), JSON.stringify(row));
});

test("cpd pill shows the first sentence of the summary", () => {
  assert.ok(render("Job api-1 merged. The rest follows.")[0]!.includes(pill("Job api-1 merged")), "cut at the first `.`");
  assert.ok(render("Escalation from api-1: it needs a decision")[0]!.includes(pill("Escalation from api-1")), "cut at the first `:`");
  assert.ok(render("first line\nsecond line")[0]!.includes(pill("first line")), "cut at the line end");
});

test("cpd pill cuts a first sentence over 60 characters with an ellipsis", () => {
  const long = `${"word ".repeat(20)}end`;
  const exact = "x".repeat(60);
  assert.ok(render(long)[0]!.includes(pill(`${long.slice(0, 59)}…`)) || render(long)[0]!.includes(pill(`${long.slice(0, 60)}…`)), "a long sentence ends with …");
  assert.ok(render(exact)[0]!.includes(pill(`${exact}`)), "a sentence of exactly 60 is not cut");
  const cut = render(long)[0]!.match(/\x1b\[37m(.*?)\x1b\[39m/)![1]!;
  assert.equal(cut.length, 60 + 1, JSON.stringify(cut));
});

test("cpd message lead and rails are 8 and the toggle is 7", () => {
  const collapsed = render("Job api-1 merged.\nsecond line");
  assert.ok(collapsed[0]!.includes("\x1b[90m╭─\x1b[39m"), JSON.stringify(collapsed[0]));
  assert.ok(collapsed.at(-1)!.includes(`\x1b[90m╰\x1b[39m\x1b[37m${FOLDED} expand · 03:11\x1b[39m`), JSON.stringify(collapsed.at(-1)));
  const expanded = render("Job api-1 merged.\nsecond line", true);
  assert.ok(expanded[1]!.includes("\x1b[90m│ \x1b[39m"), JSON.stringify(expanded[1]));
  assert.ok(expanded.at(-1)!.includes(`\x1b[90m╰\x1b[39m\x1b[37m${OPEN} collapse · 03:11\x1b[39m`), JSON.stringify(expanded.at(-1)));
});

test("cpd message draws only indexed colours 0-15", () => {
  const out = [...render("Job api-1 merged.\nsecond line"), ...render("Job api-1 merged.\nsecond line", true)].join("\n");
  const seen: string[] = [];
  for (const m of out.matchAll(/\x1b\[([0-9;]*)m/g)) {
    const p = m[1]!;
    const n = Number(p);
    const ok =
      [0, 1, 4, 7, 22, 23, 24, 27, 39, 49].includes(n) || (n >= 30 && n <= 37) || (n >= 40 && n <= 47) || (n >= 90 && n <= 97) || (n >= 100 && n <= 107) ||
      /^[34]8;5;(\d|1[0-5])$/.test(p);
    if (p !== "" && !ok) seen.push(p);
  }
  assert.deepEqual(seen, []);
  assert.ok(out.includes("\x1b[7m"), "the magenta pill is drawn (the check is not vacuous)");
});
