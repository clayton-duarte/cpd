// The helper's contract of docs/architecture/swr-jira.md §3: one getJiraIssue with fields, the site id from the request,
// and the facts of the read in the answer. A fake MCP client: no network, no process.
import assert from "node:assert/strict";
import { test } from "node:test";
import { transition, type Answer, type CallTool, type Request } from "../lib/jira-transition.ts";

const CLOUD = "11111111-2222-3333-4444-555555555555";
const NAMES = { in_progress: "In Progress", in_review: "In Review", done: "Done" };
const json = (v: unknown): Answer => ({ isError: false, text: JSON.stringify(v, null, 2) });
const jira = { id: CLOUD, url: "https://s.atlassian.net", scopes: ["read:jira-work", "write:jira-work"] };
const read = (name: string, key = "indeterminate"): Answer =>
  json({ key: "ABC-1", fields: { status: { name, statusCategory: { key } }, summary: "Retry failed payouts", customfield_10016: null } });

function fake(table: Record<string, Answer>) {
  const calls: Array<{ name: string; args: Record<string, unknown> }> = [];
  const call: CallTool = async (name, args) => {
    calls.push({ name, args });
    const a = table[name];
    if (a === undefined) throw new Error(`unscripted call ${name}`);
    return a;
  };
  return { call, calls };
}

const request = (over: Partial<Request> = {}): Request => ({
  server: { url: "https://mcp.atlassian.com/v1/mcp/authv2" },
  ticket: "ABC-1",
  target: "in_review",
  names: NAMES,
  cloud_id: CLOUD,
  target_name: "In Review",
  later_names: ["Done"],
  estimate_fields: [],
  ...over,
});

test("jira_helper_makes_one_get_issue_call_with_fields", async () => {
  const f = fake({
    getJiraIssue: read("In Progress"),
    getTransitionsForJiraIssue: json({ transitions: [{ id: "2", name: "In Review", to: { name: "In Review" } }] }),
    transitionJiraIssue: json({ success: true, statusName: "In Review" }),
  });
  const out = await transition(f.call, request());
  assert.deepEqual(
    f.calls.map((c) => c.name),
    ["getJiraIssue", "getTransitionsForJiraIssue", "transitionJiraIssue"],
    "a cached cloud_id skips the site call, and the read is made once",
  );
  assert.deepEqual(f.calls[0]!.args, { cloudId: CLOUD, issueIdOrKey: "ABC-1", fields: ["status", "summary"] });
  assert.equal(out.outcome, "moved");
  assert.equal((out as any).cloud_id, CLOUD);
  assert.equal((out as any).summary, "Retry failed payouts");
  assert.deepEqual((out as any).estimates, {});
});

test("a null cloud_id resolves the site once, then reads once with the estimate fields", async () => {
  const f = fake({
    getAccessibleAtlassianResources: json([jira]),
    getJiraIssue: read("In Review"),
  });
  const out = await transition(f.call, request({ cloud_id: null, target: "in_progress", target_name: "In Progress", later_names: ["In Review", "Done"], estimate_fields: ["customfield_10016"] }));
  assert.deepEqual(
    f.calls.map((c) => c.name),
    ["getAccessibleAtlassianResources", "getJiraIssue"],
  );
  assert.deepEqual(f.calls[1]!.args.fields, ["status", "summary", "customfield_10016"]);
  assert.deepEqual(out, {
    ticket: "ABC-1",
    target: "in_progress",
    outcome: "unchanged",
    cloud_id: CLOUD,
    status_name: "In Review",
    status_category: "indeterminate",
    why: "past_target",
    summary: "Retry failed payouts",
    estimates: { customfield_10016: null },
  });
});
