import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/** The lines from the heading line to the next heading of the same or a higher level. */
function section(text: string, heading: string): string {
  const level = heading.match(/^#+/)![0].length;
  const lines = text.split("\n");
  const start = lines.indexOf(heading);
  assert.notEqual(start, -1, `heading ${heading} found`);
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {
    const m = lines[i].match(/^(#+) /);
    if (m && m[1].length <= level) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end).join("\n");
}

function line(text: string, prefix: string): string {
  const found = text.split("\n").find((l) => l.startsWith(prefix));
  assert.ok(found, `line starting ${prefix} found`);
  return found;
}

test("a failed open or update Job is retried and never cancelled because a cancel of an open Job stands", () => {
  const lead = readFileSync(new URL("../skills/cpd-lead/SKILL.md", import.meta.url), "utf8");
  assert.doesNotMatch(lead, /cancel[^.\n]*\badds? a fresh/i, "no sentence says a cancel makes cpd add a fresh Job");
  const row = line(section(lead, "### Dispatching"), "| `job-failed`");
  const bullet = line(section(lead, "#### Failed Jobs"), "- **A PR's open or update Job**");
  const check = line(section(lead, "### Mechanical checks"), "- **`job-failed` of the open Job**");
  for (const l of [row, bullet, check]) {
    assert.match(l, /`cpd job retry <k> --tier <T>`/);
    assert.match(l, /never cancel/i);
  }
  assert.match(bullet, /commands\.md §5\.3/);
  assert.match(bullet, /cpd pr open --pr <key>/);
  assert.match(bullet, /cpd pr rebase --pr <key>/);
});
