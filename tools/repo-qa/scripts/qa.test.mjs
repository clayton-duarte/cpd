import assert from "node:assert/strict";
import { test } from "node:test";
import { createGuard, validateConfig, validateRun, weeklyAllowanceFloor } from "./qa.mjs";

const fixture = { version: 1, scope: "sample-qa", timezone: "America/Toronto", limits: { issues: 20, prs: 10 },
  repositories: [{ name: "sample/primary", base: "main" }, { name: "sample/related", base: "develop" }] };
const { MARKER, REPOS, batchMarker, day, run, status } = createGuard(fixture);

const now = new Date("2026-11-13T17:00:00Z");
const weeklyRun = { version: 1, id: "weekly", started_at: now.toISOString(), limits: null, authorization: "Synthetic User weekly approval" };
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

test("weekly threshold replaces cardinal caps without erasing audit counts", () => {
  const sample = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
    remaining_percent: 50, resets_at: "2026-11-16T00:14:45Z", ordinary_usage_allowed: true, spend_control_reached: false };
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  for (const percent of [49, 50, 51]) {
    const guard = createGuard(config, { clock: () => now, readUsage: () => ({ ...sample, remaining_percent: percent }) });
    const s = guard.status(() => [item(1)], now, null, weeklyRun);
    assert.equal(s.can_create_issue, percent >= 50);
    assert.equal(s.can_create_pr, percent >= 50);
    assert.deepEqual(s.remaining, { issues: null, prs: null });
    assert.deepEqual(s.counts, { issues: 2, prs: 0 });
  }
  const capped = createGuard({ ...config, limits: { issues: 0, prs: 1 } }, { clock: () => now, readUsage: () => sample });
  assert.equal(capped.status(() => [], now, null, weeklyRun).can_create_issue, false);
  assert.equal(capped.status(() => [], now, null, weeklyRun).can_create_pr, true);
  const descriptor = { version: 1, id: "weekly", started_at: now.toISOString(), limits: null, authorization: "User: weekly policy" };
  assert.equal(createGuard(config, { clock: () => now, readUsage: () => sample }).status(() => [], now, null, descriptor).run.remaining, null);
});

test("weekly policy rejects missing policy, unsafe freshness and malformed telemetry before GitHub", () => {
  assert.throws(() => createGuard({ ...fixture, limits: null }), /require/);
  for (const weekly_allowance of [null, {}, { min_remaining_percent: 0, max_age_seconds: 60 },
    { min_remaining_percent: 50, max_age_seconds: 61 }, { min_remaining_percent: 50, max_age_seconds: 0 }]) {
    assert.throws(() => createGuard({ ...fixture, weekly_allowance }));
  }
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  const sample = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
    remaining_percent: 70, resets_at: "2026-11-16T00:14:45Z", ordinary_usage_allowed: true, spend_control_reached: false };
  const noApi = () => assert.fail("invalid telemetry must fail before GitHub");
  for (const change of [{ sampled_at: "2026-11-13T16:58:59Z" }, { sampled_at: "2026-11-13T17:00:01Z" }, { sampled_at: null },
    { resets_at: now.toISOString() }, { resets_at: "2027-01-01T00:00:00Z" }, { remaining_percent: NaN }, { remaining_percent: -1 },
    { window_minutes: 300 }, { limit_id: "other" }, { ordinary_usage_allowed: null }, { spend_control_reached: null }, { token: "forbidden" }]) {
    assert.throws(() => createGuard(config, { clock: () => now, readUsage: () => ({ ...sample, ...change }) }).status(noApi, now));
  }
  assert.throws(() => createGuard(config, { clock: () => now, readUsage: () => { throw new Error("reader unavailable"); } }).status(noApi, now), /reader unavailable/);
  for (const change of [{ ordinary_usage_allowed: false }, { spend_control_reached: true }]) {
    assert.equal(createGuard(config, { clock: () => now, readUsage: () => ({ ...sample, ...change }) }).status(() => [], now, null, weeklyRun).can_create_pr, false);
  }
});

test("status refreshes weekly usage after pagination before authorizing work", () => {
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 1 } };
  for (const scenario of ["stale", "revoked", "fresh", "rounding", "missing", "reset"]) {
    let current = now.getTime(), reads = 0;
    const initial = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
      remaining_percent: 70, resets_at: new Date(current + 86400_000).toISOString(), ordinary_usage_allowed: true, spend_control_reached: false };
    const guard = createGuard(config, { clock: () => new Date(current), readUsage: () => {
      if (++reads === 1 || scenario === "stale") return initial;
      if (scenario === "missing") throw new Error("reader unavailable");
      return { ...initial, sampled_at: new Date(current).toISOString(),
        remaining_percent: scenario === "revoked" ? 49 : 70,
        resets_at: scenario === "reset" ? new Date(current + 2 * 86400_000).toISOString() : scenario === "rounding" ? new Date(Date.parse(initial.resets_at) + 1000).toISOString() : initial.resets_at };
    } });
    const api = () => { current += 1250; return []; };
    if (["stale", "missing", "reset"].includes(scenario)) assert.throws(() => guard.run(["status"], api, undefined, null, weeklyRun), /stale|unavailable|window/);
    else {
      const result = guard.run(["status"], api, undefined, null, weeklyRun);
      assert.equal(result.result.can_create_issue, ["fresh", "rounding"].includes(scenario));
      assert.equal(result.result.weekly_allowance.remaining_percent, ["fresh", "rounding"].includes(scenario) ? 70 : 49);
    }
    assert.equal(reads, 2);
  }
});

test("creation refreshes usage through status and immediately before POST", () => {
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  const sample = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
    remaining_percent: 70, resets_at: new Date(now.getTime() + 86400_000).toISOString(), ordinary_usage_allowed: true, spend_control_reached: false };
  for (const changedRead of [2, 3]) for (const scenario of ["threshold", "stale", "fresh", "reset"]) {
    let reads = 0, posts = 0;
    const guard = createGuard(config, { clock: () => now, readUsage: () => {
      ++reads;
      if (reads < changedRead) return sample;
      return { ...sample, remaining_percent: scenario === "threshold" ? 49 : 70,
        sampled_at: scenario === "stale" ? new Date(now.getTime() - 61000).toISOString() : sample.sampled_at,
        resets_at: scenario === "reset" ? new Date(now.getTime() + 2 * 86400_000).toISOString() : sample.resets_at };
    } });
    const api = (args) => { if (args.includes("POST")) { posts++; return { number: 1 }; } return []; };
    if (["stale", "reset"].includes(scenario)) assert.throws(() => guard.run(["issue", REPOS[0], "title", "body"], api, () => "reason", null, weeklyRun), /stale|window/);
    else assert.equal(guard.run(["issue", REPOS[0], "title", "body"], api, () => "reason", null, weeklyRun).exit, scenario === "fresh" ? 0 : 3);
    assert.equal(posts, scenario === "fresh" ? 1 : 0);
    assert.equal(reads, scenario === "fresh" ? 3 : changedRead);
  }
});

test("PR upkeep reports conflicted owned PRs and ignores unmarked ones without writing", () => {
  const guard = createGuard(fixture, { clock: () => now });
  const result = guard.run(["pr-status"], (args) => {
    assert.equal(args.length, 1, "all calls are GETs");
    if (args[0].includes("/issues?")) return [];
    if (args[0].includes("/pulls?")) return args[0].includes(REPOS[0])
      ? [{ number: 1, body: MARKER }, { number: 2, body: "another operator" }] : [];
    assert.equal(args[0], `repos/${REPOS[0]}/pulls/1`);
    return { number: 1, state: "open", body: MARKER, title: "owned fix", html_url: "https://example.test/pr/1",
      head: { sha: "a".repeat(40), ref: "qa/fix" }, base: { sha: "b".repeat(40), ref: "main" }, mergeable: false };
  });
  assert.equal(result.exit, 4);
  assert.equal(result.result.pull_requests.length, 1);
  assert.equal(result.result.pull_requests[0].mergeability, "conflicted");
  assert.equal(result.result.pull_requests[0].head_sha, "a".repeat(40));
});

const prInfo = (number, mergeable = true, extra = {}) => ({ number, state: "open", body: MARKER,
  head: { sha: "a".repeat(40), ref: `qa/${number}` }, base: { sha: "b".repeat(40), ref: "main" }, mergeable, ...extra });

test("PR upkeep paginates all open attributed PRs across repositories and older runs", () => {
  const calls = [];
  const guard = createGuard(fixture, { clock: () => now });
  const checked = guard.run(["pr-status"], ([path]) => {
    calls.push(path);
    if (path.includes("/issues?")) return [];
    if (path.includes("/pulls?")) {
      if (path.includes(REPOS[1])) return [{ number: 3, body: MARKER }];
      return path.endsWith("page=1")
        ? Array.from({ length: 100 }, (_, i) => ({ number: i + 1, body: i === 0 ? `${MARKER}\nold run` : null }))
        : [{ number: 101, body: `${MARKER}\nold run` }];
    }
    const number = Number(path.split("/").at(-1));
    return prInfo(number, number === 101 ? null : true);
  });
  assert.equal(checked.exit, 4);
  assert.deepEqual(checked.result.pull_requests.map((p) => [p.repository, p.number, p.mergeability]),
    [[REPOS[0], 1, "mergeable"], [REPOS[0], 101, "pending"], [REPOS[1], 3, "mergeable"]]);
  assert.ok(calls.some((p) => p.includes("/pulls?") && p.endsWith("page=2")));
  assert.ok(!calls.some((p) => p.endsWith("/pulls/2")), "unmarked PRs are not inspected");
});

test("mergeable PRs pass while PRs closed or unmarked during inspection leave the active set", () => {
  const guard = createGuard(fixture, { clock: () => now });
  const result = guard.prStatus(([path]) => {
    if (path.includes("/issues?")) return [];
    if (path.includes("/pulls?")) return [{ number: 1, body: MARKER }, { number: 2, body: MARKER }, { number: 3, body: MARKER }];
    const number = Number(path.split("/").at(-1));
    return prInfo(number, true, number === 2 ? { state: "closed" } : number === 3 ? { body: null } : {});
  });
  assert.equal(result.exit, 0);
  assert.equal(result.result.pull_requests.length, 2, "only the active owned PR in each repo remains");
  assert.equal(result.result.checked_at, now.toISOString());
});

test("PR upkeep fails closed on malformed metadata, API failures and repeated pagination", () => {
  const guard = createGuard(fixture, { clock: () => now });
  for (const info of [null, {}, prInfo(2), prInfo(1, "true"), { ...prInfo(1), mergeable: undefined }, prInfo(1, true, { head: {} })]) {
    assert.throws(() => guard.prStatus(([path]) => {
      if (path.includes("/issues?")) return [];
      if (path.includes("/pulls?")) return [{ number: 1, body: MARKER }];
      return info;
    }), /metadata|mergeability/);
  }
  assert.throws(() => guard.prStatus(([path]) => {
    if (path.includes("/issues?")) return [];
    throw new Error("PR API unavailable");
  }), /unavailable/);
  assert.throws(() => guard.prStatus(([path]) => path.includes("/issues?") ? [] : {}), /open PR response/);
  const repeated = Array.from({ length: 100 }, (_, i) => ({ number: i + 1, body: null }));
  assert.throws(() => guard.prStatus(([path]) => path.includes("/issues?") ? [] : repeated), /pagination made no progress/);
});

test("PR upkeep checks fresh allowance before and after PR metadata reads", () => {
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  const initial = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
    remaining_percent: 70, resets_at: "2026-11-16T00:14:45Z", ordinary_usage_allowed: true, spend_control_reached: false };
  for (const scenario of ["blocked", "revoked", "stale", "reset"]) {
    let sample = { ...initial, remaining_percent: scenario === "blocked" ? 49 : 70 };
    let current = now.getTime(), prCalls = 0;
    const guard = createGuard(config, { clock: () => new Date(current), readUsage: () => sample });
    const api = ([path]) => {
      if (path.includes("/issues?")) return [];
      prCalls++;
      if (scenario === "revoked") sample = { ...sample, remaining_percent: 49 };
      if (scenario === "stale") current += 61_000;
      if (scenario === "reset") sample = { ...sample, resets_at: "2026-11-17T00:14:45Z" };
      return [];
    };
    if (["stale", "reset"].includes(scenario)) assert.throws(() => guard.prStatus(api), /stale|window changed/i);
    else {
      const result = guard.prStatus(api);
      assert.equal(result.exit, 3);
      assert.equal(result.result.can_create_pr, false);
    }
    if (scenario === "blocked") assert.equal(prCalls, 0);
  }
});

test("weekly creation needs an anchored run while standalone inspection remains available", () => {
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  const sample = { version: 1, source: "codex-app-server", sampled_at: now.toISOString(), limit_id: "codex", window_minutes: 10080,
    remaining_percent: 100, resets_at: "2026-11-16T00:14:45Z", ordinary_usage_allowed: true, spend_control_reached: false };
  const guard = createGuard(config, { clock: () => now, readUsage: () => sample });
  const s = guard.status(() => [], now);
  assert.equal(s.run_required, true);
  assert.equal(s.can_create_issue, false);
  assert.equal(s.can_create_pr, false);
  for (const args of [["issue", REPOS[0], "title", "body"], ["pr", REPOS[0], "branch", "title", "body"]]) {
    const result = guard.run(args, (args) => { assert.ok(!args.includes("POST")); return []; }, () => "reason");
    assert.equal(result.exit, 3);
    assert.equal(result.result.run_required, true);
  }
});

test("fresh guard processes bind an immutable weekly run to its starting allowance window", async () => {
  const { spawnSync } = await import("node:child_process");
  const config = { ...fixture, limits: null, weekly_allowance: { min_remaining_percent: 50, max_age_seconds: 60 } };
  const descriptor = { version: 1, id: "same-run", started_at: "2026-11-09T00:00:00Z", limits: null, authorization: "Synthetic User approval" };
  // Exactly the window start is valid; one millisecond earlier belongs to the old window.
  for (const [reset, start, allowed] of [
    ["2026-11-16T00:00:00Z", descriptor.started_at, true],
    ["2026-11-16T00:00:00Z", "2026-11-08T23:59:59.999Z", false],
    ["2026-11-23T00:00:00Z", descriptor.started_at, false],
  ]) {
    const current = new Date(Date.parse(reset) - 86400_000);
    const sample = { version: 1, source: "codex-app-server", sampled_at: current.toISOString(), limit_id: "codex", window_minutes: 10080,
      remaining_percent: 50, resets_at: reset, ordinary_usage_allowed: true, spend_control_reached: false };
    const script = `import { createGuard } from ${JSON.stringify(new URL("./qa.mjs", import.meta.url).href)};
      const guard = createGuard(${JSON.stringify(config)}, { clock: () => new Date(${JSON.stringify(current)}), readUsage: () => (${JSON.stringify(sample)}) });
      let calls = 0;
      try {
        const result = guard.status(() => { calls++; return []; }, new Date(${JSON.stringify(current)}), null, ${JSON.stringify({ ...descriptor, started_at: start })});
        console.log(JSON.stringify({ allowed: result.can_create_issue, run_required: result.run_required }));
      } catch (error) { console.log(JSON.stringify({ error: error.message, calls })); process.exitCode = 1; }`;
    const result = spawnSync(process.execPath, ["--input-type=module"], { input: script, encoding: "utf8" });
    const value = JSON.parse(result.stdout);
    assert.equal(result.status, allowed ? 0 : 1, result.stderr || result.stdout);
    if (allowed) assert.deepEqual(value, { allowed: true, run_required: false });
    else { assert.match(value.error, /authorized weekly window/i); assert.equal(value.calls, 0); }
  }
});


const pacedPolicy = { daily_pacing: { timezone: "America/Toronto", sunday_release_hour: 20 }, max_age_seconds: 60 };
const resetPolicy = { reset_pacing: true, max_age_seconds: 60 };
const pacedSample = (at, remaining = 100, reset = "2026-11-23T00:15:00Z") => ({
  version: 1, source: "codex-app-server", sampled_at: at, limit_id: "codex", window_minutes: 10080,
  remaining_percent: remaining, resets_at: reset, ordinary_usage_allowed: true, spend_control_reached: false,
});

test("daily pacing allocates the seven calendar days without rounding the floor", () => {
  for (let day = 16; day <= 22; day++) {
    const at = new Date(`2026-11-${day}T17:00:00Z`);
    assert.equal(weeklyAllowanceFloor(pacedPolicy, at, "2026-11-23T00:15:00Z"), (22 - day) * 100 / 7);
  }
  // Local midnight, not the UTC date or a fixed UTC offset, releases Tuesday's budget.
  for (const [at, d, reset] of [
    ["2026-11-17T04:59:59Z", 6, "2026-11-23T00:15:00Z"],
    ["2026-11-17T05:00:00Z", 5, "2026-11-23T00:15:00Z"],
    ["2026-06-09T03:59:59Z", 6, "2026-06-14T23:15:00Z"],
    ["2026-06-09T04:00:00Z", 5, "2026-06-14T23:15:00Z"],
  ]) assert.equal(weeklyAllowanceFloor(pacedPolicy, new Date(at), reset), d * 100 / 7);
});

test("Sunday releases Monday's allocation at 8pm only after the actual reset", () => {
  for (const [at, reset, expected] of [
    ["2026-11-15T23:00:00Z", "2026-11-16T00:15:00Z", 0],
    ["2026-11-16T00:30:00Z", "2026-11-23T00:15:00Z", 100],
    ["2026-11-16T00:59:59Z", "2026-11-23T00:15:00Z", 100],
    ["2026-11-16T01:00:00Z", "2026-11-23T00:15:00Z", 600 / 7],
    // A delayed official reset is not replaced with a fictional 100% balance at 8pm.
    ["2026-11-16T01:00:00Z", "2026-11-16T02:00:00Z", 0],
    ["2026-06-08T00:00:00Z", "2026-06-14T23:15:00Z", 600 / 7],
    // DST changes do not move the Sunday release hour.
    ["2026-03-09T00:00:00Z", "2026-03-15T23:15:00Z", 600 / 7],
    ["2026-11-02T01:00:00Z", "2026-11-09T00:15:00Z", 600 / 7],
  ]) assert.equal(weeklyAllowanceFloor(pacedPolicy, new Date(at), reset), expected, at);
});

test("daily pacing stops at its floor, carries savings forward and retains account refusals", () => {
  const at = "2026-11-16T17:00:00Z";
  const descriptor = { ...weeklyRun, started_at: at };
  for (const remaining of [0, 85, 600 / 7, 86, 100]) {
    const guard = createGuard({ ...fixture, limits: null, weekly_allowance: pacedPolicy }, {
      clock: () => new Date(at), readUsage: () => pacedSample(at, remaining),
    });
    let posts = 0;
    const result = guard.run(["issue", REPOS[0], "finding", "body"], (args) => {
      if (args.includes("POST")) { posts++; return { number: 1 }; }
      return [];
    }, () => "Evidence", null, descriptor);
    assert.equal(result.exit, remaining > 600 / 7 ? 0 : 3);
    assert.equal(posts, remaining > 600 / 7 ? 1 : 0);
    if (result.exit === 3) assert.equal(result.result.weekly_allowance.spendable_percent, 0);
  }
  const tuesday = "2026-11-17T17:00:00Z";
  for (const change of [{}, { ordinary_usage_allowed: false }, { spend_control_reached: true }]) {
    const guard = createGuard({ ...fixture, limits: null, weekly_allowance: pacedPolicy }, {
      clock: () => new Date(tuesday), readUsage: () => ({ ...pacedSample(tuesday), ...change }),
    });
    const state = guard.status(() => [], undefined, null, descriptor);
    assert.equal(state.can_create_issue, Object.keys(change).length === 0);
    assert.equal(state.weekly_allowance.spendable_percent, 100 - 500 / 7);
  }
});

test("daily pacing rejects conflicting policy and unexpected weekly reset days", () => {
  for (const policy of [
    { ...pacedPolicy, min_remaining_percent: 50 },
    { ...pacedPolicy, daily_pacing: null },
    { ...pacedPolicy, daily_pacing: { timezone: "bad/zone", sunday_release_hour: 20 } },
    { ...pacedPolicy, daily_pacing: { timezone: "America/Toronto", sunday_release_hour: 24 } },
  ]) assert.throws(() => createGuard({ ...fixture, weekly_allowance: policy }));
  const at = "2026-11-16T17:00:00Z";
  const guard = createGuard({ ...fixture, weekly_allowance: pacedPolicy }, {
    clock: () => new Date(at), readUsage: () => pacedSample(at, 100, "2026-11-20T00:00:00Z"),
  });
  assert.throws(() => guard.status(() => assert.fail("Invalid reset must fail before GitHub")), /Sunday reset/);
});

test("daily pacing rechecks the live balance immediately before a write", () => {
  const at = "2026-11-16T17:00:00Z";
  let reads = 0, posts = 0;
  const guard = createGuard({ ...fixture, limits: null, weekly_allowance: pacedPolicy }, {
    clock: () => new Date(at), readUsage: () => pacedSample(at, ++reads === 3 ? 85 : 100),
  });
  const result = guard.run(["pr", REPOS[0], "qa/fix", "title", "body"], (args) => {
    if (args.includes("POST")) posts++;
    return [];
  }, () => "Evidence", null, { ...weeklyRun, started_at: at });
  assert.equal(result.exit, 3);
  assert.equal(reads, 3);
  assert.equal(posts, 0);
});

test("daily pacing recalculates the reserve after pagination crosses a release boundary", () => {
  for (const [before, after, remaining, floor] of [
    ["2026-11-17T04:59:59Z", "2026-11-17T05:00:00Z", 80, 500 / 7],
    ["2026-11-16T00:59:59Z", "2026-11-16T01:00:00Z", 100, 600 / 7],
  ]) {
    let current = before;
    const guard = createGuard({ ...fixture, limits: null, weekly_allowance: pacedPolicy }, {
      clock: () => new Date(current), readUsage: () => pacedSample(current, remaining),
    });
    const state = guard.status(() => { current = after; return []; }, undefined, null,
      { ...weeklyRun, started_at: before });
    assert.equal(state.weekly_allowance.min_remaining_percent, floor);
    assert.equal(state.can_create_issue, true);
  }
});

test("reset pacing follows the exact official window, regardless of weekday or DST", () => {
  for (const reset of ["2026-11-23T00:17:22Z", "2026-11-20T12:34:56Z",
    "2026-03-10T00:00:00Z", "2026-11-03T00:00:00Z"]) {
    for (const [left, expected] of [[604800000, 100], [302400000, 50], [151200000, 25], [1, 100 / 604800000]]) {
      const at = new Date(Date.parse(reset) - left);
      assert.equal(weeklyAllowanceFloor(resetPolicy, at, reset), expected);
    }
  }
  const at = new Date("2026-11-16T15:08:47.900Z");
  const floor = weeklyAllowanceFloor(resetPolicy, at, "2026-11-23T00:17:22Z");
  assert.ok(floor > 91 && floor < 92); // Unlike the superseded Monday 85.7% floor.
  for (const reset of ["invalid", at.toISOString(), "2026-11-16T15:08:47Z", "2026-11-24T00:00:00Z"]) {
    assert.throws(() => weeklyAllowanceFloor(resetPolicy, at, reset), /active official weekly window/);
  }
});

test("reset pacing rejects ambiguous or malformed configuration", () => {
  for (const change of [
    { reset_pacing: false }, { reset_pacing: null }, { reset_pacing: {} },
    { min_remaining_percent: 50 }, { daily_pacing: pacedPolicy.daily_pacing },
    { max_age_seconds: 0 }, { max_age_seconds: 61 },
  ]) assert.throws(() => validateConfig({ ...fixture, weekly_allowance: { ...resetPolicy, ...change } }));
});

test("reset pacing stops exactly at the reserve and retains account refusals", () => {
  const at = "2026-11-19T12:00:00Z", reset = "2026-11-23T00:00:00Z";
  const descriptor = { ...weeklyRun, started_at: at };
  for (const [remaining, change, allowed] of [
    [0, {}, false], [50, {}, false], [50.0001, {}, true], [75, {}, true],
    [100, { ordinary_usage_allowed: false }, false], [100, { spend_control_reached: true }, false],
  ]) {
    const guard = createGuard({ ...fixture, limits: null, weekly_allowance: resetPolicy }, {
      clock: () => new Date(at), readUsage: () => ({ ...pacedSample(at, remaining, reset), ...change }),
    });
    let posts = 0;
    const result = guard.run(["issue", REPOS[0], "finding", "body"], (args) => {
      if (args.includes("POST")) posts++;
      return [];
    }, () => "Evidence", null, descriptor);
    assert.equal(result.exit, allowed ? 0 : 3);
    assert.equal(posts, allowed ? 1 : 0);
    if (!allowed) assert.equal(result.result.weekly_allowance.spendable_percent, Math.max(0, remaining - 50));
  }
});

test("reset pacing refuses stale telemetry, expired windows and old run authority", () => {
  const at = "2026-11-19T12:00:00Z", reset = "2026-11-23T00:00:00Z";
  for (const [change, start] of [
    [{ sampled_at: "2026-11-19T11:58:59Z" }, at],
    [{ sampled_at: "2026-11-19T12:00:01Z" }, at],
    [{ resets_at: at }, at],
    [{ window_minutes: 60 }, at],
    [{}, "2026-11-15T23:59:59Z"],
  ]) {
    const guard = createGuard({ ...fixture, limits: null, weekly_allowance: resetPolicy }, {
      clock: () => new Date(at), readUsage: () => ({ ...pacedSample(at, 100, reset), ...change }),
    });
    assert.throws(() => guard.status(() => assert.fail("Must refuse before GitHub"),
      undefined, null, { ...weeklyRun, started_at: start }), /weekly|stale|reset/i);
  }
});

test("reset pacing rechecks the balance before writes and recalculates after pagination", () => {
  const at = "2026-11-19T12:00:00Z", reset = "2026-11-23T00:00:00Z";
  let reads = 0, posts = 0;
  const guard = createGuard({ ...fixture, limits: null, weekly_allowance: resetPolicy }, {
    clock: () => new Date(at), readUsage: () => pacedSample(at, ++reads === 3 ? 50 : 100, reset),
  });
  const result = guard.run(["pr", REPOS[0], "qa/fix", "title", "body"], (args) => {
    if (args.includes("POST")) posts++;
    return [];
  }, () => "Evidence", null, { ...weeklyRun, started_at: at });
  assert.equal(result.exit, 3);
  assert.equal(reads, 3);
  assert.equal(posts, 0);

  let current = at;
  const moving = createGuard({ ...fixture, limits: null, weekly_allowance: resetPolicy }, {
    clock: () => new Date(current), readUsage: () => pacedSample(current, 75, reset),
  });
  const state = moving.status(() => { current = "2026-11-19T12:00:30Z"; return []; },
    undefined, null, { ...weeklyRun, started_at: at });
  assert.equal(state.weekly_allowance.min_remaining_percent, (302400000 - 30000) / 604800000 * 100);
  assert.equal(state.can_create_issue, true);
});

test("reset pacing refuses a reset change during pagination", () => {
  const at = "2026-11-19T12:00:00Z";
  let reset = "2026-11-23T00:00:00Z";
  const guard = createGuard({ ...fixture, limits: null, weekly_allowance: resetPolicy }, {
    clock: () => new Date(at), readUsage: () => pacedSample(at, 100, reset),
  });
  assert.throws(() => guard.status(() => { reset = "2026-11-23T01:00:00Z"; return []; },
    undefined, null, { ...weeklyRun, started_at: at }), /Weekly window changed/);
});
