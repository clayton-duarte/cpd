import assert from "node:assert/strict";
import { test } from "node:test";
import { CustomMessageComponent } from "../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-message.js";
import type { Theme } from "../lib/pi.ts";
import { Rendering } from "../lib/render.ts";

/** hs-d557 (2), (5): in a cpd block the lines (`╭─`, `│ `, `╰`) are 8 on every row; only the toggle glyph and its word are 7. */

const theme: Theme = { fg: (_c, t) => t, bg: (_c, t) => t, bold: (t) => t };
const GREY = (s: string): string => `\x1b[90m${s}\x1b[39m`;
const RAIL = GREY("│ ");
const long = "word abcd ".repeat(20);
const CONTENTS = ["a\n\nb\n" + long, "\x1b[31mred\nspans\x1b[0m plain\n" + long, "\x1b[1;4mbold\x1b[0m\n\x1b[38;5;5m" + long + "\n\x1b[49mend", "x\ty\r\nz"];

test("every row of a cpd block has the rail in 8", () => {
  for (const content of CONTENTS)
    for (const customType of ["cpd", "cpd-plan-review-summary"])
      for (const outputPad of [1, 0])
        for (const expanded of [true, false]) {
          const message = { role: "custom" as const, timestamp: 0, customType, content, display: true, details: { review: "r1" } };
          const where = JSON.stringify({ customType, outputPad, expanded, content });
          const component = new CustomMessageComponent(message, new Rendering().render, undefined, outputPad);
          component.setExpanded(expanded);
          const rows = component.render(40).filter((l: string) => l.trim() !== "");
          const open = customType !== "cpd";
          assert.ok(rows.length >= 2, where);
          assert.ok(rows[0]!.includes(GREY("╭─")), `head lead is 8: ${where}`);
          const body = open || expanded ? rows.slice(1, open ? undefined : -1) : [];
          for (const r of body) assert.ok(r.replace(/^ /, "").startsWith(RAIL), `rail cell ${JSON.stringify(r)} in ${where}`);
          if (!open) {
            const glyph = expanded ? "\u{F0063} collapse" : "\u{F004B} expand";
            assert.equal(rows[rows.length - 1]!.replace(/^ /, ""), `${GREY("╰")}\x1b[37m${glyph}\x1b[39m`, `closing row: ${where}`);
          }
        }
});
