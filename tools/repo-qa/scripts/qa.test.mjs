import assert from "node:assert/strict";
import { test } from "node:test";
import { createGuard, validateConfig, validateRun } from "./qa.mjs";

const fixture = { version: 1, scope: "sample-qa", timezone: "America/Toronto", limits: { issues: 20, prs: 10 },
  repositories: [{ name: "sample/primary", base: "main" }, { name: "sample/related", base: "develop" }] };
const { MARKER, REPOS, batchMarker, day, run, status } = createGuard(fixture);

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
      if (command === "pr") assert.equal(input.base, "develop");
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
    assert.throws(() => status(() => assert.fail("API must not run"), now, batch), /QA_BATCH/);
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

test("invalid config and run metadata fail closed", () => {
  for (const change of [{ version: 2 }, { scope: "bad -->" }, { timezone: "Not/AZone" }, { limits: { issues: -1, prs: 1 } },
    { limits: { issues: 1.5, prs: 2 } }, { repositories: [] }, { typo: true },
    { repositories: [{ name: "https://github.com/a/b", base: "main" }] },
    { repositories: [{ name: "a/b", base: "" }] },
    { repositories: [{ name: "a/b", base: "main" }, { name: "A/B", base: "main" }] }])
    assert.throws(() => validateConfig({ ...fixture, ...change }));
  const good = { version: 1, id: "sample-run", started_at: "2026-11-13T12:00:00Z", limits: { issues: 10, prs: 5 }, authorization: "User instruction in issue #1" };
  for (const change of [{ id: "" }, { authorization: "" }, { started_at: "invalid" }, { started_at: "2026-11-14T00:00:00Z" }, { limits: { issues: 1 } }, { unknown: true }]) {
    assert.throws(() => status(() => assert.fail("API must not run"), now, null, { ...good, ...change }));
  }
  assert.equal(validateRun(good, now).id, "sample-run");
});

test("separate configs isolate repositories, scopes, clocks and PR bases", () => {
  const other = createGuard({ ...fixture, scope: "other-qa", timezone: "Asia/Tokyo", repositories: [{ name: "another/project", base: "release/stable" }] });
  assert.equal(other.day("2026-11-13T16:00:00Z"), "2026-11-14");
  assert.equal(day("2026-11-13T16:00:00Z"), "2026-11-13");
  assert.deepEqual(other.status(() => [item(1)], now).counts, { issues: 0, prs: 0 });
  const api = (args, payload) => {
    assert.ok(args[0].startsWith("repos/another/project/"));
    if (!args.includes("POST")) return [];
    assert.equal(payload.base, "release/stable");
    assert.ok(payload.body.startsWith(other.MARKER));
    return { number: 1 };
  };
  assert.equal(other.run(["pr", "another/project", "qa/fix", "title", "body"], api, () => "reason").exit, 0);
  assert.throws(() => other.run(["issue", REPOS[0], "title", "body"], () => assert.fail("API must not run"), () => "reason"));
});

test("a run cap is independent of day and reset allowances and survives a new session", () => {
  const guard = createGuard(fixture);
  const descriptor = { version: 1, id: "pilot", started_at: "2026-11-13T12:00:00Z", limits: { issues: 2, prs: 1 }, authorization: "User: run 2/1" };
  const body = `${MARKER}\n${guard.runMarker(descriptor.id)}`;
  const rows = [item(1, { body }), item(2, { body }), item(3, { body, pull_request: {} }), item(4)];
  const api = () => {
    let calls = 0;
    return () => calls++ ? [] : rows;
  };
  for (const instance of [guard, createGuard(JSON.parse(JSON.stringify(fixture)))]) {
    const s = instance.status(api(), now, "2026-11-13-reset-2", descriptor);
    assert.deepEqual(s.counts, { issues: 0, prs: 0 });
    assert.deepEqual(s.run.counts, { issues: 2, prs: 1 });
    assert.deepEqual(s.day_counts, { issues: 3, prs: 1 });
    assert.deepEqual(s.remaining, { issues: 0, prs: 0 });
    assert.equal(s.can_create_issue, false);
    assert.equal(s.can_create_pr, false);
  }
});

test("long run counting extends the REST window and retains work across calendar days", () => {
  const guard = createGuard(fixture);
  const descriptor = { version: 1, id: "long-run", started_at: "2026-11-01T10:00:00Z", limits: { issues: 10, prs: 5 }, authorization: "User: multi-day run" };
  let calls = 0;
  const s = guard.status((args) => {
    const since = new Date(new URL(`https://example.test/${args[0]}`).searchParams.get("since"));
    assert.ok(since < new Date(descriptor.started_at));
    return calls++ ? [] : [item(1, { created_at: "2026-11-01T10:00:00Z", body: `${MARKER}\n${guard.runMarker(descriptor.id)}` })];
  }, now, null, descriptor);
  assert.equal(s.counts.issues, 0);
  assert.equal(s.run.counts.issues, 1);
  assert.equal(s.remaining.issues, 9);
});

test("run creation marks intent atomically and independently refuses each exhausted cap", () => {
  const guard = createGuard(fixture);
  const descriptor = { version: 1, id: "literal-run", started_at: new Date(Date.now() - 60_000).toISOString(), limits: { issues: 1, prs: 0 }, authorization: "User: one finding, no PR" };
  let posts = 0;
  const api = (args, payload) => {
    if (!args.includes("POST")) return [];
    posts++;
    assert.equal(payload.body, `${MARKER}\n${guard.runMarker(descriptor.id)}\n\nreason`);
    return { number: 1 };
  };
  assert.equal(guard.run(["issue", REPOS[0], "title", "body"], api, () => "reason", null, descriptor).exit, 0);
  assert.equal(guard.run(["pr", REPOS[0], "head", "title", "body"], api, () => "reason", null, descriptor).exit, 3);
  assert.equal(posts, 1);
  const exhausted = { ...descriptor, limits: { issues: 0, prs: 1 } };
  assert.equal(guard.run(["issue", REPOS[0], "title", "body"], api, () => "reason", null, exhausted).exit, 3);
});
