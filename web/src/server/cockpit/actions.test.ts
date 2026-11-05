import { describe, expect, it } from "vitest";
import type { Cpd } from "./cli.ts";
import { runAction } from "./actions.ts";
import { WebError } from "../../protocol/domain.ts";

function fakeCpd(reply: () => Promise<Record<string, unknown>> = async () => ({ ok: true, done: true })): { cpd: Cpd; calls: (readonly string[])[] } {
  const calls: (readonly string[])[] = [];
  return { calls, cpd: { json: async (args) => { calls.push(args); return reply(); } } };
}

// strict fake: `plan show` answers workflows[]; `pr poll` (a GitHub GraphQL poll) is a failure
function planShowCpd(): { cpd: Cpd; calls: (readonly string[])[] } {
  const calls: (readonly string[])[] = [];
  const workflows = [{ key: "w1", pull_request: { repo: "acme/api", number: 3 } }];
  return {
    calls,
    cpd: {
      json: async (args) => {
        calls.push(args);
        if (args[0] === "pr" && args[1] === "poll") throw new WebError("cpd_failed", "rate limited", { cpdCode: "rate_limited", exitCode: 1 });
        if (args[0] === "plan" && args[1] === "show") return { ok: true, workflows };
        return { ok: true };
      },
    },
  };
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

  it("pr.merge without a pull resolves the stored pull through plan show", async () => {
    // `cpd pr merge <owner>/<repo>#<n>` (commands.md 9.10): the pull request comes from the Workflow's stored pull_request.
    const { cpd, calls } = planShowCpd();
    await runAction(cpd, { kind: "pr.merge", planId: "p1", workflow: "w1" });
    expect(calls).toEqual([["plan", "show", "--plan", "p1"], ["pr", "merge", "acme/api#3"]]);
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

describe("tree menu actions", () => {
  const cases: readonly (readonly [unknown, readonly string[]])[] = [
    [{ kind: "plan.new", planId: "my-plan", contextId: "c1", title: "My plan" }, ["plan", "new", "my-plan", "--context=c1", "--title=My plan", "--by-user"]],
    [{ kind: "context.rename", contextId: "c1", title: "New title" }, ["context", "rename", "c1", "--title=New title"]],
    [{ kind: "plan.rename", planId: "p1", title: "New title" }, ["plan", "rename", "--plan=p1", "--title=New title"]],
    [{ kind: "session.rename", sessionId: "s1", name: "-odd name" }, ["session", "rename", "--session=s1", "--", "-odd name"]],
    [{ kind: "session.fork", sourceId: "s1", name: "a fork" }, ["session", "fork", "s1", "--name=a fork"]],
    [{ kind: "session.close", sessionId: "s1" }, ["session", "close", "s1"]],
    [{ kind: "context.archive", contextId: "c1" }, ["context", "archive", "c1"]],
    [{ kind: "plan.archive", planId: "p1" }, ["plan", "archive", "--plan=p1"]],
    [{ kind: "session.archive", sessionId: "s1" }, ["session", "archive", "s1"]],
    [{ kind: "context.unarchive", contextId: "c1" }, ["context", "unarchive", "c1"]],
    [{ kind: "plan.unarchive", planId: "p1" }, ["plan", "unarchive", "--plan=p1"]],
    [{ kind: "session.unarchive", sessionId: "s1" }, ["session", "unarchive", "s1"]],
    [{ kind: "context.delete", contextId: "c1" }, ["context", "delete", "c1", "--yes"]],
    [{ kind: "plan.delete", planId: "p1" }, ["plan", "delete", "p1", "--yes"]],
  ];
  const busy = (): WebError => new WebError("cpd_failed", "chat slot is held", { cpdCode: "in_chat_slot", exitCode: 1 });

  it("each tree menu action runs its cpd argv", async () => {
    const { cpd, calls } = fakeCpd();
    for (const [action] of cases) await runAction(cpd, action);
    expect(calls).toEqual(cases.map(([, argv]) => argv));
  });

  it("archive and close retry once after session park when the chat slot is busy", async () => {
    const retried: unknown[] = [
      { kind: "context.archive", contextId: "c1" },
      { kind: "plan.archive", planId: "p1" },
      { kind: "session.archive", sessionId: "s1" },
      { kind: "session.close", sessionId: "s1" },
    ];
    for (const action of retried) {
      let first = true;
      const { cpd, calls } = fakeCpd(async () => {
        if (first) { first = false; throw busy(); }
        return { ok: true };
      });
      const out = await runAction(cpd, action);
      const argv = cases.find(([a]) => (a as { kind: string }).kind === (action as { kind: string }).kind)![1];
      expect(calls, (action as { kind: string }).kind).toEqual([argv, ["session", "park"], argv]);
      expect(out).toEqual({ ok: true });
    }
    // a second in_chat_slot error is not retried again
    const { cpd, calls } = fakeCpd(async () => { throw busy(); });
    await expect(runAction(cpd, { kind: "plan.archive", planId: "p1" })).rejects.toMatchObject({ details: { cpdCode: "in_chat_slot" } });
    expect(calls).toEqual([["plan", "archive", "--plan=p1"], ["session", "park"], ["plan", "archive", "--plan=p1"]]);
  });

  it("a failure other than in_chat_slot is returned without retry", async () => {
    const { cpd, calls } = fakeCpd(async () => {
      throw new WebError("cpd_failed", "plan has running jobs", { cpdCode: "archive_blocked", exitCode: 1 });
    });
    await expect(runAction(cpd, { kind: "plan.archive", planId: "p1" })).rejects.toMatchObject({ code: "cpd_failed", details: { cpdCode: "archive_blocked" } });
    expect(calls).toEqual([["plan", "archive", "--plan=p1"]]);
    // in_chat_slot on an action that does not need the slot (rename) is not retried either
    const busyRename = fakeCpd(async () => { throw busy(); });
    await expect(runAction(busyRename.cpd, { kind: "plan.rename", planId: "p1", title: "t" })).rejects.toMatchObject({ details: { cpdCode: "in_chat_slot" } });
    expect(busyRename.calls).toEqual([["plan", "rename", "--plan=p1", "--title=t"]]);
  });

  const polled = { ok: true, workflows: [{ key: "w1", pull_request: { repo: "acme/api", number: 3 } }] };

  it("pr.merge with a pull runs pr merge with it", async () => {
    const { cpd, calls } = fakeCpd();
    await runAction(cpd, { kind: "pr.merge", pull: "acme/api#3" });
    expect(calls).toEqual([["pr", "merge", "acme/api#3"]]);
  });

  it("pr.post with a pull and no workflow resolves the workflow through plan show", async () => {
    const { cpd, calls } = planShowCpd();
    await runAction(cpd, { kind: "pr.post", planId: "p1", pull: "acme/api#3" });
    expect(calls).toEqual([["plan", "show", "--plan", "p1"], ["pr", "post", "--workflow", "w1", "--plan", "p1"]]);
  });

  it("pr.repost runs pr repost with the workflow and plan", async () => {
    const { cpd, calls } = fakeCpd();
    await runAction(cpd, { kind: "pr.repost", planId: "p1", workflow: "w1" });
    expect(calls).toEqual([["pr", "repost", "--workflow", "w1", "--plan", "p1"]]);
  });

  it("pr post without a matching workflow fails with cpd_malformed_json", async () => {
    const { cpd } = planShowCpd();
    await expect(runAction(cpd, { kind: "pr.post", planId: "p1", pull: "acme/api#9" })).rejects.toMatchObject({ code: "cpd_malformed_json" });
  });

  it("mergeArgs and prVerbArgs look up workflows[] with cpd plan show --plan <planId> and never call cpd pr poll", async () => {
    const m = planShowCpd();
    await runAction(m.cpd, { kind: "pr.merge", planId: "p7", workflow: "w1" });
    const post = planShowCpd();
    await runAction(post.cpd, { kind: "pr.post", planId: "p7", pull: "acme/api#3" });
    const repost = planShowCpd();
    await runAction(repost.cpd, { kind: "pr.repost", planId: "p7", pull: "acme/api#3" });
    for (const { calls } of [m, post, repost]) {
      expect(calls[0]).toEqual(["plan", "show", "--plan", "p7"]);
      expect(calls.some((c) => c[0] === "pr" && c[1] === "poll")).toBe(false);
    }
    expect(m.calls.at(-1)).toEqual(["pr", "merge", "acme/api#3"]);
    expect(post.calls.at(-1)).toEqual(["pr", "post", "--workflow", "w1", "--plan", "p7"]);
    expect(repost.calls.at(-1)).toEqual(["pr", "repost", "--workflow", "w1", "--plan", "p7"]);
  });
});
