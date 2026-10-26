import assert from "node:assert/strict";
import { test } from "node:test";
import { type Component } from "@earendil-works/pi-tui";
import { eventMessage as rawEventMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";
import type { Theme } from "../lib/pi.ts";
import { Rendering } from "../lib/render.ts";
import { ev, ID } from "./fake.ts";

/**
 * contract §6.1.1 rule (4) and `docs/commands.md` §15.9 (hs-d708): cpd's mark φ is rendered bold wherever it renders in
 * the pi chat, and only the glyph is bold — the colon and the text after it are not. Bold is SGR 1 (open) and SGR 22
 * (close); the mark sits inside the line's own colour, so the body's text colour 37/39 is unchanged.
 */

process.env.TZ = "Asia/Kolkata";
const TIME_MS = Date.parse("2026-10-14T21:41:02.114Z");
const eventMessage = (...args: Parameters<typeof rawEventMessage>) => ({ ...rawEventMessage(...args), timestamp: TIME_MS });

const theme: Theme = {
  fg: (color, text) => `\x1b[38;5;99m<${color}>${text}\x1b[39m`,
  bg: (_color, text) => text,
  bold: (text) => `\x1b[1m${text}\x1b[22m`,
};

const render = (text: string, expanded = false): string[] => {
  const m = eventMessage(ev(7, "job-merged", { text }) as unknown as CpdEvent, ID);
  const rendering = Object.assign(new Rendering(), { style: "compact" as const });
  return (rendering.render(m, { expanded, outputPad: 1 }, theme) as Component).render(200);
};

const BOLD_PHI = "\x1b[1mφ\x1b[22m";

test("a message body draws a bold phi", () => {
  const rows = render("φ adapter: session show: bad_output: x", true);
  const body = rows.find((l) => l.startsWith("\x1b[90m│ \x1b[39m"));
  assert.ok(body !== undefined, `the body is a rail line: ${JSON.stringify(rows)}`);
  assert.equal(
    body,
    `\x1b[90m│ \x1b[39m\x1b[37m${BOLD_PHI} adapter: session show: bad_output: x\x1b[39m`,
    `the φ glyph is SGR 1/22 inside the line's colour 37/39: ${JSON.stringify(body)}`
  );
  assert.equal((body!.match(/\x1b\[1m/g) ?? []).length, 1, `only the φ glyph is bold: ${JSON.stringify(body)}`);
  assert.equal((body!.match(/\x1b\[22m/g) ?? []).length, 1, `the bold closes once with SGR 22: ${JSON.stringify(body)}`);
});
