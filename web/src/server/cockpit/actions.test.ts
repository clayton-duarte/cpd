import { describe, expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { runAction } from "./actions.ts";
import { WebError } from "../../protocol/domain.ts";

function fakeCpd(reply: () => Promise<Record<string, unknown>> = async () => ({ ok: true, done: true })): { cpd: Cpd; calls: (readonly string[])[] } {
  const calls: (readonly string[])[] = [];
  return { calls, cpd: { json: async (args) => { calls.push(args); return reply(); } } };
}

describe("actions", () => {
  it("inbox snooze, dismiss and approve map to cpd inbox commands", async () => {
    const { cpd, calls } = fakeCpd();
    await runAction(cpd, { kind: "inbox.snooze", itemId: 4, duration: "1h" });
    await runAction(cpd, { kind: "inbox.dismiss", itemId: 5 });
    await runAction(cpd, { kind: "inbox.approve", itemId: 6 });
    await runAction(cpd, { kind: "inbox.claim", itemId: 7 });
    expect(calls).toEqual([
      ["inbox", "snooze", "4", "1h"],
      ["inbox", "dismiss", "5"],
      ["inbox", "approve", "6"],
      ["inbox", "claim", "7"],
    ]);
  });

  it("pr post maps to cpd pr post for a workflow", async () => {
    const { cpd, calls } = fakeCpd(async () => ({ ok: true, number: 12 }));
    const out = await runAction(cpd, { kind: "pr.post", planId: "p1", workflow: "w1" });
    expect(calls).toEqual([["pr", "post", "--workflow", "w1", "--plan", "p1"]]);
    expect(out).toEqual({ ok: true, number: 12 });
  });

  it("inbox.snooze tomorrow passes the next 09:00 local time", async () => {
    // cpd computes the next 09:00 local itself (commands.md 21.3): the web passes the word.
    const { cpd, calls } = fakeCpd();
    await runAction(cpd, { kind: "inbox.snooze", itemId: 4, duration: "tomorrow" });
    expect(calls).toEqual([["inbox", "snooze", "4", "tomorrow"]]);
  });

  it("pr.merge runs the cpd merge command for the workflow", async () => {
    // `cpd pr merge <owner>/<repo>#<n>` (commands.md 9.10): the pull request comes from the Workflow's stored pull_request.
    const { cpd, calls } = fakeCpd(async () => ({ ok: true, workflows: [{ key: "w1", pull_request: { repo: "acme/api", number: 3 } }], stored: true }));
    await runAction(cpd, { kind: "pr.merge", planId: "p1", workflow: "w1" });
    expect(calls.at(-1)).toEqual(["pr", "merge", "acme/api#3"]);
  });

  it("engine.start runs cpd watch ensure", async () => {
    const { cpd, calls } = fakeCpd();
    await runAction(cpd, { kind: "engine.start" });
    expect(calls).toEqual([["watch", "--ensure"]]);
  });

  it("an action error code is surfaced to the caller", async () => {
    const { cpd } = fakeCpd(async () => {
      throw new WebError("cpd_failed", "no such item", { cpdCode: "item_not_found", exitCode: 1 });
    });
    await expect(runAction(cpd, { kind: "inbox.dismiss", itemId: 99 })).rejects.toMatchObject({
      code: "cpd_failed",
      details: { cpdCode: "item_not_found" },
    });
  });

  it("unknown action kinds are rejected", async () => {
    const { cpd, calls } = fakeCpd();
    await expect(runAction(cpd, { kind: "inbox.explode", itemId: 1 })).rejects.toMatchObject({ code: "unknown_action" });
    expect(calls).toEqual([]);
  });
});
