import assert from "node:assert/strict";
import { test } from "node:test";
import { MARKER, REPOS, batchMarker, day, run, status } from "./qa.mjs";

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
  assert.deepEqual(result.remaining, { issues: 18, prs: 8 });
});

test("caps are independent and cannot have negative allowances", () => {
  let calls = 0;
  const result = status(() => calls++ ? [] : Array.from({ length: 21 }, (_, i) => item(i)), now);
  assert.equal(result.can_create_issue, false);
  assert.equal(result.can_create_pr, true);
  assert.deepEqual(result.remaining, { issues: 0, prs: 10 });
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

test("refuses the twenty-first issue and eleventh PR without a POST", () => {
  for (const [command, count] of [["issue", 20], ["pr", 10]]) {
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

test("an explicit batch preserves day totals and counts only its own creations across repos", () => {
  const batch = "2026-11-13-reset-2";
  const rows = [item(1), item(2, { body: `${MARKER}\n${batchMarker(batch)}` }),
    item(3, { body: `${MARKER}\n${batchMarker(batch)}`, pull_request: {} }),
    item(4, { body: `${MARKER}\n${batchMarker("2026-11-13-another")}` })];
  const result = status(() => rows, now, batch);
  assert.deepEqual(result.counts, { issues: 2, prs: 2 });
  assert.deepEqual(result.day_counts, { issues: 6, prs: 2 });
  assert.deepEqual(status(() => rows, now, null).counts, result.day_counts);
});

test("wrong-day, empty and malformed batch identifiers fail before API access", () => {
  for (const batch of ["", "2026-11-12-reset-2", "2026-11-13-../bad", "arbitrary"])
    assert.throws(() => status(() => assert.fail("API must not run"), now, batch), /CPD_QA_BATCH/);
});

test("an exhausted ordinary day does not consume an explicitly authorized fresh batch", () => {
  const rows = [...Array.from({ length: 20 }, (_, n) => item(n)),
    ...Array.from({ length: 10 }, (_, n) => item(n + 20, { pull_request: {} }))];
  const api = () => {
    let calls = 0;
    return () => calls++ ? [] : rows;
  };
  const ordinary = status(api(), now, null);
  assert.equal(ordinary.can_create_issue, false);
  assert.equal(ordinary.can_create_pr, false);
  const reset = status(api(), now, "2026-11-13-reset-2");
  assert.deepEqual(reset.counts, { issues: 0, prs: 0 });
  assert.deepEqual(reset.day_counts, { issues: 20, prs: 10 });
  assert.deepEqual(reset.remaining, { issues: 20, prs: 10 });
});

test("batch creation is marked atomically and its own cap is enforced", () => {
  const batch = `${day(new Date())}-reset-2`;
  const body = `${MARKER}\n${batchMarker(batch)}`;
  let posts = 0;
  const api = (args, input) => {
    if (args.includes("POST")) {
      posts++;
      assert.equal(input.body, `${body}\n\nreason`);
      return { number: 42 };
    }
    return [];
  };
  assert.equal(run(["issue", REPOS[0], "title", "file"], api, () => "reason", batch).result.number, 42);
  assert.equal(posts, 1);
  let reads = 0;
  const full = () => reads++ ? [] : Array.from({ length: 20 }, (_, number) => item(number, {
    body, created_at: new Date().toISOString(),
  }));
  assert.equal(run(["issue", REPOS[0], "title", "file"], full, () => "reason", batch).exit, 3);
});
