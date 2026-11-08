// @vitest-environment node
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");

// Every WebUI file that contains an empty-state branch (the 7 branches named in the
// empty-states scout report). Each must render the Empty component, never a bare <p>.
const EMPTY_FILES = [
  "cockpit/ContextTree.tsx",
  "cockpit/PlanView.tsx",
  "cockpit/ManagerView.tsx",
  "App.tsx",
  "cockpit/Inbox.tsx",
  "chat/Transcript.tsx",
] as const;

// The attributes a bare <p> must never carry: they mark an empty state, whose markup must
// come from the Empty component (a div[data-slot="empty"]) instead.
const EMPTY_MARKERS = [
  // data-empty, as a boolean attribute
  /(?:^|\s)data-empty(?=[\s=>/]|$)/,
  /data-part\s*=\s*["']empty["']/,
  /data-part\s*=\s*["']plan-empty["']/,
  /data-part\s*=\s*["']manager-empty["']/,
];

// The suite title is worded to match the harness: `cpd test run` filters web tests with the
// pinned title as an unescaped vitest --testNamePattern, so the class `[data-empty]` in the
// test title consumes the `a` of "paragraph" here and the exact test below is selected.
describe("no empty branch uses a bare paragraph", () => {
  it("no empty branch uses a bare p[data-empty]", () => {
    const bareEmptyParagraphs: string[] = [];
    const filesWithoutEmptyComponent: string[] = [];

    for (const rel of EMPTY_FILES) {
      const src = read(rel);

      for (const match of src.matchAll(/<p\b[^>]*>/g)) {
        if (EMPTY_MARKERS.some((marker) => marker.test(match[0]))) {
          const line = src.slice(0, match.index).split("\n").length;
          bareEmptyParagraphs.push(`${rel}:${line}: ${match[0].replace(/\s+/g, " ").trim()}`);
        }
      }

      if (!/<Empty[\s/>]/.test(src)) {
        filesWithoutEmptyComponent.push(rel);
      }
    }

    expect(bareEmptyParagraphs).toEqual([]);
    expect(filesWithoutEmptyComponent).toEqual([]);
  });
});
