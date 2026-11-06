import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { ConnectionError, Refusal, main, transition, type Answer, type CallTool, type Request } from "../lib/jira-transition.ts";

const here = dirname(fileURLToPath(import.meta.url));
const helper = join(here, "..", "lib", "jira-transition.ts");
const CLOUD = "11111111-2222-3333-4444-555555555555";
const OTHER = "99999999-8888-7777-6666-555555555555";
const NAMES = { in_progress: "In Progress", in_review: "In Review", done: "Done" };
const SERVER = { url: "https://mcp.atlassian.com/v1/mcp/authv2" };
const ARGS = ["--coding-agent", "/x/index.js"];
const ENV = { PI_CODING_AGENT_DIR: "/agent" };

const json = (v: unknown): Answer => ({ isError: false, text: JSON.stringify(v, null, 2) });
const failed = (statusCode: number, message = "boom"): Answer => ({ isError: true, text: JSON.stringify({ error: true, message, statusCode }, null, 2) });
const jira = { id: CLOUD, url: "https://s.atlassian.net", scopes: ["read:jira-work", "write:jira-work"] };
const confluence = { id: CLOUD, url: "https://s.atlassian.net", scopes: ["read:page:confluence", "write:page:confluence"] };
const SUMMARY = "Retry failed payouts";
const issue = (name: string, key = "indeterminate", extra: Record<string, unknown> = {}): Answer => json({ key: "ABC-1", fields: { status: { name, statusCategory: { key } }, summary: SUMMARY, ...extra } });
type Names = { in_progress: string; in_review: string; done: string };
const TARGETS = ["in_progress", "in_review", "done"] as const;
/** What a successful read adds to every answer: the site, the summary and the estimate values read. */
const facts = (estimates: Record<string, unknown> = {}) => ({ cloud_id: CLOUD, summary: SUMMARY, estimates });
const tr = (id: string, name: string, to = name) => ({ id, name, to: { name: to } });
const moved = (statusName: string): Answer => json({ success: true, statusName });

type Table = Record<string, Answer | Error | ((args: Record<string, unknown>) => Answer | Error)>;

/** The request of swr-jira.md §3.1: Rust computes `target_name` and `later_names` from the policy's names; so does this. */
function request(target: Request["target"] = "in_review", names: Names = NAMES, estimate_fields: string[] = [], cloud_id: string | null = null): Request {
  const later = TARGETS.slice(TARGETS.indexOf(target) + 1).map((t) => names[t]).filter((n) => n !== "");
  return { server: SERVER, ticket: "ABC-1", target, cloud_id, target_name: names[target], later_names: later, estimate_fields };
}

function scripted(table: Table) {
  const calls: Array<{ name: string; args: Record<string, unknown>; timeoutMs: number }> = [];
  const call: CallTool = async (name, args, timeoutMs) => {
    calls.push({ name, args, timeoutMs });
    let a = table[name];
    if (a === undefined) throw new Error(`unscripted call ${name}`);
    if (typeof a === "function") a = a(args);
    if (a instanceof Error) throw a;
    return a;
  };
  return { call, calls };
}

const happy = (over: Table = {}): Table => ({
  getAccessibleAtlassianResources: json([confluence, jira]),
  getJiraIssue: issue("In Progress"),
  getTransitionsForJiraIssue: json({ transitions: [tr("21", "In Progress"), tr("2", "In Review")] }),
  transitionJiraIssue: moved("In Review"),
  ...over,
});

async function refusal(table: Table, req: Request = request()): Promise<{ refusal: Refusal; calls: ReturnType<typeof scripted>["calls"] }> {
  const f = scripted(table);
  try {
    await transition(f.call, req);
  } catch (e) {
    assert.ok(e instanceof Refusal, `a Refusal, got ${e}`);
    return { refusal: e, calls: f.calls };
  }
  assert.fail("expected a Refusal");
}

test("transition moves the issue with the first transition whose target status has the name of the target", async () => {
  const f = scripted(happy());
  const out = await transition(f.call, request());
  assert.deepEqual(f.calls, [
    { name: "getAccessibleAtlassianResources", args: {}, timeoutMs: 30000 },
    { name: "getJiraIssue", args: { cloudId: CLOUD, issueIdOrKey: "ABC-1", fields: ["status", "summary"] }, timeoutMs: 30000 },
    { name: "getTransitionsForJiraIssue", args: { cloudId: CLOUD, issueIdOrKey: "ABC-1" }, timeoutMs: 30000 },
    { name: "transitionJiraIssue", args: { cloudId: CLOUD, issueIdOrKey: "ABC-1", transition: { id: "2" } }, timeoutMs: 30000 },
  ]);
  assert.deepEqual(out, { ticket: "ABC-1", target: "in_review", outcome: "moved", from: "In Progress", to: "In Review", ...facts() });
});

test("transition reads only the status and summary and takes the site with both Jira scopes", async () => {
  const f = scripted(happy({ getAccessibleAtlassianResources: json([{ ...confluence, id: OTHER }, jira]) }));
  await transition(f.call, request());
  for (const c of f.calls.slice(1)) assert.equal(c.args.cloudId, CLOUD);
  const read = f.calls.find((c) => c.name === "getJiraIssue")!;
  assert.deepEqual(read.args.fields, ["status", "summary"]);
});

test("transition leaves a ticket at the target, in a later status or in a done category alone and makes no transition call", async () => {
  const cases: Array<[Request["target"], string, string, string]> = [
    ["in_review", "In Review", "indeterminate", "at_target"],
    ["in_review", "Won't Do", "done", "done_category"],
    ["in_progress", "In Review", "indeterminate", "past_target"],
    ["in_review", "Done", "indeterminate", "past_target"],
  ];
  for (const [target, status, category, why] of cases) {
    const f = scripted(happy({ getJiraIssue: issue(status, category) }));
    const out = await transition(f.call, request(target));
    assert.deepEqual(out, { ticket: "ABC-1", target, outcome: "unchanged", status_name: status, status_category: category, why, ...facts() }, why);
    assert.deepEqual(f.calls.map((c) => c.name), ["getAccessibleAtlassianResources", "getJiraIssue"], why);
  }
  // A later target with an empty name does not count as past.
  const f = scripted(happy({ getJiraIssue: issue("Done", "indeterminate") }));
  const out = await transition(f.call, request("in_review", { ...NAMES, done: "" }));
  assert.equal(out.outcome, "moved");
});

test("transition picks by target status name, never by transition name or id, and skips a transition to the current status", async () => {
  const list = [
    tr("2", "In Review"),
    tr("5", "Done", "Won't Do"),
    tr("31", "Done"),
    tr("6", "PR Approved", "Done"),
  ];
  const f = scripted(happy({ getJiraIssue: issue("In Review"), getTransitionsForJiraIssue: json({ transitions: list }), transitionJiraIssue: moved("Done") }));
  const out = await transition(f.call, request("done"));
  assert.deepEqual(f.calls.at(-1), { name: "transitionJiraIssue", args: { cloudId: CLOUD, issueIdOrKey: "ABC-1", transition: { id: "31" } }, timeoutMs: 30000 });
  assert.deepEqual(out, { ticket: "ABC-1", target: "done", outcome: "moved", from: "In Review", to: "Done", ...facts() });

  const g = scripted(happy({ getJiraIssue: issue("In Progress"), getTransitionsForJiraIssue: json({ transitions: [tr("21", "In Progress"), tr("6", "PR Approved", "Done"), tr("7", "In Review", "Backlog")] }), transitionJiraIssue: moved("Done") }));
  await transition(g.call, request("done"));
  assert.equal((g.calls.at(-1)!.args.transition as any).id, "6");
});

test("transition refuses jira_refused no_transition when no transition leads to the name", async () => {
  const r = await refusal(happy({ getTransitionsForJiraIssue: json({ transitions: [tr("21", "In Progress"), tr("7", "In Review", "Backlog")] }) }));
  assert.equal(r.refusal.code, "jira_refused");
  assert.equal(r.refusal.details.reason, "no_transition");
  assert.equal(r.refusal.exit, 1);
  assert.ok(!r.calls.some((c) => c.name === "transitionJiraIssue"));
});

test("transition refuses jira_refused issue_not_found on a 404 of the issue read", async () => {
  const text = JSON.stringify({ error: true, message: "{\"errorMessages\":[\"Issue does not exist or you do not have permission to see it.\"],\"errors\":{}}", rateLimit: { limit: 400, remaining: 399 }, statusCode: 404 }, null, 2);
  const r = await refusal(happy({ getJiraIssue: { isError: true, text } }));
  assert.equal(r.refusal.code, "jira_refused");
  assert.equal(r.refusal.details.reason, "issue_not_found");
  assert.deepEqual(r.calls.map((c) => c.name), ["getAccessibleAtlassianResources", "getJiraIssue"]);
});

test("transition refuses jira_refused site_ambiguous with two Jira sites", async () => {
  const r = await refusal(happy({ getAccessibleAtlassianResources: json([jira, { ...jira, id: OTHER, url: "https://t.atlassian.net" }]) }));
  assert.equal(r.refusal.code, "jira_refused");
  assert.equal(r.refusal.details.reason, "site_ambiguous");
  assert.deepEqual(r.calls.map((c) => c.name), ["getAccessibleAtlassianResources"]);
});

test("transition maps a missing sign-in, a 401 or 403 and no Jira-scoped site to jira_auth", async () => {
  const cases: Table[] = [
    { getAccessibleAtlassianResources: new ConnectionError("needs sign-in", true) },
    { getJiraIssue: failed(401) },
    { getJiraIssue: failed(403) },
    { getJiraIssue: { isError: true, text: "MCP error: requires sign-in" } },
    { getAccessibleAtlassianResources: json([confluence]) },
    { getAccessibleAtlassianResources: json([{ ...jira, scopes: ["read:jira-work"] }]) },
  ];
  for (const [i, over] of cases.entries()) {
    const r = await refusal(happy(over));
    assert.equal(r.refusal.code, "jira_auth", `case ${i}`);
    assert.equal(r.refusal.exit, 3, `case ${i}`);
    assert.equal(r.refusal.message, "Atlassian needs a sign-in. Run: pi mcp login atlassian", `case ${i}`);
  }
});

test("transition maps 429, 5xx, a 400 of the transition, a timeout and unparseable text to jira_failed", async () => {
  const cases: Array<[Table, string]> = [
    [{ getJiraIssue: failed(429) }, "getJiraIssue"],
    [{ getJiraIssue: failed(503) }, "getJiraIssue"],
    [{ transitionJiraIssue: failed(400, "Failed to transition issue: Bad Request. Transition id '2' is not valid for this issue.") }, "transitionJiraIssue"],
    [{ getTransitionsForJiraIssue: new Error("timed out after 30000ms") }, "getTransitionsForJiraIssue"],
    [{ getJiraIssue: { isError: false, text: "not json at all" } }, "getJiraIssue"],
    [{ getJiraIssue: json({ fields: {} }) }, "getJiraIssue"],
    [{ getTransitionsForJiraIssue: json({ nope: [] }) }, "getTransitionsForJiraIssue"],
    [{ transitionJiraIssue: json({ success: true }) }, "transitionJiraIssue"],
    [{ getAccessibleAtlassianResources: { isError: false, text: "{}" } }, "getAccessibleAtlassianResources"],
  ];
  for (const [i, [over, tool]] of cases.entries()) {
    const r = await refusal(happy(over));
    assert.equal(r.refusal.code, "jira_failed", `case ${i}`);
    assert.equal(r.refusal.exit, 1, `case ${i}`);
    assert.equal(r.refusal.details.tool, tool, `case ${i}`);
  }
});

function body(over: Record<string, unknown> = {}): string {
  return JSON.stringify({ server: SERVER, ticket: "ABC-1", target: "in_review", cloud_id: null, target_name: "In Review", later_names: ["Done"], estimate_fields: [], ...over });
}

test("main refuses a stdin that is not the request with usage before connecting", async () => {
  let opened = 0;
  const open = (async () => void opened++) as any;
  const cases: Array<[string[], NodeJS.ProcessEnv, string]> = [
    [ARGS, ENV, "not json"],
    [ARGS, ENV, body({ ticket: "abc-1" })],
    [ARGS, ENV, body({ ticket: "ABC-" })],
    [ARGS, ENV, body({ target: "later" })],
    [ARGS, ENV, body({ target_name: "" })],
    [ARGS, ENV, body({ server: { command: "x" } })],
    [ARGS, ENV, body({ server: {} })],
    [[], ENV, body()],
    [ARGS, {}, body()],
  ];
  for (const [i, [argv, env, stdin]] of cases.entries()) {
    const r = await main(argv, env, "/cwd", stdin, open);
    assert.equal(r.exit, 2, `case ${i}`);
    assert.equal((r.value as any).ok, false, `case ${i}`);
    assert.equal((r.value as any).error.code, "usage", `case ${i}`);
  }
  assert.equal(opened, 0);
});

test("main connects with the server of the request and prints one ok line for a move", async () => {
  const f = scripted(happy());
  let closed = 0;
  const opens: unknown[][] = [];
  const open = (async (...a: unknown[]) => {
    opens.push(a);
    return { call: f.call, close: async () => void closed++ };
  }) as any;
  const server = { url: "https://mcp.atlassian.com/v1/mcp/authv2", exposure: "deferred" };
  const r = await main(ARGS, ENV, "/cwd", body({ server }), open);
  assert.equal(opens.length, 1);
  assert.equal(opens[0]![0], "/x/index.js");
  assert.equal(opens[0]![1], "/agent");
  assert.equal(opens[0]![2], "/cwd");
  assert.deepEqual(opens[0]![3], server);
  assert.deepEqual(r, { value: { ok: true, ticket: "ABC-1", target: "in_review", outcome: "moved", from: "In Progress", to: "In Review", ...facts() }, exit: 0 });
  assert.equal(closed, 1);
});

test("main prints a failure as ok false with its code and exit and closes the connection", async () => {
  let closed = 0;
  const mk = (table: Table) => (async () => ({ call: scripted(table).call, close: async () => void closed++ })) as any;
  const r = await main(ARGS, ENV, "/cwd", body(), mk(happy({ getTransitionsForJiraIssue: json({ transitions: [] }) })));
  assert.equal(r.exit, 1);
  const v = r.value as any;
  assert.equal(v.ok, false);
  assert.equal(v.error.code, "jira_refused");
  assert.equal(typeof v.error.message, "string");
  assert.equal(v.error.details.reason, "no_transition");
  assert.equal(closed, 1);

  const auth = await main(ARGS, ENV, "/cwd", body(), mk(happy({ getJiraIssue: failed(401) })));
  assert.equal(auth.exit, 3);
  assert.equal((auth.value as any).error.code, "jira_auth");
  assert.equal(closed, 2);

  const thrown = await main(ARGS, ENV, "/cwd", body(), (async () => { throw new Error("cannot open"); }) as any);
  assert.equal(thrown.exit, 1);
  assert.equal((thrown.value as any).error.code, "jira_helper_failed");
  assert.equal(closed, 2);
});

test("run as a script it prints one usage line for an empty stdin", () => {
  const r = spawnSync(process.execPath, [helper, "--coding-agent", "/nonexistent/pi/index.js"], {
    input: "",
    env: { ...process.env, PI_CODING_AGENT_DIR: "/nonexistent/agent" },
    encoding: "utf8",
    timeout: 30000,
  });
  assert.equal(r.status, 2, r.stderr);
  const lines = r.stdout.trim().split("\n");
  assert.equal(lines.length, 1);
  assert.equal(JSON.parse(lines[0]!).error.code, "usage");
});

/** A `done` move of an issue at In Review whose single read carries `fields` (default: the estimate empty). */
const EST = (over: Table = {}, fields: string[] = ["customfield_10016"], read: Record<string, unknown> = { customfield_10016: null }) => ({
  req: request("done", NAMES, fields),
  table: happy({
    getJiraIssue: issue("In Review", "indeterminate", read),
    getTransitionsForJiraIssue: json({ transitions: [tr("31", "Done")] }),
    editJiraIssue: json({ id: "1" }),
    transitionJiraIssue: moved("Done"),
    ...over,
  }),
});
const C = (name: string, args: Record<string, unknown>) => ({ name, args, timeoutMs: 30000 });
const base = { cloudId: CLOUD, issueIdOrKey: "ABC-1" };

test("transition to done writes an empty estimate field with one edit and then moves the ticket", async () => {
  const { req, table } = EST();
  const f = scripted(table);
  const out = await transition(f.call, req);
  assert.deepEqual(f.calls, [
    C("getAccessibleAtlassianResources", {}),
    C("getJiraIssue", { ...base, fields: ["status", "summary", "customfield_10016"] }),
    C("getTransitionsForJiraIssue", base),
    C("editJiraIssue", { ...base, fields: { customfield_10016: 1 } }),
    C("transitionJiraIssue", { ...base, transition: { id: "31" } }),
  ]);
  assert.deepEqual(out, { ticket: "ABC-1", target: "done", outcome: "moved", from: "In Review", to: "Done", ...facts({ customfield_10016: 1 }) });
});

test("transition to done leaves a filled estimate field, zero included, and skips a field the issue does not carry", async () => {
  for (const read of [{ customfield_10016: 0 }, {}]) {
    const { req, table } = EST({}, ["customfield_10016", "customfield_10030"], read);
    const f = scripted(table);
    await transition(f.call, req);
    assert.deepEqual(f.calls.map((c) => c.name), ["getAccessibleAtlassianResources", "getJiraIssue", "getTransitionsForJiraIssue", "transitionJiraIssue"]);
    assert.deepEqual(f.calls[1], C("getJiraIssue", { ...base, fields: ["status", "summary", "customfield_10016", "customfield_10030"] }));
  }
});

test("transition to done writes every empty estimate field in one edit and only those", async () => {
  const { req, table } = EST({}, ["customfield_10016", "customfield_10030", "customfield_10040"], { customfield_10016: null, customfield_10030: 5, customfield_10040: null });
  const f = scripted(table);
  await transition(f.call, req);
  const edits = f.calls.filter((c) => c.name === "editJiraIssue");
  assert.deepEqual(edits, [C("editJiraIssue", { ...base, fields: { customfield_10016: 1, customfield_10040: 1 } })]);
});

test("transition to done does not move the ticket when the estimate read or write fails", async () => {
  const cases: Array<[Table, string, string, number | undefined]> = [
    [{ getJiraIssue: failed(503) }, "jira_failed", "getJiraIssue", 503],
    [{ editJiraIssue: failed(400, "Field 'customfield_10016' cannot be set") }, "jira_failed", "editJiraIssue", 400],
    [{ editJiraIssue: failed(401) }, "jira_auth", "", undefined],
    [{ editJiraIssue: new Error("timed out after 30000ms") }, "jira_failed", "editJiraIssue", undefined],
  ];
  for (const [i, [over, code, tool, status]] of cases.entries()) {
    const { req, table } = EST(over);
    const r = await refusal(table, req);
    assert.equal(r.refusal.code, code, `case ${i}`);
    assert.ok(!r.calls.some((c) => c.name === "transitionJiraIssue"), `case ${i}`);
    if (code === "jira_auth") assert.equal(r.refusal.exit, 3);
    else assert.equal(r.refusal.details.tool, tool, `case ${i}`);
    if (status !== undefined) assert.equal(r.refusal.details.status_code, status, `case ${i}`);
    if (i === 1) assert.ok(r.refusal.message.includes("cannot be set"));
  }
});

test("transition to done takes a successful edit as success without parsing its answer", async () => {
  const { req, table } = EST({ editJiraIssue: { isError: false, text: "ok, not JSON" } });
  const f = scripted(table);
  const out = await transition(f.call, req);
  assert.ok(f.calls.some((c) => c.name === "editJiraIssue"));
  assert.equal(out.outcome, "moved");
});

test("main refuses a request whose estimate_fields is missing, repeated or not a list of custom field ids with usage", async () => {
  let opened = 0;
  const open = (async () => {
    opened++;
    return { call: scripted(happy()).call, close: async () => {} };
  }) as any;
  const bad: unknown[] = [undefined, "customfield_1", ["summary"], ["customfield_"], ["customfield_1a"], [1], ["customfield_1", "customfield_1"]];
  for (const [i, v] of bad.entries()) {
    const r = await main(ARGS, ENV, "/cwd", body({ estimate_fields: v }), open);
    assert.equal(r.exit, 2, `case ${i}`);
    assert.equal((r.value as any).error.code, "usage", `case ${i}`);
  }
  assert.equal(opened, 0);
  for (const v of [[], ["customfield_10016"]]) {
    await main(ARGS, ENV, "/cwd", body({ estimate_fields: v }), open);
  }
  assert.equal(opened, 2);
});

test("transition writes no estimate for in_progress or in_review or with an empty list", async () => {
  const f = scripted(happy());
  await transition(f.call, request("in_review", NAMES, ["customfield_10016"]));
  assert.equal(f.calls.length, 4);
  const { req, table } = EST();
  const g = scripted(table);
  await transition(g.call, { ...req, estimate_fields: [] });
  assert.equal(g.calls.length, 4);
});

test("transition makes no estimate call when the ticket is unchanged or has no transition", async () => {
  const { req, table } = EST();
  const f = scripted({ ...table, getJiraIssue: issue("Done", "done") });
  await transition(f.call, req);
  assert.equal(f.calls.length, 2);
  const r = await refusal({ ...table, getTransitionsForJiraIssue: json({ transitions: [tr("2", "In Review")] }) }, req);
  assert.equal(r.refusal.details.reason, "no_transition");
  assert.ok(!r.calls.some((c) => c.name === "editJiraIssue"));
  assert.equal(r.calls.filter((c) => c.name === "getJiraIssue").length, 1);
});
