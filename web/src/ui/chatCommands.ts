import type { CockpitAction, ContextTreeModel } from "../protocol/cockpit.ts";

export type ChatCommandName = "archive" | "unarchive";

export type ChatArchiveTarget =
  | { readonly kind: "context"; readonly id: string }
  | { readonly kind: "plan"; readonly id: string }
  | { readonly kind: "session"; readonly id: string };

export type ChatCommandResult =
  | { readonly kind: "none" }
  | { readonly kind: "error"; readonly message: string }
  | { readonly kind: "action"; readonly action: CockpitAction };

/** Resolve the object a chat command acts on from the current cockpit tree. */
export function resolveChatArchiveTarget(tree: ContextTreeModel | null, sessionId: string): ChatArchiveTarget | null {
  if (tree === null) return null;
  for (const context of tree.contexts) {
    const contextSession = context.sessions.find((session) => session.ref.id === sessionId);
    if (contextSession !== undefined) {
      if (contextSession.ref.role === "manager") return { kind: "context", id: context.id };
      if (contextSession.ref.role === "lead") return { kind: "session", id: sessionId };
      return null;
    }
    for (const plan of context.plans) {
      const planSession = plan.sessions.find((session) => session.ref.id === sessionId);
      if (planSession === undefined) continue;
      if (planSession.ref.role === "lead" && plan.leadSessionId === sessionId) return { kind: "plan", id: plan.id };
      return null;
    }
  }
  return null;
}

export function chatCommandAction(command: ChatCommandName, target: ChatArchiveTarget): CockpitAction {
  switch (target.kind) {
    case "context":
      return command === "archive"
        ? { kind: "context.archive", contextId: target.id }
        : { kind: "context.unarchive", contextId: target.id };
    case "plan":
      return command === "archive"
        ? { kind: "plan.archive", planId: target.id }
        : { kind: "plan.unarchive", planId: target.id };
    case "session":
      return command === "archive"
        ? { kind: "session.archive", sessionId: target.id }
        : { kind: "session.unarchive", sessionId: target.id };
  }
}

/**
 * Parse and resolve an exact chat command. Unknown slash commands return `none` and continue to
 * the agent; recognized commands never do.
 */
export function resolveChatCommand(text: string, tree: ContextTreeModel | null, sessionId: string): ChatCommandResult {
  const trimmed = text.trim();
  const match = /^\/([a-z][a-z0-9-]*)(?:\s+([\s\S]*))?$/i.exec(trimmed);
  if (match === null) return { kind: "none" };
  const name = match[1]?.toLowerCase();
  if (name !== "archive" && name !== "unarchive") return { kind: "none" };
  if ((match[2] ?? "").trim() !== "") return { kind: "error", message: `Use /${name} with no arguments.` };

  const target = resolveChatArchiveTarget(tree, sessionId);
  if (target === null) return { kind: "error", message: `No archive target was found for this chat.` };
  return { kind: "action", action: chatCommandAction(name, target) };
}
