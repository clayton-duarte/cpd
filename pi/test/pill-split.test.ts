import assert from "node:assert/strict";
import { test } from "node:test";
import type { Component } from "@earendil-works/pi-tui";
import { Rendering } from "../lib/render.ts";
import type { RenderedMessage, Theme } from "../lib/pi.ts";

/** contract §6.9 / §6.1.1 (hs-d534, hs-d545), commands.md §15.9: the cpd pill holds only `cpd`; the first sentence follows it outside, text 7; a block that cannot fold ends with ` ╰󰬪`. */

const theme: Theme = { fg: (_c, t) => t, bg: (_c, t) => t, bold: (t) => t };
const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";
const TERM = "\u{F0B2A}";
const PILL = `\x1b[35m${CAP_L}\x1b[45m\x1b[30mcpd\x1b[49m\x1b[35m${CAP_R}\x1b[39m`;
const GREY = (s: string): string => `\x1b[90m${s}\x1b[39m`;
const plain = (s: string): string => s.replace(/\x1b\[[0-9;]*m/g, "");

const cpdMessage = (text: string, content = "body"): RenderedMessage =>
  ({ role: "custom", timestamp: 0, customType: "cpd", content, display: true, details: { text } }) as unknown as RenderedMessage;
const summaryMessage = (): RenderedMessage =>
  ({ role: "custom", timestamp: 0, customType: "cpd-plan-summary", content: "# Title\nline two", display: true, details: { review: "r6" } }) as unknown as RenderedMessage;

const draw = (m: RenderedMessage, o: { expanded?: boolean; outputPad?: number; width?: number } = {}): string[] =>
  (new Rendering().render(m, { expanded: o.expanded ?? false, outputPad: o.outputPad ?? 1 }, theme) as Component).render(o.width ?? 80);

test("the cpd pill holds only the word cpd with caps 5 and text 0", () => {
  for (const expanded of [false, true])
    for (const outputPad of [1, 0]) {
      const head = draw(cpdMessage("Worker of Job x"), { expanded, outputPad })[0]!;
      assert.ok(head.includes(PILL), JSON.stringify(head));
      assert.ok(plain(head).includes(`${CAP_L}cpd${CAP_R}`), JSON.stringify(plain(head)));
      assert.ok(!head.includes("\x1b[7m") && !head.includes("\x1b[2m"), "no reverse, no faint");
    }
});

test("the first sentence follows the pill outside it in text 7", () => {
  const head = draw(cpdMessage("Worker of Job chatfix (Attempt a1). More."))[0]!;
  assert.equal(head, ` ${GREY("╭─")}${PILL} \x1b[37mWorker of Job chatfix (Attempt a1)\x1b[39m`);
  assert.equal(plain(draw(cpdMessage("Worker of Job x"), { outputPad: 0 })[0]!), `╭─${CAP_L}cpd${CAP_R} Worker of Job x`);
});

test("a sentence over 60 characters is cut with an ellipsis outside the pill", () => {
  const sentence = "w".repeat(10).concat(" ").repeat(9) + "end";
  const head = draw(cpdMessage(sentence), { width: 200 })[0]!;
  assert.ok(head.includes(PILL), "the pill is still just cpd");
  const after = plain(head).slice(` ╭─${CAP_L}cpd${CAP_R} `.length);
  assert.ok(after.endsWith("…"), after);
  assert.ok(after.length <= 61, after);
  assert.ok(!sentence.startsWith(after), "cut");
});

test("a long sentence wraps like a tool row's title and never inside the pill", () => {
  const sentence = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
  const rows = draw(cpdMessage(sentence), { width: 24 });
  const text = rows.map(plain);
  const head = rows.findIndex((_r, i) => i > 0 && !text[i]!.startsWith(" │ "));
  const cont = text.slice(1, head === -1 ? undefined : head);
  assert.ok(text[0]!.startsWith(` ╭─${CAP_L}cpd${CAP_R} alpha`), text[0]);
  assert.ok(cont.length >= 1 && cont.every((l) => l.startsWith(" │ ")), "continuation rows led by ' │ ' like renderToolCall");
  assert.equal(rows.join("").split(CAP_L).length - 1, 1, "one pill");
  assert.ok(text.slice(0, 1 + cont.length).every((l) => l.length <= 24), "within the pane");
  assert.ok(rows.slice(1, 1 + cont.length).every((l) => !l.includes("\x1b[45m")), "no pill background on continuation rows");
  assert.equal(text.slice(0, 1 + cont.length).join("").replace(/[\s│╭─]|cpd/gu, "").replace(/[\u{E0B6}\u{E0B4}]/gu, ""), sentence.replace(/\s/g, ""), "the sentence is whole, never cut");
});

test("an always-open block ends with the closing row of ╰󰬪, all 8", () => {
  for (const expanded of [false, true])
    for (const outputPad of [1, 0]) {
      const rows = draw(summaryMessage(), { expanded, outputPad });
      assert.equal(rows.at(-1), `${outputPad === 1 ? " " : ""}${GREY(`╰${TERM}`)}`, JSON.stringify(rows.at(-1)));
    }
});

test("an always-open block's last row is never a bare rail", () => {
  for (const width of [80, 20]) {
    const last = plain(draw(summaryMessage(), { width }).at(-1)!).trim();
    assert.ok(last !== "│" && last !== "", JSON.stringify(last));
    assert.ok(last.startsWith("╰"), last);
  }
});

test("a block with no body ends with ╰󰬪 and no toggle", () => {
  for (const expanded of [false, true]) {
    const rows = draw(cpdMessage("Nothing more", ""), { expanded });
    assert.equal(rows.at(-1), ` ${GREY(`╰${TERM}`)}`);
    const all = plain(rows.join("\n"));
    assert.ok(!all.includes("expand") && !all.includes("collapse"), "no toggle");
  }
});

test("a foldable block keeps its toggle with ╰ in 8 and the glyph and word in 7", () => {
  const m = cpdMessage("Worker of Job x", "line one\nline two");
  assert.equal(draw(m, { expanded: false }).at(-1), ` ${GREY("╰")}\x1b[37m\u{F0B26} expand\x1b[39m`);
  assert.equal(draw(m, { expanded: true }).at(-1), ` ${GREY("╰")}\x1b[37m\u{F0B2C} collapse\x1b[39m`);
  assert.equal(draw(m, { expanded: true })[0], ` ${GREY("╭─")}${PILL} \x1b[37mWorker of Job x\x1b[39m`);
});
