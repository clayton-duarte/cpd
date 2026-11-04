import { expect, it } from "vitest";
import type { Urgency } from "../protocol/cockpit.ts";
import { inboxDisplay, jobDisplay, knownInboxKinds } from "./dango-status.ts";

it("dango status resolves job statuses to tone and glyph", () => {
  const table: Array<[string, string, string]> = [
    ["draft", "decision", "draft"],
    ["queued", "neutral", "queued"],
    ["running", "active", "active"],
    ["landing", "landing", "landing"],
    ["checking", "landing", "landing"],
    ["merging", "landing", "landing"],
    ["merged", "done", "done"],
    ["failed", "blocked", "blocked"],
    ["cancelled", "neutral", "idle"],
  ];
  for (const [status, tone, glyph] of table) {
    expect(jobDisplay(status), status).toEqual({ tone, glyph, label: status });
  }
  expect(jobDisplay("weird")).toEqual({ tone: "neutral", glyph: "idle", label: "weird" });
});

it("dango status resolves inbox urgency and kind pairs", () => {
  const urgencies: Array<[Urgency, number, string, string, string]> = [
    ["red", 1, "blocking", "blocked", "blocked"],
    ["yellow", 2, "delivery", "landing", "landing"],
    ["blue", 3, "review", "focus", "focus"],
    ["magenta", 4, "decide", "decision", "decision"],
    ["cyan", 5, "in flight", "active", "active"],
    ["white", 6, "unread", "unread", "unread"],
  ];
  expect(knownInboxKinds).toEqual([
    "engine-down", "loop-guard", "deploy", "pr-ready", "pr-closed", "review-requested",
    "dialog", "plan-review", "drafts-parked", "curate-retire", "unread",
  ]);
  for (const [urgency, tier, tierLabel, tone, glyph] of urgencies) {
    for (const kind of [...knownInboxKinds, "something-new"]) {
      // urgency alone decides; kind never changes the display
      expect(inboxDisplay(urgency), `${urgency}/${kind}`).toEqual({ tier, tierLabel, tone, glyph, label: tierLabel });
    }
  }
});
