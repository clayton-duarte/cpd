import assert from "node:assert/strict";
import { beforeEach, test } from "node:test";
import { cpdApproveBypass, externalBlock, ghWriteBypass, grantSlackSend, isSlackSend, reasonText, resetSlackGrants, slackScriptBypass, takeSlackSend } from "../lib/externalguard.ts";

// Decision pro-d451: raw gh comment writes, raw cpd pr approve, slack-post.ts and ungranted Slack sends are refused.

beforeEach(() => resetSlackGrants());

const refused = (fn: (c: string) => string | undefined, commands: string[]) => {
  for (const c of commands) {
    const m = fn(c);
    assert.ok(typeof m === "string" && m !== "", `refused: ${c}`);
  }
};
const allowed = (fn: (c: string) => string | undefined, commands: string[]) => {
  for (const c of commands) assert.equal(fn(c), undefined, `allowed: ${c}`);
};

test("ghWriteBypass refuses gh pr comment, gh issue comment and gh pr review", () => {
  refused(ghWriteBypass, ["gh pr comment 12 --body hi", "gh issue comment 3 -b x", "gh pr review 12 --approve", "gh pr review 12 --comment -b x"]);
  assert.match(ghWriteBypass("gh pr comment 12 --body hi") ?? "", /gh pr comment/);
});

test("ghWriteBypass refuses gh api writes to comments, reviews and replies endpoints", () => {
  refused(ghWriteBypass, [
    "gh api repos/o/r/issues/12/comments -f body=hi",
    "gh api -X POST repos/o/r/pulls/12/reviews -f event=APPROVE",
    "gh api --method POST repos/o/r/pulls/12/comments/99/replies -f body=x",
    "gh api repos/o/r/pulls/12/comments --method=POST -f body=x",
    "gh api -X PATCH repos/o/r/issues/comments/5 -f body=x",
  ]);
});

test("ghWriteBypass refuses gh api graphql mutations addComment, addPullRequestReview and addPullRequestReviewThreadReply", () => {
  for (const m of ["addComment", "addPullRequestReview", "addPullRequestReviewThreadReply"]) {
    refused(ghWriteBypass, [`gh api graphql -f query='mutation { ${m}(input: {}) { clientMutationId } }'`]);
  }
});

test("ghWriteBypass sees the command behind env, sudo, time, timeout and a pipeline", () => {
  refused(ghWriteBypass, [
    "env GH_TOKEN=x gh pr comment 1 -b x",
    "FOO=1 gh pr comment 1 -b x",
    "sudo gh pr comment 1 -b x",
    "time gh pr review 1 --approve",
    "timeout 30 gh pr comment 1 -b x",
    "timeout 30 env A=b gh issue comment 1 -b x",
    "echo hi | gh pr comment 1 -F -",
    "ls && timeout 5 gh pr comment 1 -b x",
  ]);
});

test("ghWriteBypass lets gh reads through", () => {
  allowed(ghWriteBypass, [
    "gh pr view 12",
    "gh pr list --state open",
    "gh pr checks 12",
    "gh issue view 3 --comments",
    "gh api repos/o/r/pulls/12/comments",
    "gh api repos/o/r/issues/12/comments --method GET",
    "gh api graphql -f query='query { viewer { login } }'",
    "timeout 30 gh pr view 1",
    "echo gh pr comment",
    "ls",
  ]);
  assert.notEqual(ghWriteBypass("gh pr comment 1 -b x"), undefined, "while a write is still refused");
});

test("slackScriptBypass refuses slack-post.ts in bash and lets other commands through", () => {
  refused(slackScriptBypass, ["node /home/u/skills/slack/slack-post.ts --channel C1", "node slack-post.ts", "/home/u/skills/slack-post.ts hi", "./slack-post.ts x", "FOO=1 node a/b/slack-post.ts"]);
  allowed(slackScriptBypass, ["ls", "cat slack-post.ts.md", "node other.ts", "echo hi"]);
});

test("cpdApproveBypass refuses a raw cpd pr approve", () => {
  refused(cpdApproveBypass, ["cpd pr approve --kind post --workflow w", "env X=1 cpd pr approve --kind reply --workflow w --to c1", "timeout 5 cpd pr approve --kind post --workflow w"]);
  assert.match(cpdApproveBypass("cpd pr approve --kind post --workflow w") ?? "", /cpd pr approve/);
  allowed(cpdApproveBypass, ["cpd pr show --workflow w", "cpd pr post --workflow w", "cpd host approve", "echo cpd pr approve"]);
});

const SEND = "mcp__slack__slack_send_message";
const DRAFT = "mcp__slack__slack_send_message_draft";

test("a slack grant is one-shot and matches the tool, channel, message and thread exactly", () => {
  assert.equal(isSlackSend(SEND), true);
  assert.equal(isSlackSend(DRAFT), true);
  assert.equal(isSlackSend("mcp__slack__slack_read_channel"), false);
  const p = { channel: "C1", message: "hello", thread_ts: "1.2" };
  const input = { channel_id: "C1", message: "hello", thread_ts: "1.2" };
  grantSlackSend("slack_send_message", p);
  assert.equal(takeSlackSend(DRAFT, input), false, "other tool");
  assert.equal(takeSlackSend(SEND, { ...input, channel_id: "C2" }), false, "channel");
  assert.equal(takeSlackSend(SEND, { ...input, message: "hello!" }), false, "message");
  assert.equal(takeSlackSend(SEND, { ...input, thread_ts: "1.3" }), false, "thread");
  assert.equal(takeSlackSend(SEND, { channel_id: "C1", message: "hello" }), false, "no thread");
  assert.equal(takeSlackSend(SEND, input), true, "exact match");
  assert.equal(takeSlackSend(SEND, input), false, "consumed");
  grantSlackSend("slack_send_message_draft", { channel: "C1", message: "m" });
  assert.equal(takeSlackSend(DRAFT, { channel_id: "C1", message: "m" }), true);
  grantSlackSend("slack_send_message", { channel: "C1", message: "m" });
  resetSlackGrants();
  assert.equal(takeSlackSend(SEND, { channel_id: "C1", message: "m" }), false, "reset");
});

test("externalBlock refuses an ungranted slack send tool and lets one granted send through once", () => {
  const args = { channel_id: "C1", message: "hi" };
  const reason = externalBlock(SEND, args);
  assert.ok(reason !== undefined && reason !== "" && !reason.includes("\n"), "a one-line reason");
  assert.ok(reason.includes("cpd_external_approve"));
  assert.equal(reason, externalBlock(SEND, args), "same reason, nothing consumed");
  assert.match(reasonText("gh pr comment"), /cpd_external_approve/);
  assert.ok(!reasonText("gh pr comment").includes("\n"));
  grantSlackSend("slack_send_message", { channel: "C1", message: "hi" });
  assert.equal(externalBlock(SEND, args), undefined, "granted send passes");
  assert.notEqual(externalBlock(SEND, args), undefined, "only once");
  assert.notEqual(externalBlock("bash", { command: "gh pr comment 1 -b x" }), undefined);
  assert.notEqual(externalBlock("bash", { command: "cpd pr approve --kind post --workflow w" }), undefined);
  assert.notEqual(externalBlock("bash", { command: "node slack-post.ts" }), undefined);
  assert.equal(externalBlock("bash", { command: "ls" }), undefined);
  assert.equal(externalBlock("read", { path: "x" }), undefined);
});
