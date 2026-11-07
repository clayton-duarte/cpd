// The shared cpd command builder (docs/contract.md §5.6, the web-to-CLI argv contract):
// every cpd argv the web sends goes through here, so one test can prove the whole surface
// parses against the real CLI grammar. `web/src/server/cockpit/argv-contract.test.ts` is that
// proof; a command with no variant here has no test.

/** One cpd command the web sends; a discriminated union (docs/contract.md §2.1 conventions). */
export type CpdCommand =
  | { readonly kind: "version" }
  | { readonly kind: "incident.status" }
  | { readonly kind: "config.read" }
  | { readonly kind: "config.experimental.read" }
  | { readonly kind: "config.tiers.set"; readonly tiers: TiersArgv }
  | { readonly kind: "config.experimental.set"; readonly key: string; readonly on: boolean }
  | { readonly kind: "context.list" }
  | { readonly kind: "context.curation"; readonly contextId: string; readonly limit: number }
  | { readonly kind: "context.rename"; readonly contextId: string; readonly title: string }
  | { readonly kind: "context.archive"; readonly contextId: string }
  | { readonly kind: "context.unarchive"; readonly contextId: string }
  | { readonly kind: "context.delete"; readonly contextId: string }
  | { readonly kind: "context.retire"; readonly contextId: string; readonly approve?: string; readonly reject?: string }
  | { readonly kind: "plan.list" }
  | { readonly kind: "plan.show"; readonly planId: string }
  | { readonly kind: "plan.new"; readonly planId: string; readonly contextId: string; readonly title: string }
  | { readonly kind: "plan.rename"; readonly planId: string; readonly title: string }
  | { readonly kind: "plan.archive"; readonly planId: string }
  | { readonly kind: "plan.unarchive"; readonly planId: string }
  | { readonly kind: "plan.delete"; readonly planId: string }
  | { readonly kind: "plan.decide"; readonly review: string; readonly decision: "approve" | "reject"; readonly planId: string; readonly feedbackFile?: string }
  | { readonly kind: "session.list"; readonly archived?: boolean }
  | { readonly kind: "session.show"; readonly sessionId: string }
  | { readonly kind: "session.send"; readonly sessionId: string; readonly text: string; readonly unblocks: string; readonly origin?: string }
  | { readonly kind: "session.retier"; readonly sessionId: string; readonly tier: string }
  | { readonly kind: "session.retierAll" }
  | { readonly kind: "session.park" }
  | { readonly kind: "session.rename"; readonly sessionId: string; readonly name: string }
  | { readonly kind: "session.fork"; readonly sourceId: string; readonly name: string }
  | { readonly kind: "session.close"; readonly sessionId: string }
  | { readonly kind: "session.archive"; readonly sessionId: string }
  | { readonly kind: "session.unarchive"; readonly sessionId: string }
  | { readonly kind: "inbox.list" }
  | { readonly kind: "inbox.snooze"; readonly itemId: number; readonly duration: string }
  | { readonly kind: "inbox.dismiss"; readonly itemId: number }
  | { readonly kind: "inbox.claim"; readonly itemId: number }
  | { readonly kind: "inbox.approve"; readonly itemId: number }
  | { readonly kind: "events.list"; readonly eventKind: string; readonly since: number; readonly limit: number }
  | { readonly kind: "events.follow"; readonly since?: number; readonly untilIdle?: boolean }
  | { readonly kind: "watch.ensure" }
  | { readonly kind: "pr.post"; readonly verb: "post" | "repost"; readonly workflow: string; readonly planId: string }
  | { readonly kind: "pr.merge"; readonly pull: string }
  | { readonly kind: "cockpit.serve" }
  | { readonly kind: "swr.badge.get"; readonly sessionId: string }
  | { readonly kind: "swr.badge.put"; readonly sessionId: string; readonly entryId: string; readonly label: string; readonly score: number; readonly text: string; readonly tone: string; readonly others?: string };

/** One `--role <tier>` of `config tiers set` (commands.md §14.2a). */
export interface TiersArgv {
  readonly auxiliar: string;
  readonly worker: string;
  readonly lead: string;
  readonly manager: string;
}

/** The argv after the program name `cpd` (so `"inbox"` is its first element, not `"cpd"`). */
export function toArgv(cmd: CpdCommand): string[] {
  switch (cmd.kind) {
    case "version": return ["--version"];
    case "incident.status": return ["incident", "status"];
    case "config.read": return ["config"];
    case "config.experimental.read": return ["config", "experimental"];
    case "config.tiers.set":
      return ["config", "tiers", "set",
        "--auxiliar", cmd.tiers.auxiliar,
        "--worker", cmd.tiers.worker,
        "--lead", cmd.tiers.lead,
        "--manager", cmd.tiers.manager];
    case "config.experimental.set": return ["config", "experimental", cmd.key, cmd.on ? "on" : "off"];
    case "context.list": return ["context", "list"];
    case "context.curation": return ["context", "curation", `--context=${cmd.contextId}`, `--limit=${cmd.limit}`];
    case "context.rename": return ["context", "rename", cmd.contextId, `--title=${cmd.title}`];
    case "context.archive": return ["context", "archive", cmd.contextId];
    case "context.unarchive": return ["context", "unarchive", cmd.contextId];
    case "context.delete": return ["context", "delete", cmd.contextId, "--yes"];
    case "context.retire": {
      const argv = ["context", "retire", `--context=${cmd.contextId}`];
      if (cmd.approve !== undefined && cmd.approve !== "") argv.push(`--approve=${cmd.approve}`);
      if (cmd.reject !== undefined && cmd.reject !== "") argv.push(`--reject=${cmd.reject}`);
      return argv;
    }
    case "plan.list": return ["plan", "list"];
    case "plan.show": return ["plan", "show", "--plan", cmd.planId];
    case "plan.new": return ["plan", "new", cmd.planId, `--context=${cmd.contextId}`, `--title=${cmd.title}`, "--by-user"];
    case "plan.rename": return ["plan", "rename", `--plan=${cmd.planId}`, `--title=${cmd.title}`];
    case "plan.archive": return ["plan", "archive", `--plan=${cmd.planId}`];
    case "plan.unarchive": return ["plan", "unarchive", `--plan=${cmd.planId}`];
    case "plan.delete": return ["plan", "delete", cmd.planId, "--yes"];
    case "plan.decide": {
      const argv = ["plan", "decide", cmd.review, cmd.decision === "approve" ? "--approve" : "--reject"];
      if (cmd.feedbackFile !== undefined && cmd.feedbackFile !== "") argv.push("--feedback-file", cmd.feedbackFile);
      argv.push("--plan", cmd.planId);
      return argv;
    }
    case "session.list": return cmd.archived === true ? ["session", "list", "--archived"] : ["session", "list"];
    case "session.show": return ["session", "show", cmd.sessionId];
    case "session.send": {
      const argv = ["session", "send", cmd.sessionId];
      if (cmd.origin !== undefined && cmd.origin !== "") argv.push("--origin", cmd.origin);
      argv.push("--unblocks", cmd.unblocks, "--", cmd.text);
      return argv;
    }
    case "session.retier": return ["session", "retier", cmd.sessionId, "--tier", cmd.tier];
    case "session.retierAll": return ["session", "retier", "--all"];
    case "session.park": return ["session", "park"];
    case "session.rename": return ["session", "rename", `--session=${cmd.sessionId}`, "--", cmd.name];
    case "session.fork": return ["session", "fork", cmd.sourceId, `--name=${cmd.name}`];
    case "session.close": return ["session", "close", cmd.sessionId];
    case "session.archive": return ["session", "archive", cmd.sessionId];
    case "session.unarchive": return ["session", "unarchive", cmd.sessionId];
    case "inbox.list": return ["inbox", "list"];
    case "inbox.snooze": return ["inbox", "snooze", String(cmd.itemId), cmd.duration];
    case "inbox.dismiss": return ["inbox", "dismiss", String(cmd.itemId)];
    case "inbox.claim": return ["inbox", "claim", String(cmd.itemId)];
    case "inbox.approve": return ["inbox", "approve", String(cmd.itemId)];
    case "events.list": return ["events", "list", "--kind", cmd.eventKind, "--since", String(cmd.since), "--limit", String(cmd.limit)];
    case "events.follow": {
      const argv = ["events", "follow"];
      if (cmd.since !== undefined) argv.push("--since", String(cmd.since));
      if (cmd.untilIdle === true) argv.push("--until-idle");
      return argv;
    }
    case "watch.ensure": return ["watch", "--ensure"];
    case "pr.post": return ["pr", cmd.verb, "--workflow", cmd.workflow, "--plan", cmd.planId];
    case "pr.merge": return ["pr", "merge", cmd.pull];
    case "cockpit.serve": return ["cockpit", "serve"];
    case "swr.badge.get": return ["swr", "badge", "get", cmd.sessionId];
    case "swr.badge.put": {
      const argv = ["swr", "badge", "put", cmd.sessionId, "--entry", cmd.entryId, "--label", cmd.label, "--score", String(cmd.score), "--text", cmd.text, "--tone", cmd.tone];
      if (cmd.others !== undefined && cmd.others !== "") argv.push("--others", cmd.others);
      return argv;
    }
  }
}

const TIERS: TiersArgv = { auxiliar: "GX", worker: "H3", lead: "S3", manager: "O3" };

/** Every command the web can send, for the argv contract test: one per variant, plus the
 * optional-flag combinations the call sites produce (commands.md §5.6). */
export function allCpdCommands(): CpdCommand[] {
  return [
    { kind: "version" },
    { kind: "incident.status" },
    { kind: "config.read" },
    { kind: "config.experimental.read" },
    { kind: "config.tiers.set", tiers: TIERS },
    { kind: "config.experimental.set", key: "write_curation", on: true },
    { kind: "config.experimental.set", key: "durable_workers", on: false },
    { kind: "context.list" },
    { kind: "context.curation", contextId: "c1", limit: 10 },
    { kind: "context.rename", contextId: "c1", title: "T" },
    { kind: "context.archive", contextId: "c1" },
    { kind: "context.unarchive", contextId: "c1" },
    { kind: "context.delete", contextId: "c1" },
    { kind: "context.retire", contextId: "c1" },
    { kind: "context.retire", contextId: "c1", approve: "cpd-d1" },
    { kind: "context.retire", contextId: "c1", reject: "cpd-d1" },
    { kind: "plan.list" },
    { kind: "plan.show", planId: "p1" },
    { kind: "plan.new", planId: "p1", contextId: "c1", title: "T" },
    { kind: "plan.rename", planId: "p1", title: "T" },
    { kind: "plan.archive", planId: "p1" },
    { kind: "plan.unarchive", planId: "p1" },
    { kind: "plan.delete", planId: "p1" },
    { kind: "plan.decide", review: "r1", decision: "approve", planId: "p1" },
    { kind: "plan.decide", review: "r1", decision: "reject", planId: "p1" },
    { kind: "plan.decide", review: "r1", decision: "reject", planId: "p1", feedbackFile: "/tmp/f.md" },
    { kind: "session.list" },
    { kind: "session.list", archived: true },
    { kind: "session.show", sessionId: "s1" },
    { kind: "session.send", sessionId: "s1", text: "hi", unblocks: "User message from the web chat", origin: "webui" },
    { kind: "session.send", sessionId: "s1:manager", text: "hi", unblocks: "WebUI error toast (x)" },
    { kind: "session.retier", sessionId: "s1", tier: "S3" },
    { kind: "session.retierAll" },
    { kind: "session.park" },
    { kind: "session.rename", sessionId: "s1", name: "N" },
    { kind: "session.fork", sourceId: "s1", name: "N" },
    { kind: "session.close", sessionId: "s1" },
    { kind: "session.archive", sessionId: "s1" },
    { kind: "session.unarchive", sessionId: "s1" },
    { kind: "inbox.list" },
    { kind: "inbox.snooze", itemId: 4, duration: "1h" },
    { kind: "inbox.snooze", itemId: 4, duration: "tomorrow" },
    { kind: "inbox.dismiss", itemId: 4 },
    { kind: "inbox.claim", itemId: 4 },
    { kind: "inbox.approve", itemId: 4 },
    { kind: "events.list", eventKind: "attempt-ended", since: 0, limit: 5000 },
    { kind: "events.follow", since: 0 },
    { kind: "events.follow", since: 0, untilIdle: true },
    { kind: "watch.ensure" },
    { kind: "pr.post", verb: "post", workflow: "w1", planId: "p1" },
    { kind: "pr.post", verb: "repost", workflow: "w1", planId: "p1" },
    { kind: "pr.merge", pull: "acme/api#3" },
    { kind: "cockpit.serve" },
    { kind: "swr.badge.get", sessionId: "s1" },
    { kind: "swr.badge.put", sessionId: "s1", entryId: "e1", label: "L", score: 1, text: "T", tone: "green" },
    { kind: "swr.badge.put", sessionId: "s1", entryId: "e1", label: "L", score: 1, text: "T", tone: "green", others: "[]" },
  ];
}
