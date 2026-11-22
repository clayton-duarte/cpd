import { expect, it } from "vitest";
import type { ContextTreeModel, SessionNode } from "../protocol/cockpit.ts";
import type { SessionRef } from "../protocol/domain.ts";
import { resolveChatArchiveTarget, resolveChatCommand } from "./chatCommands.ts";

const session = (id: string, role: SessionRef["role"], planId: string | null = null): SessionNode => ({
  ref: { id, key: id, runner: "durable", role, state: "idle", title: id },
  contextId: "ctx",
  planId,
  jobKey: null,
  archived: false,
  dot: { state: "idle", attention: null },
});

const tree: ContextTreeModel = {
  cursor: 1,
  contexts: [{
    id: "ctx",
    title: "Context",
    archived: false,
    records: 0,
    openQuestions: 0,
    sessions: [session("mgr", "manager"), session("fork", "lead")],
    plans: [{
      id: "plan",
      contextId: "ctx",
      title: "Plan",
      phase: "dispatch",
      archived: false,
      counts: { draft: 0, queued: 0, running: 0, merged: 0, failed: 0, cancelled: 0 },
      leadSessionId: "lead",
      sessions: [session("lead", "lead", "plan"), session("worker", "worker", "plan")],
    }],
  }],
};

it("routes a Manager chat to the recursive Context actions", () => {
  expect(resolveChatCommand(" /archive ", tree, "mgr")).toEqual({
    kind: "action", action: { kind: "context.archive", contextId: "ctx" },
  });
  expect(resolveChatCommand("/unarchive", tree, "mgr")).toEqual({
    kind: "action", action: { kind: "context.unarchive", contextId: "ctx" },
  });
});

it("routes a Plan Lead chat to the Plan actions", () => {
  expect(resolveChatCommand("/archive", tree, "lead")).toEqual({
    kind: "action", action: { kind: "plan.archive", planId: "plan" },
  });
  expect(resolveChatCommand("/unarchive", tree, "lead")).toEqual({
    kind: "action", action: { kind: "plan.unarchive", planId: "plan" },
  });
});

it("routes a standalone or forked Lead to the Session actions", () => {
  expect(resolveChatArchiveTarget(tree, "fork")).toEqual({ kind: "session", id: "fork" });
  expect(resolveChatCommand("/archive", tree, "fork")).toEqual({
    kind: "action", action: { kind: "session.archive", sessionId: "fork" },
  });
});

it("rejects recognized commands with arguments instead of sending them to the agent", () => {
  expect(resolveChatCommand("/archive now", tree, "mgr")).toEqual({
    kind: "error", message: "Use /archive with no arguments.",
  });
});

it("leaves unknown slash commands for the agent", () => {
  expect(resolveChatCommand("/review", tree, "mgr")).toEqual({ kind: "none" });
  expect(resolveChatCommand("hello", tree, "mgr")).toEqual({ kind: "none" });
});

it("reports a chat with no archive target", () => {
  expect(resolveChatCommand("/archive", tree, "worker")).toEqual({
    kind: "error", message: "No archive target was found for this chat.",
  });
  expect(resolveChatCommand("/archive", null, "mgr")).toEqual({
    kind: "error", message: "No archive target was found for this chat.",
  });
});
