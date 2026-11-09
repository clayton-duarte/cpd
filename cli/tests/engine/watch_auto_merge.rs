//! Engine step E18, the pending auto-merge requests (commands.md §9.8b, §21.9; contract §2.2 "PR post"; Plan auto-merge,
//! decisions pro-d115 and pro-d117): every tick the engine picks a `pr_posts` row with `auto_merge = 'pending'` whose PR is
//! open, whose Plan is not archived and which is not stacked, and queues an `auto_merge` mutation (swr-github.md §9); the outbox
//! drain runs the one core function `pr post` runs. `enabled` -> row `enabled`; `not_allowed` -> row `none`; `refused` -> row
//! `refused` and ONE `error` Event (`swr_mutation_failed`); `failed` -> the row stays pending and the outbox tries again after
//! its own backoff, with no Event. The incident hold is the shared drain's (Workflow swr-slack's tests), not E18's.
//!
//! In-process, on virtual time, like `watch_react.rs`: the engine runs on the `Fx`'s `ManualClock` and `FakeRunner`, `gh` is
//! a route that answers the read query and the mutation and records every call. No process, no sleep: a run ends when a
//! state the test reads holds (or at [`CAP`] naps, so a step that never runs fails on its assertion). Every test that says
//! "nothing runs" has a control that proves the step is alive.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use common::fx::Fx;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::store::Store;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};

/// `watch.error_repeat_seconds` of the tests that do not look at it: far above the virtual time of any run.
const LONG: u32 = 3600;

/// The naps of the engine between two ticks: a tick is a second (`tick_seconds = 1`) and a nap a tenth of one.
const NAPS_PER_TICK: usize = 10;

/// The most naps a run lasts: 150 virtual seconds.
const CAP: usize = 1500;

/// How GitHub answers.
struct Script {
    allowed: bool,
    /// The repo's `squashMergeAllowed`, `mergeCommitAllowed` and `rebaseMergeAllowed` (default: squash only).
    methods: [bool; 3],
    /// The message of a GraphQL error answering the mutation.
    refuse: Option<String>,
    /// `gh` itself fails (exit 1, this on stderr) on every auto-merge call.
    fail: Option<String>,
    /// The `mergeStateStatus` the poll document reports for every pull request (default `CLEAN`).
    merge_state: String,
}

/// The `gh` route: E5's in-process GitHub SWR poll ([`is_github_poll`]), the viewer lookup of E8, the auto-merge read
/// query and the mutation. Every call is kept.
struct Gh {
    script: Mutex<Script>,
    seen: Mutex<Vec<String>>,
    /// The engine's home: the SWR poll document's `p<i>` aliases are answered from the stored rows, so a test that stacks
    /// one PR on another's Lead Branch gets the base GitHub would report, not the default branch.
    home: PathBuf,
    /// A GitHub-side base change, independent of the Workflow row the pass will update.
    base_override: Mutex<Option<(i64, String)>>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let text = call.args.join(" ");
        self.seen.lock().unwrap().push(text.clone());
        // E5's poll IS the GitHub SWR pass (D9): the engine reads this one document in-process, before the auto-merge
        // read and mutation below (which the poll document does not carry).
        if is_github_poll(&text) {
            return Reply::ok(&self.poll_reply(&text));
        }
        // The containment compare (main's poll per open default-branch pull request) is not the poll document (D10):
        // answer it on its own, before the auto-merge reads below.
        if text.contains("/compare/") {
            return Reply::ok(r#"{"status":"ahead"}"#);
        }
        // The direct merge (`gh pr merge`, the `merge` op of `cpd pr merge`) succeeds.
        if text.starts_with("pr merge ") {
            return Reply::ok("");
        }
        let script = self.script.lock().unwrap();
        if text.contains("viewer") {
            return Reply::ok(r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#);
        }
        if let Some(fail) = &script.fail {
            return Reply::exit(1, "", fail);
        }
        if text.contains("enablePullRequestAutoMerge") {
            return match &script.refuse {
                Some(message) => Reply::exit(
                    1,
                    &format!(
                        "{}\n",
                        json!({"data": {"enablePullRequestAutoMerge": null}, "errors": [{"message": message}]})
                    ),
                    &format!("gh: {message}\n"),
                ),
                None => Reply::ok(&format!(
                    "{}\n",
                    json!({"data": {"enablePullRequestAutoMerge": {"pullRequest": {"autoMergeRequest": {"enabledAt": "2026-10-27T18:00:00Z"}}}}})
                )),
            };
        }
        if text.contains("autoMergeAllowed") {
            return Reply::ok(&format!(
                "{}\n",
                json!({"data": {"repository": {"autoMergeAllowed": script.allowed, "squashMergeAllowed": script.methods[0], "mergeCommitAllowed": script.methods[1], "rebaseMergeAllowed": script.methods[2], "pullRequest": {"id": "PR_7", "autoMergeRequest": null}}}})
            ));
        }
        Reply::exit(9, "", &format!("unexpected gh call: {text}\n"))
    }
}

impl Gh {
    /// The answer to E5's one GitHub read: the viewer, the rate limit, the two empty searches and one node per `p<i>`
    /// alias the plan asked for, read from the stored `pull_requests`/`workflows` rows (so a stacked PR keeps its base).
    fn poll_reply(&self, text: &str) -> String {
        let conn =
            Connection::open_with_flags(self.home.join("cpd.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let mut data = serde_json::Map::new();
        data.insert("viewer".into(), json!({"login": "clayton-duarte"}));
        data.insert(
            "rateLimit".into(),
            json!({"remaining": 4987, "resetAt": "2099-01-01T00:00:00Z", "cost": 1}),
        );
        for alias in ["authored", "reviews"] {
            data.insert(
                alias.into(),
                json!({"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []}),
            );
        }
        for (i, number) in poll_aliases(text).into_iter().enumerate() {
            data.insert(
                format!("p{i}"),
                json!({"pullRequest": self.node(&conn, number)}),
            );
        }
        for i in 0..64 {
            if !text.contains(&format!("m{i}: repository(owner: ")) {
                break;
            }
            data.insert(format!("m{i}"), repo_meta());
        }
        json!({"data": data}).to_string()
    }

    /// The pull request `number` of `acme/pay` as the SWR document's `Light` (with `Heavy`) selects it: the stored state
    /// and its open Workflow's base.
    fn node(&self, conn: &Connection, number: i64) -> Value {
        let (url, title, state, head): (String, String, String, Option<String>) = conn
            .query_row(
                "SELECT url, title, state, head_oid FROM pull_requests WHERE repo = 'acme/pay' AND number = ?1",
                [number],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        let stored_base: String = conn
            .query_row(
                "SELECT base FROM workflows WHERE plan = 'pay' AND status = 'open'
                   AND pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = ?1)",
                [number],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
            .unwrap_or_else(|| "main".to_owned());
        let base = self
            .base_override
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(n, _)| *n == number)
            .map_or(stored_base, |(_, base)| base.clone());
        let state = match state.as_str() {
            "merged" => "MERGED",
            "closed" => "CLOSED",
            _ => "OPEN",
        };
        let head = head.unwrap_or_else(|| "h1".to_owned());
        let merged_at = if state == "MERGED" {
            json!("2026-10-27T18:00:00Z")
        } else {
            json!(null)
        };
        json!({
            "number": number, "state": state, "isDraft": false, "mergeable": "MERGEABLE",
            "mergeStateStatus": self.script.lock().unwrap().merge_state, "reviewDecision": null,
            "baseRefName": base, "headRefName": "feat", "headRefOid": head,
            "updatedAt": "U1", "title": title, "url": url,
            "createdAt": "2026-10-01T00:00:00.000Z", "mergedAt": merged_at,
            "author": {"login": "ada"},
            "repository": {"nameWithOwner": "acme/pay", "defaultBranchRef": {"name": "main"}, "isArchived": false},
            "labels": {"nodes": []},
            "autoMergeRequest": null,
            "commits": {"nodes": [{"commit": {"oid": head, "statusCheckRollup": {"state": "SUCCESS", "contexts": {"nodes": []}}}}]},
            "latestReviews": {"nodes": []},
            "reviewThreads": {"nodes": []},
            "comments": {"nodes": []},
            "viewerDidAuthor": true,
            "viewerLatestReviewRequest": null
        })
    }
}

/// The repository metadata (`m<i>`) of `acme/pay`.
fn repo_meta() -> Value {
    json!({"defaultBranchRef": {"name": "main"}, "isArchived": false, "autoMergeAllowed": true,
           "squashMergeAllowed": true, "mergeCommitAllowed": false, "rebaseMergeAllowed": false,
           "deleteBranchOnMerge": true})
}

/// Whether `text` is E5's one GitHub read, the SWR poll document the source's `build_poll_query` sends.
fn is_github_poll(text: &str) -> bool {
    text.contains("fragment Light on PullRequest") && text.contains("authored: search(type: ISSUE")
}

/// The pull request numbers of the document's `p<i>` aliases, in document order.
fn poll_aliases(text: &str) -> Vec<i64> {
    let mut numbers = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("pullRequest(number: ") {
        rest = &rest[at + "pullRequest(number: ".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(number) = digits.parse() {
            numbers.push(number);
        }
    }
    numbers
}

/// What a test reads while the engine runs: `Clone` and `'static`, so a step of the run holds one.
#[derive(Clone)]
struct Probe {
    home: PathBuf,
    gh: Arc<Gh>,
    clock: ManualClock,
}

impl Probe {
    /// The auto-merge calls of `gh` (the viewer lookup of E8 is not E18's), argv joined.
    fn calls(&self) -> Vec<String> {
        self.gh
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.contains("autoMergeAllowed") || c.contains("enablePullRequestAutoMerge"))
            .cloned()
            .collect()
    }

    fn reads(&self) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.contains("autoMergeAllowed"))
            .count()
    }

    fn mutations(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| c.contains("enablePullRequestAutoMerge"))
            .collect()
    }

    /// The `gh pr merge` calls, argv joined.
    fn merges(&self) -> Vec<String> {
        self.gh
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("pr merge "))
            .cloned()
            .collect()
    }

    fn store(&self) -> Connection {
        Connection::open_with_flags(self.home.join("cpd.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
    }

    /// The `(auto_merge, auto_merge_at)` of PR `pr`'s post.
    fn row(&self, pr: &str) -> (String, Option<String>) {
        self.store()
            .query_row("SELECT auto_merge, auto_merge_at FROM pr_posts WHERE plan = 'pay' AND workflow = ?1", [pr], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    fn state(&self, pr: &str) -> String {
        self.row(pr).0
    }

    /// The data of every `error` Event.
    fn errors(&self) -> Vec<Value> {
        let conn = self.store();
        let mut stmt = conn
            .prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.iter()
            .map(|d| serde_json::from_str::<Value>(d).unwrap())
            .collect()
    }

    /// The `error` Events of code `code`: `(plan, session, data)`.
    fn errors_of(&self, code: &str) -> Vec<(Option<String>, Option<String>, Value)> {
        let conn = self.store();
        let mut stmt = conn
            .prepare("SELECT plan, session, data FROM events WHERE kind = 'error' ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.into_iter()
            .map(|(p, s, d)| (p, s, serde_json::from_str::<Value>(&d).unwrap()))
            .filter(|(_, _, d)| d["code"] == code)
            .collect()
    }

    /// A write of the store, as another process's: a fixture the engine finds at its next tick.
    fn write(&self, sql: &str) {
        common::sql(
            &mut Store::open(
                &CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(),
                5000,
            )
            .unwrap(),
            sql,
        );
    }

    /// Moves the engine's clock forward by `seconds`.
    fn skip(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }
}

struct Merge {
    fx: Fx,
    gh: Arc<Gh>,
}

/// A fake pi install, `watch.error_repeat_seconds = repeat`, the `gh` route (auto-merge allowed, no refusal, no failure);
/// Plan `pay` (its Lead stopped, the Plan done now) in Context `ctx`. No PR, no post yet.
fn merge(repeat: u32) -> Merge {
    let fx = Fx::with_ports_empty();
    // Migrate in-process: no template builder or cpd subprocess belongs in this fixture.
    drop(fx.env.store());
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(adapter.join("lib")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    // Nothing in this fixture may find a real Node through the inherited PATH.
    fx.export("PATH", fx.env.bin().to_str().unwrap());
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\nerror_repeat_seconds = {repeat}\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.engine_routes();
    fx.install_catalog();
    let gh = Arc::new(Gh {
        script: Mutex::new(Script {
            allowed: true,
            methods: [true, false, false],
            refuse: None,
            fail: None,
            merge_state: "CLEAN".to_owned(),
        }),
        seen: Mutex::new(Vec::new()),
        home: fx.env.cpd_home(),
        base_override: Mutex::new(None),
    });
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    let now = fx.stamp_ago(0);
    // The repo's settings as the pass stored them: the executor reads them from the cache (swr-github.md §9).
    fx.env.sql(&format!(
        "INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, '{now}');"
    ));
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', '{now}', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'pay-lead', 't', 't', '{now}');"
    ));
    Merge { fx, gh }
}

impl Merge {
    fn probe(&self) -> Probe {
        Probe {
            home: self.fx.env.cpd_home(),
            gh: self.gh.clone(),
            clock: (*self.fx.fakes.clock).clone(),
        }
    }

    fn script(&self, change: impl FnOnce(&mut Script)) {
        change(&mut self.gh.script.lock().unwrap());
    }

    /// PR `key` of Plan `pay`, pull request `number`, based on `base`, with the pull request in state `pr_state` and the
    /// PR row in status `status`; its post requests auto-merge as `auto_merge` (`enabled` and `refused` carry a time).
    fn pr(
        &self,
        key: &str,
        number: u32,
        base: &str,
        status: &str,
        pr_state: &str,
        auto_merge: &str,
    ) {
        let url = format!("https://github.com/acme/pay/pull/{number}");
        let merged_at = if status == "merged" { "'t'" } else { "NULL" };
        let at = if matches!(auto_merge, "enabled" | "refused") {
            "'t'"
        } else {
            "NULL"
        };
        // A waiting PR has no worktree, no start and no pull request yet.
        let waiting = status == "waiting";
        let (worktree, started) = if waiting {
            ("NULL", "NULL")
        } else {
            ("'/nowhere/lead'", "'t'")
        };
        let (n, u, opened, pull) = if waiting {
            (
                "NULL".to_owned(),
                "NULL".to_owned(),
                "NULL",
                "NULL".to_owned(),
            )
        } else {
            (
                number.to_string(),
                format!("'{url}'"),
                "'t'",
                format!(
                    "(SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = {number})"
                ),
            )
        };
        let pull_request = if waiting {
            String::new()
        } else {
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                   VALUES ('acme/pay', {number}, '{url}', 'PR {key}', 0, '{pr_state}', 'none', 'none', 't', {merged_at}, 't');"
            )
        };
        // The Slack source queues the request when it confirms the post (swr-slack.md §6.2): the same row, here by hand.
        let queued = if auto_merge == "pending" {
            let at = self.fx.stamp_ago(0);
            format!(
                "INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at)
                   VALUES ('github', 'github:pr:acme/pay#{number}', 'auto_merge',
                           '{{\"number\":{number},\"plan\":\"pay\",\"repo\":\"acme/pay\",\"workflow\":\"{key}\"}}', 'pending', '{at}');"
            )
        } else {
            String::new()
        };
        self.fx.env.sql(&format!(
            "{pull_request}
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
               VALUES ('pay', '{key}', '/nowhere', '{base}', '{status}', 'cpd/pay/{key}', {worktree}, {n}, {u}, {opened}, {pull}, cpd_txn(), 't', {started}, {merged_at});
             INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, auto_merge, auto_merge_at)
               VALUES ('pay', '{key}', 'C0POLICY1', '1793040{number:03}.000001', 'https://acme.slack.com/archives/C0POLICY1/p{number}', 't', '{auto_merge}', {at});
             {queued}"
        ));
    }

    /// An open PR whose post waits for auto-merge, based on the default branch.
    fn pending(&self, key: &str, number: u32) {
        self.pr(key, number, "main", "open", "open", "pending");
    }

    /// The engine runs until `step` says so or [`CAP`] naps. Hold the first nap until the initial poll has finished, so its
    /// GitHub snapshot cannot race a test's later store writes and revert a simulated base change.
    fn run(&self, mut step: impl FnMut(&Probe, usize) -> bool + Send + 'static) {
        let probe = self.probe();
        let (ended_tx, ended_rx) = mpsc::channel();
        let mut initial_poll = true;
        self.fx
            .watch_gated(
                move |nap| {
                    if initial_poll {
                        ended_rx.recv().expect("the initial GitHub poll ended");
                        initial_poll = false;
                    }
                    nap >= CAP || step(&probe, nap)
                },
                move || {
                    let _ = ended_tx.send(());
                },
            )
            .unwrap();
    }
}

/// `done` once `cond` has held for `ticks` whole ticks: the state the engine reached, and that it stays so.
fn then_ticks(
    ticks: usize,
    mut cond: impl FnMut(&Probe) -> bool + Send + 'static,
) -> impl FnMut(&Probe, usize) -> bool + Send + 'static {
    let mut since = None;
    move |p, nap| {
        if since.is_none() && cond(p) {
            since = Some(nap);
        }
        since.is_some_and(|at| nap >= at + ticks * NAPS_PER_TICK)
    }
}

#[test]
fn enables_a_pending_request_for_an_unstacked_open_pr() {
    let t = merge(LONG);
    t.pending("repo", 7);
    t.run(then_ticks(3, |p| p.state("repo") == "enabled"));
    let p = t.probe();

    let (state, at) = p.row("repo");
    assert_eq!(state, "enabled");
    assert!(at.is_some_and(|at| !at.is_empty()), "auto_merge_at is set");
    let calls = p.calls();
    assert_eq!(
        calls.len(),
        2,
        "the read query, then one mutation, and later ticks make no call: {calls:?}"
    );
    assert!(calls[0].contains("autoMergeAllowed"), "{calls:?}");
    assert!(
        calls[1].contains("enablePullRequestAutoMerge") && calls[1].contains("SQUASH"),
        "{calls:?}"
    );
    assert!(
        calls.iter().all(|c| !c.contains("pr merge")),
        "never `gh pr merge`: {calls:?}"
    );
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn waits_while_the_base_is_another_prs_lead_branch_then_enables_after_the_unstack() {
    let t = merge(LONG);
    t.pr("base", 6, "main", "open", "open", "enabled");
    t.pr("top", 7, "cpd/pay/base", "open", "open", "pending");
    // Five ticks stacked; then simulate the poll's base follow. The initial poll has finished before this write.
    let held = Arc::new(Mutex::new(None));
    let record = held.clone();
    let mut unstacked = false;
    let mut tail = then_ticks(3, |p| p.state("top") == "enabled");
    t.run(move |p, nap| {
        if !unstacked {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls(), p.state("top")));
                p.write("UPDATE workflows SET base = 'main' WHERE plan = 'pay' AND key = 'top';");
                unstacked = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(
        held.lock().unwrap().take(),
        Some((Vec::<String>::new(), "pending".to_owned())),
        "stacked: no gh call, the row stays pending"
    );
    assert_eq!(
        p.state("top"),
        "enabled",
        "after the unstack the engine enables it"
    );
    assert_eq!(p.mutations().len(), 1, "{:?}", p.calls());
    assert_eq!(p.errors(), Vec::<Value>::new());
}

#[test]
fn a_poll_follows_a_remote_base_change_through_the_fake_catalog() {
    let t = merge(LONG);
    t.pr("base", 6, "main", "open", "open", "enabled");
    t.pr("top", 7, "cpd/pay/base", "open", "open", "pending");
    let home = CpdHome::resolve(Some(t.fx.env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let config = cpd_core::config::Config::load(&home).unwrap();
    let mut cx = cpd_core::swr::source::Cx {
        store: &mut store,
        ports: &t.fx.fakes.ports,
        home: &home,
        config: &config,
    };
    *t.gh.base_override.lock().unwrap() = Some((7, "main".to_owned()));
    cpd_core::swr::source::github::GithubSource::default()
        .pass_with_effects(&mut cx, t.fx.fakes.clock.utc())
        .unwrap();
    let base: String = t
        .probe()
        .store()
        .query_row(
            "SELECT base FROM workflows WHERE plan = 'pay' AND key = 'top'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(base, "main");
    assert_eq!(
        t.fx.calls("node").len(),
        1,
        "the cold catalog is read through the fake runner"
    );
}

#[test]
fn a_gh_failure_keeps_the_row_pending_and_the_outbox_tries_again_after_its_backoff() {
    let t = merge(LONG);
    t.script(|s| s.fail = Some("HTTP 502: Bad Gateway".to_owned()));
    t.pending("repo", 7);
    // The first run fails (its call is the signal that its end was seen). Then three ticks pass: below the backoff, no new
    // run. Then 60 s more pass: the same request is tried again and fails again.
    let (mut failed_at, mut again) = (None, false);
    let mut tail = then_ticks(3, |p| p.reads() >= 2);
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    t.run(move |p, nap| {
        let Some(at) = failed_at else {
            if p.reads() >= 1 {
                failed_at = Some(nap);
            }
            return false;
        };
        if !again {
            if nap >= at + 3 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some(p.reads());
                p.skip(60);
                again = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(
        seen.lock().unwrap().take(),
        Some(1),
        "three seconds after the failed run is not 60 s: no call"
    );
    assert_eq!(
        p.reads(),
        2,
        "after the backoff the same request is tried again, once: {:?}",
        p.calls()
    );
    assert_eq!(
        p.row("repo"),
        ("pending".to_owned(), None),
        "a failure leaves the row pending"
    );
    assert_eq!(
        p.errors(),
        Vec::<Value>::new(),
        "a retried failure raises no Event"
    );
}

#[test]
fn a_github_refusal_marks_the_row_refused_raises_one_error_event_and_is_not_retried() {
    let t = merge(LONG);
    // The pull request is not clean (GitHub reports BLOCKED), so the refusal stays a refusal.
    t.script(|s| {
        s.merge_state = "BLOCKED".to_owned();
        s.refuse = Some("Pull request is in clean status".to_owned());
    });
    t.pending("repo", 7);
    t.run(then_ticks(5, |p| p.state("repo") == "refused"));
    let p = t.probe();

    let (state, at) = p.row("repo");
    assert_eq!(state, "refused");
    assert!(at.is_some_and(|at| !at.is_empty()), "auto_merge_at is set");
    assert_eq!(p.mutations().len(), 1, "never retried: {:?}", p.calls());
    let refused = p.errors_of("swr_mutation_failed");
    assert_eq!(refused.len(), 1, "ONE error Event: {:?}", p.errors());
    let (_, session, data) = &refused[0];
    assert_eq!(*session, None);
    let message = data["message"].as_str().unwrap();
    assert!(
        message.contains("acme/pay#7") && message.contains("Pull request is in clean status"),
        "{message}"
    );
    assert_eq!(p.errors().len(), 1, "no other error: {:?}", p.errors());
}

#[test]
fn auto_merge_clean_status_merges_directly() {
    // The stored merge_state is `clean` (the poll read CLEAN): GitHub's refusal is not a failure, the drain falls back to
    // the merge op (swr-github.md §9), decided from the stored state and not from the message text.
    let t = merge(LONG);
    t.script(|s| s.refuse = Some("Pull request is in clean status".to_owned()));
    t.pending("repo", 7);
    t.run(then_ticks(5, |p| !p.merges().is_empty()));
    let p = t.probe();

    let merges = p.merges();
    assert_eq!(merges.len(), 1, "one direct merge: {:?}", p.calls());
    assert!(
        merges[0].contains("acme/pay") && merges[0].contains("--squash"),
        "{}",
        merges[0]
    );
    assert_eq!(
        p.errors_of("swr_mutation_failed").len(),
        0,
        "no swr_mutation_failed Event: {:?}",
        p.errors()
    );
    assert_ne!(p.state("repo"), "refused", "the row is not refused");
}

#[test]
fn a_repo_that_stopped_allowing_auto_merge_gets_none_and_no_error_event() {
    let t = merge(LONG);
    t.script(|s| s.allowed = false);
    t.pending("repo", 7);
    t.run(then_ticks(5, |p| p.state("repo") != "pending"));
    let p = t.probe();

    assert_eq!(
        p.row("repo"),
        ("none".to_owned(), None),
        "a settled row without a time"
    );
    assert!(
        p.mutations().is_empty(),
        "nothing is requested: {:?}",
        p.calls()
    );
    assert_eq!(p.reads(), 1, "never retried: {:?}", p.calls());
    assert_eq!(
        p.errors(),
        Vec::<Value>::new(),
        "no error Event for a repo that lacks auto-merge"
    );
}

#[test]
fn a_pending_request_is_enabled_with_the_method_the_repo_allows() {
    let t = merge(LONG);
    t.script(|s| s.methods = [false, false, true]);
    t.pending("repo", 7);
    t.run(then_ticks(3, |p| p.state("repo") != "pending"));
    let p = t.probe();

    let mutations = p.mutations();
    assert_eq!(mutations.len(), 1, "{:?}", p.calls());
    assert!(
        mutations[0].contains("REBASE") && !mutations[0].contains("SQUASH"),
        "{mutations:?}"
    );
    assert_eq!(p.state("repo"), "enabled");
}

#[test]
fn a_repo_that_allows_no_merge_method_is_refused_with_one_error_event() {
    let t = merge(LONG);
    t.script(|s| s.methods = [false, false, false]);
    t.pending("repo", 7);
    t.run(then_ticks(5, |p| p.state("repo") != "pending"));
    let p = t.probe();

    assert_eq!(p.state("repo"), "refused");
    assert!(
        p.mutations().is_empty(),
        "nothing is requested: {:?}",
        p.calls()
    );
    assert_eq!(p.reads(), 1, "never retried: {:?}", p.calls());
    let refused = p.errors_of("swr_mutation_failed");
    assert_eq!(refused.len(), 1, "ONE error Event: {:?}", p.errors());
    let message = refused[0].2["message"].as_str().unwrap();
    assert!(
        message.contains("the repo allows no merge method"),
        "{message}"
    );
    assert_eq!(p.errors().len(), 1, "no other error: {:?}", p.errors());
}

#[test]
fn rows_that_are_not_pending_or_whose_pr_is_not_open_make_no_gh_call() {
    let t = merge(LONG);
    // Left alone: rows that are not pending, a pending row whose PR merged, one whose PR is waiting.
    t.pr("none", 1, "main", "open", "open", "none");
    t.pr("done", 2, "main", "open", "open", "enabled");
    t.pr("no", 3, "main", "open", "open", "refused");
    t.pr("merged", 4, "main", "merged", "merged", "pending");
    t.pr("waits", 5, "main", "waiting", "open", "pending");
    // The control: a pending row of an open PR, so that the silence is shown after the step proved it is alive.
    t.pending("go", 6);
    t.run(then_ticks(3, |p| p.state("go") == "enabled"));
    let p = t.probe();

    assert_eq!(p.reads(), 1, "only the control is read: {:?}", p.calls());
    assert_eq!(p.mutations().len(), 1, "{:?}", p.calls());
    assert_eq!(p.state("none"), "none");
    assert_eq!(p.state("done"), "enabled");
    assert_eq!(p.state("no"), "refused");
    assert_eq!(p.state("merged"), "pending");
    assert_eq!(p.state("waits"), "pending");
    assert_eq!(p.errors(), Vec::<Value>::new());
}
