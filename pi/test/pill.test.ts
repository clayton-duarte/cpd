import assert from "node:assert/strict";
import { test } from "node:test";
import { type Component } from "@earendil-works/pi-tui";
import { eventMessage } from "../lib/delivery.ts";
import type { CpdEvent } from "../lib/event.ts";
import type { Theme } from "../lib/pi.ts";
import { Rendering } from "../lib/render.ts";
import { ev, ID } from "./fake.ts";

/** contract §6.1.1 (hs-d530 R1): cpd's message title pill is background 8, text 7, caps 8, drawn with explicit SGR. */

// A theme that paints a recognisable code for `muted`, so that a leftover `theme.fg("muted")` shows.
const theme: Theme = {
  fg: (color, text) => `\x1b[38;5;99m<${color}>${text}\x1b[39m`,
  bg: (_color, text) => text,
  bold: (text) => text,
};

const CAP_L = "\u{E0B6}";
const CAP_R = "\u{E0B4}";

test("a cpd message title pill is magenta 5 with text 0", () => {
  const m = eventMessage(ev(7, "job-merged", { text: "Job api-1 merged." }) as unknown as CpdEvent, ID);
  const rendering = Object.assign(new Rendering(), { style: "compact" as const });
  const row = (rendering.render(m, { expanded: false, outputPad: 1 }, theme) as Component).render(200)[0]!;
  const title = "cpd Job api-1 merged.";
  assert.ok(row.includes(`\x1b[35m${CAP_L}\x1b[45m\x1b[30m${title}\x1b[49m\x1b[35m${CAP_R}\x1b[39m`), JSON.stringify(row));
  assert.ok(!row.includes("\x1b[7m"), "no reverse video");
  assert.ok(!row.includes("<muted>" + CAP_L) && !row.includes(`<muted>${title}`), "the pill is no theme-muted block");
});
