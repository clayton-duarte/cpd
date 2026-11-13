import assert from "node:assert/strict";
import { test } from "node:test";
import { MARKER, REPOS, day, run, status } from "./qa.mjs";

const now = new Date("2026-11-13T17:00:00Z");
const item = (number, extra = {}) => ({ number, created_at: now.toISOString(), body: MARKER, ...extra });

test("Toronto midnight and both daylight-saving transitions", () => {
  for (const [utc, expected] of [
    ["2026-01-02T04:59:59Z", "2026-01-01"], ["2026-01-02T05:00:00Z", "2026-01-02"],
    ["2026-07-02T03:59:59Z", "2026-07-01"], ["2026-07-02T04:00:00Z", "2026-07-02"],
    ["2026-03-08T06:59:59Z", "2026-03-08"], ["2026-03-08T07:00:00Z", "2026-03-08"],
    ["2026-11-01T05:59:59Z", "2026-11-01"], ["2026-11-01T06:00:00Z", "2026-11-01"],
  ]) assert.equal(day(utc), expected);
});

test("counts both repositories and every page, including closed issues and draft PRs", () => {
  let calls = 0;
  const result = status((args) => {
    const index = calls++;
    assert.ok(args[0].startsWith(`repos/${REPOS[Math.floor(index / 2)]}/issues?state=all&since=`));
    assert.ok(args[0].endsWith(`&page=${index % 2 + 1}`));
    return index % 2 === 0
      ? [item(1, { state: "closed" }), ...Array.from({ length: 99 }, (_, i) => item(i + 10, { body: "unrelated" }))]
      : [item(3, { pull_request: {}, draft: true }), item(1), item(4, { created_at: "2026-11-13T04:59:59Z" })];
  }, now);
  assert.equal(calls, 4);
  assert.deepEqual(result.counts, { issues: 2, prs: 2 });
  assert.deepEqual(result.remaining, { issues: 8, prs: 3 });
});

test("caps are independent and cannot have negative allowances", () => {
  let calls = 0;
  const result = status(() => calls++ ? [] : Array.from({ length: 11 }, (_, i) => item(i)), now);
  assert.equal(result.can_create_issue, false);
  assert.equal(result.can_create_pr, true);
  assert.deepEqual(result.remaining, { issues: 0, prs: 5 });
});

test("fall-back day includes creations more than 24 hours ago", () => {
  const late = new Date("2026-11-02T04:59:59Z"); // 23:59 Toronto, at the end of a 25-hour day
  const result = status((args) => {
    const since = new Date(new URL(`https://github.com/${args[0]}`).searchParams.get("since"));
    assert.ok(since < new Date("2026-11-01T04:00:00Z"));
    return [item(1, { created_at: "2026-11-01T04:00:00Z" })];
  }, late);
  assert.equal(result.counts.issues, 2);
});

test("failed later page and non-progressing pagination never authorize creation", () => {
  const full = Array.from({ length: 100 }, (_, i) => item(i));
  let calls = 0;
  assert.throws(() => status(() => {
    if (calls++) throw new Error("page two unavailable");
    return full;
  }, now), /page two unavailable/);
  assert.throws(() => status(() => full, now), /no progress/);
});

test("API failure, invalid pages and malformed records fail closed before writes", () => {
  for (const reply of [{ message: "denied" }, [null], [{}], [item(1, { created_at: "invalid" })]]) {
    assert.throws(() => run(["issue", REPOS[0], "title", "body.md"], () => reply, () => "body"));
  }
  assert.throws(() => status(() => { throw new Error("rate limited"); }), /rate limited/);
});

test("refuses the eleventh issue and sixth PR without a POST", () => {
  for (const [command, count] of [["issue", 10], ["pr", 5]]) {
    let calls = 0;
    const api = (args) => {
      assert.ok(!args.includes("POST"));
      return calls++ ? [] : Array.from({ length: count }, (_, i) => item(i, {
        created_at: new Date().toISOString(), ...(command === "pr" ? { pull_request: {} } : {}),
      }));
    };
    const args = command === "issue" ? [command, REPOS[0], "title", "body.md"] : [command, REPOS[0], "branch", "title", "body.md"];
    assert.equal(run(args, api, () => "body").exit, 3);
    assert.equal(calls, 2);
  }
});

test("creation rechecks both repos then sends literal title/body and attribution atomically", () => {
  for (const command of ["issue", "pr"]) {
    let calls = 0;
    const api = (args, input) => {
      if (++calls <= 2) return [];
      assert.ok(args.includes("POST"));
      assert.equal(input.title, "literal $(command) `text`");
      assert.equal(input.body, `${MARKER}\n\nreason\nresearch`);
      if (command === "pr") assert.equal(input.base, "main");
      return { number: 42 };
    };
    const args = command === "issue" ? [command, REPOS[0], "literal $(command) `text`", "body.md"]
      : [command, REPOS[1], "qa/42", "literal $(command) `text`", "body.md"];
    assert.equal(run(args, api, () => "reason\nresearch").result.number, 42);
    assert.equal(calls, 3);
  }
});

test("bad commands, unknown repos and empty bodies never reach GitHub", () => {
  const api = () => { assert.fail("unexpected GitHub request"); };
  for (const args of [[], ["status", "extra"], ["issue", "other/repo", "title", "file"], ["pr", REPOS[0]]]) {
    assert.throws(() => run(args, api));
  }
  assert.throws(() => run(["issue", REPOS[0], "title", "file"], api, () => " "));
});
