import assert from "node:assert/strict";
import { test } from "node:test";
import { type Component } from "@earendil-works/pi-tui";
import { eventMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";
import type { Theme } from "../lib/pi.ts";
import { Rendering } from "../lib/render.ts";
import { ev, ID } from "./fake.ts";

process.env.TZ = "Asia/Kolkata";

/** contract §6.1.1 (hs-d530 R1): cpd's message title pill is inverted (reverse video), caps 8, drawn with explicit SGR. */

// A theme that paints a recognisable code for `muted`, so that a leftover `theme.fg("muted")` shows.
const theme: Theme = {
  fg: (color, text) => `\x1b[38;5;99m<${color}>${text}\x1b[39m`,
  bg: (_color, text) => text,
  bold: (text) => text,
};

const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";

test("a φ message title pill is magenta 5, inverted", () => {
  const m = { ...eventMessage(ev(7, "job-merged", { text: "Job api-1 merged." }) as unknown as CpdEvent, ID), timestamp: Date.parse("2026-10-14T21:41:02.114Z") };
  const rendering = Object.assign(new Rendering(), { style: "compact" as const });
  const row = (rendering.render(m, { expanded: false, outputPad: 1 }, theme) as Component).render(200)[0]!;
  const title = `\x1b[1mφ\x1b[22m`;
  assert.ok(row.includes(`\x1b[35m${CAP_L}\x1b[7m${title}\x1b[27m\x1b[35m${CAP_R}\x1b[39m`), JSON.stringify(row));
  assert.ok(!row.includes("<muted>" + CAP_L) && !row.includes(`<muted>${title}`), "the pill is no theme-muted block");
});
