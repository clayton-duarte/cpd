//! The single-call GitHub poll (docs/architecture/swr-github.md §3, §4, §7): the assertions count `gh api graphql`
//! invocations on the `FakeRunner`, never time. `gh` is an in-process `World`; time is a `ManualClock` passed as `now`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::Cx;
use super::github::GithubSource;
use crate::clock::{Clock, ManualClock, timestamp};
use crate::config::Config;
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::ports::{Fakes, Ports};
use crate::pr::poll::{PollPlan, build_poll_query, head_ref_oid_on};
use crate::store::Store;
use crate::swr::{cache, endpoint};

/// One pull request as GitHub has it.
#[derive(Clone)]
struct Pr {
    repo: &'static str,
    number: i64,
    state: &'static str,
    oid: &'static str,
    updated: &'static str,
    labels: Vec<&'static str>,
    /// `(name, conclusion)` of the head's check runs; the rollup is FAILURE when any is not SUCCESS.
    checks: Vec<(&'static str, &'static str)>,
}

fn pr(number: i64) -> Pr {
    Pr {
        repo: "acme/pay",
        number,
        state: "OPEN",
        oid: "a1",
        updated: "U1",
        labels: vec![],
        checks: vec![("build", "SUCCESS")],
    }
}

impl Pr {
    fn node(&self) -> Value {
        let failed = self.checks.iter().any(|(_, c)| *c != "SUCCESS");
        let runs: Vec<Value> = self
            .checks
            .iter()
            .map(|(n, c)| {
                json!({"__typename":"CheckRun","name":n,"conclusion":c,"status":"COMPLETED","detailsUrl":null,
                "checkSuite":{"workflowRun":{"databaseId":9,"workflow":{"name":"ci"}}}})
            })
            .collect();
        json!({
            "number": self.number, "state": self.state, "isDraft": false, "mergeable": "MERGEABLE",
            "mergeStateStatus": "CLEAN", "reviewDecision": null, "baseRefName": "main", "headRefName": "feat",
            "headRefOid": self.oid, "updatedAt": self.updated, "title": format!("PR {}", self.number),
            "url": format!("https://github.com/{}/pull/{}", self.repo, self.number), "createdAt": "2026-10-01T00:00:00Z",
            "mergedAt": (self.state == "MERGED").then_some("2026-10-02T00:00:00Z"),
            "author": {"login": "ada"}, "repository": {"nameWithOwner": self.repo},
            "labels": {"nodes": self.labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>()},
            "autoMergeRequest": null,
            "commits": {"nodes": [{"commit": {"oid": self.oid,
                "statusCheckRollup": {"state": if failed {"FAILURE"} else {"SUCCESS"}, "contexts": {"nodes": runs}}}}]},
            "latestOpinionatedReviews": {"nodes": []}, "latestReviews": {"nodes": []},
            "reviewThreads": {"nodes": []}, "comments": {"nodes": []},
        })
    }
}

/// What GitHub answers: the two searches, the pull requests an alias can read, the rate limit, a failure.
struct State {
    authored: Vec<Pr>,
    reviews: Vec<Pr>,
    world: BTreeMap<(String, i64), Pr>,
    remaining: i64,
    reset: OffsetDateTime,
    fail: Option<Reply>,
}

struct World(Mutex<State>);

/// The text between `from` and the next `"` after the first `from` in `q` at or after `at`.
fn quoted(q: &str, from: &str, at: usize) -> Option<(String, usize)> {
    let start = q[at..].find(from)? + at + from.len();
    let end = q[start..].find('"')? + start;
    Some((q[start..end].to_owned(), end))
}

/// `(alias index, owner/name, number)` of every `p<i>: repository(owner: "o", name: "n") { pullRequest(number: N)`.
fn pr_aliases(q: &str) -> Vec<(usize, String, i64)> {
    (0..64)
        .map_while(|i| {
            let at = q.find(&format!("p{i}: repository(owner: \""))?;
            let (owner, end) = quoted(q, "owner: \"", at)?;
            let (name, end) = quoted(q, "name: \"", end)?;
            let n = q[end..].split("pullRequest(number: ").nth(1)?;
            let digits: String = n.chars().take_while(char::is_ascii_digit).collect();
            Some((i, format!("{owner}/{name}"), digits.parse().ok()?))
        })
        .collect()
}

/// `(alias index, owner/name)` of every `m<i>: repository(...)` (metadata).
fn meta_aliases(q: &str) -> Vec<(usize, String)> {
    (0..64)
        .map_while(|i| {
            let at = q.find(&format!("m{i}: repository(owner: \""))?;
            let (owner, end) = quoted(q, "owner: \"", at)?;
            let (name, _) = quoted(q, "name: \"", end)?;
            Some((i, format!("{owner}/{name}")))
        })
        .collect()
}

impl Handler for World {
    fn handle(&self, call: &Call) -> Reply {
        let st = self.0.lock().unwrap();
        if let Some(fail) = &st.fail {
            return fail.clone();
        }
        let query = call
            .args
            .iter()
            .find_map(|a| a.strip_prefix("query="))
            .unwrap_or_default();
        let mut data = serde_json::Map::new();
        data.insert("viewer".into(), json!({"login": "ada"}));
        data.insert(
            "rateLimit".into(),
            json!({"remaining": st.remaining, "resetAt": timestamp(st.reset), "cost": 1}),
        );
        for (alias, list) in [("authored", &st.authored), ("reviews", &st.reviews)] {
            data.insert(
                alias.into(),
                json!({"pageInfo": {"hasNextPage": false, "endCursor": null},
                       "nodes": list.iter().map(Pr::node).collect::<Vec<_>>()}),
            );
        }
        for (i, repo, number) in pr_aliases(query) {
            let found = st.world.get(&(repo, number)).map(|p| {
                let mut node = p.node();
                node["viewerDidAuthor"] = json!(false);
                node["viewerLatestReviewRequest"] = Value::Null;
                node
            });
            data.insert(format!("p{i}"), json!({"pullRequest": found}));
        }
        for (i, _) in meta_aliases(query) {
            data.insert(
                format!("m{i}"),
                json!({"defaultBranchRef": {"name": "main"}, "isArchived": false, "autoMergeAllowed": true,
                       "squashMergeAllowed": true, "mergeCommitAllowed": false, "rebaseMergeAllowed": false,
                       "deleteBranchOnMerge": true}),
            );
        }
        Reply::ok(&json!({"data": data}).to_string())
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
    config: Config,
    fakes: Fakes,
    world: Arc<World>,
    source: GithubSource,
    t0: OffsetDateTime,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(home.config(), "version = 1\nmachine = \"hs\"\n").unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let config = Config::load(&home).unwrap();
        let fakes = Fakes::new();
        let t0 = ManualClock::new().utc();
        let world = Arc::new(World(Mutex::new(State {
            authored: vec![],
            reviews: vec![],
            world: BTreeMap::new(),
            remaining: 4000,
            reset: t0 + Duration::from_secs(3600),
            fail: None,
        })));
        let _ = (*fakes.runner).clone().route("gh", world.clone());
        Rig {
            _dir: dir,
            home,
            store,
            config,
            fakes,
            world,
            source: GithubSource::default(),
            t0,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.world.0.lock().unwrap()
    }

    /// One pass `secs` after `t0`.
    fn pass(&mut self, secs: u64) -> Result<(), crate::error::CpdError> {
        let now = self.t0 + Duration::from_secs(secs);
        let mut cx = Cx {
            store: &mut self.store,
            ports: &self.fakes.ports,
            home: &self.home,
            config: &self.config,
        };
        self.source.pass(&mut cx, now)
    }

    fn graphql(&self) -> Vec<String> {
        self.fakes
            .runner
            .calls()
            .iter()
            .filter(|c| {
                c.args
                    .starts_with(&["api".to_owned(), "graphql".to_owned()])
            })
            .map(|c| {
                c.args
                    .iter()
                    .find_map(|a| a.strip_prefix("query="))
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    fn sql(&mut self, sql: &str) {
        let sql = sql.to_owned();
        self.store
            .write("test seed", move |tx| {
                tx.execute(&sql, [])?;
                Ok::<(), crate::error::CpdError>(())
            })
            .unwrap();
    }

    /// A stored pull request of `acme/pay`.
    fn seed(&mut self, p: &Pr, authored: bool) {
        let merged = if p.state == "MERGED" { "'m'" } else { "NULL" };
        self.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at,
               merged_at, changed_at, authored, head_oid, gh_updated_at)
             VALUES ('{}', {}, 'u', 'PR {}', 0, '{}', 'none', 'passed', 'c', {merged}, 't', {}, '{}', '{}')",
            p.repo,
            p.number,
            p.number,
            p.state.to_lowercase(),
            i32::from(authored),
            p.oid,
            p.updated
        ));
    }

    fn seed_detail(&mut self, p: &Pr) {
        self.sql(&format!(
            "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
             VALUES ('{}', {}, '{}', '{}', 't', '{{}}', 'h')",
            p.repo, p.number, p.oid, p.updated
        ));
    }

    fn text(&mut self, sql: &str) -> Option<String> {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?))
            .unwrap()
    }

    fn int(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn pr_state(&mut self, n: i64) -> Option<String> {
        self.text(&format!(
            "SELECT state FROM pull_requests WHERE number = {n}"
        ))
    }
}

fn mentions(query: &str, number: i64) -> bool {
    query.contains(&format!("pullRequest(number: {number})"))
}

#[test]
fn gh_poll_makes_one_graphql_call_per_pass() {
    let mut rig = Rig::new();
    for n in 1..=20 {
        let p = pr(n);
        rig.seed(&p, true);
        rig.state().world.insert(("acme/pay".into(), n), p);
    }
    rig.pass(0).unwrap();
    let queries = rig.graphql();
    assert_eq!(queries.len(), 1, "one graphql call for twenty absent PRs");
    let q = &queries[0];
    for part in [
        "viewer",
        "rateLimit",
        "authored: search",
        "reviews: search",
        "p19: repository",
    ] {
        assert!(q.contains(part), "the document has `{part}`: {q}");
    }
    assert_eq!(rig.fakes.runner.calls().len(), 1, "and no other gh call");
}

#[test]
fn gh_poll_selects_heavy_fields_only_for_moved_or_unhydrated_prs() {
    // The builder: heavy for the heavy set, never for an absent-only alias.
    let q = build_poll_query(&PollPlan {
        heavy: vec![("acme/pay".into(), 1)],
        absent: vec![("acme/pay".into(), 2)],
        meta: vec![],
    });
    let (first, second) = q
        .split_once("p1: repository")
        .expect("one alias per heavy or absent PR");
    assert!(
        first.contains("...Heavy") && first.contains("pullRequest(number: 1)"),
        "{q}"
    );
    assert!(
        !second.contains("...Heavy") && second.contains("viewerDidAuthor"),
        "{q}"
    );

    // The pass: 1 unchanged, 2 head moved, 3 updatedAt moved, 4 unhydrated.
    let mut rig = Rig::new();
    let stored = [pr(1), pr(2), pr(3), pr(4)];
    for p in &stored {
        rig.seed(p, true);
    }
    for p in &stored[..3] {
        rig.seed_detail(p);
    }
    let mut moved = stored.to_vec();
    moved[1].oid = "b2";
    moved[2].updated = "U2";
    rig.state().authored = moved;
    rig.pass(0).unwrap();
    let queries = rig.graphql();
    assert_eq!(queries.len(), 1);
    let q = &queries[0];
    assert!(
        !mentions(q, 1),
        "an unchanged hydrated PR costs no heavy fields: {q}"
    );
    for n in [2, 3, 4] {
        assert!(mentions(q, n), "PR {n} is heavy-selected: {q}");
    }
}

#[test]
fn gh_poll_unchanged_pr_opens_no_write_txn() {
    let mut rig = Rig::new();
    rig.state().authored = vec![pr(1)];
    rig.pass(0).unwrap();
    let history = rig.int("SELECT count(*) FROM history WHERE tbl = 'pull_requests'");
    let changed = rig.text("SELECT changed_at FROM pull_requests WHERE number = 1");
    let hydrated = rig.text("SELECT hydrated_at FROM swr_pr_detail WHERE number = 1");
    assert!(
        history >= 1 && hydrated.is_some(),
        "the first pass wrote the header and the detail"
    );

    rig.pass(10).unwrap();
    assert_eq!(
        rig.int("SELECT count(*) FROM history WHERE tbl = 'pull_requests'"),
        history,
        "an unchanged pass writes no header"
    );
    assert_eq!(
        rig.text("SELECT changed_at FROM pull_requests WHERE number = 1"),
        changed
    );
    assert_eq!(
        rig.text("SELECT hydrated_at FROM swr_pr_detail WHERE number = 1"),
        hydrated,
        "and no detail"
    );
    assert!(
        !mentions(&rig.graphql()[1], 1),
        "the second pass selects no heavy fields for it"
    );
}

#[test]
fn gh_poll_never_rereads_a_merged_or_closed_pr() {
    let mut rig = Rig::new();
    let (mut merged, mut closed) = (pr(5), pr(6));
    merged.state = "MERGED";
    closed.state = "CLOSED";
    rig.seed(&merged, true);
    rig.seed(&closed, true);
    rig.state().world.insert(("acme/pay".into(), 5), merged);
    rig.state().world.insert(("acme/pay".into(), 6), closed);
    rig.pass(0).unwrap();
    rig.pass(600).unwrap();
    for q in rig.graphql() {
        assert!(
            !mentions(&q, 5) && !mentions(&q, 6),
            "no alias reads a merged or closed PR: {q}"
        );
    }
    assert_eq!(rig.pr_state(5).as_deref(), Some("merged"));
    assert_eq!(rig.pr_state(6).as_deref(), Some("closed"));
}

#[test]
fn gh_poll_confirms_a_vanished_open_pr_once() {
    let mut rig = Rig::new();
    let (mut merged, mut closed) = (pr(7), pr(8));
    merged.state = "MERGED";
    closed.state = "CLOSED";
    rig.seed(&pr(7), true);
    rig.seed(&pr(8), true);
    rig.state().world.insert(("acme/pay".into(), 7), merged);
    rig.state().world.insert(("acme/pay".into(), 8), closed);
    rig.pass(0).unwrap();
    let first = rig.graphql();
    assert!(
        mentions(&first[0], 7) && mentions(&first[0], 8),
        "one confirmation read each"
    );
    assert_eq!(rig.pr_state(7).as_deref(), Some("merged"));
    assert_eq!(rig.pr_state(8).as_deref(), Some("closed"));
    for n in [7, 8] {
        let row = cache::get(&mut rig.store, &format!("github:pr:acme/pay#{n}"))
            .unwrap()
            .expect("the key is committed");
        assert_eq!(row.stale_after, None, "PR {n} is never stale again");
    }
    rig.pass(600).unwrap();
    assert!(
        !mentions(&rig.graphql()[1], 7) && !mentions(&rig.graphql()[1], 8),
        "confirmed once, never again"
    );
}

#[test]
fn gh_poll_reads_a_first_seen_slack_pr_once() {
    let mut rig = Rig::new();
    let mut merged = pr(42);
    merged.state = "MERGED";
    rig.state().world.insert(("acme/pay".into(), 42), merged);
    let ts = rig.t0.unix_timestamp() - 86_400;
    rig.sql(&format!(
        "INSERT INTO swr_slack_posts (channel, ts, repo, number, author_is_me, fetched_at)
         VALUES ('C1', '{ts}.000100', 'acme/pay', 42, 0, 't')"
    ));
    rig.pass(0).unwrap();
    assert!(
        mentions(&rig.graphql()[0], 42),
        "the first-seen PR gets one read"
    );
    assert_eq!(rig.pr_state(42).as_deref(), Some("merged"));
    rig.pass(600).unwrap();
    assert!(!mentions(&rig.graphql()[1], 42), "and only one");
}

#[test]
fn gh_poll_caches_viewer_and_repo_metadata_for_24h() {
    let mut rig = Rig::new();
    rig.state().authored = vec![pr(1)];
    rig.pass(0).unwrap();
    assert!(
        rig.graphql()[0].contains("m0: repository"),
        "the first pass reads the repo metadata"
    );
    assert_eq!(rig.text("SELECT login FROM swr_viewer"), Some("ada".into()));
    assert_eq!(
        rig.text("SELECT default_branch FROM swr_repos WHERE repo = 'acme/pay'"),
        Some("main".into())
    );
    let viewer = cache::get(&mut rig.store, "github:viewer")
        .unwrap()
        .expect("viewer key");
    assert_eq!(
        viewer.stale_after,
        Some(rig.t0 + Duration::from_secs(24 * 3600))
    );

    rig.pass(23 * 3600).unwrap();
    assert!(
        !rig.graphql()[1].contains("m0: repository"),
        "cached at 23 h"
    );
    rig.pass(25 * 3600).unwrap();
    assert!(
        rig.graphql()[2].contains("m0: repository"),
        "read again after 24 h"
    );
}

#[test]
fn gh_poll_pauses_the_endpoint_below_the_rate_floor() {
    let mut rig = Rig::new();
    rig.state().remaining = 99;
    let reset = rig.state().reset;
    rig.pass(0).unwrap();
    let row = endpoint::get(&mut rig.store, "github")
        .unwrap()
        .expect("endpoint row");
    assert_eq!(row.paused_until, Some(reset));
    assert!(endpoint::is_paused(&row, rig.t0 + Duration::from_secs(1)));
    assert!(!endpoint::is_paused(&row, reset));
    rig.pass(60).unwrap();
    assert_eq!(rig.graphql().len(), 1, "a paused endpoint makes no call");

    let mut ok = Rig::new();
    ok.state().remaining = 100;
    ok.pass(0).unwrap();
    let row = endpoint::get(&mut ok.store, "github")
        .unwrap()
        .expect("endpoint row");
    assert!(
        !endpoint::is_paused(&row, ok.t0 + Duration::from_secs(1)),
        "the floor is below 100"
    );
}

#[test]
fn gh_poll_keeps_cached_data_when_the_call_fails() {
    let mut rig = Rig::new();
    rig.state().authored = vec![pr(1)];
    rig.pass(0).unwrap();
    let hash = cache::get(&mut rig.store, "github:pr:acme/pay#1")
        .unwrap()
        .unwrap()
        .data_hash;
    rig.state().fail = Some(Reply::exit(1, "", "HTTP 502: Bad Gateway"));
    let _ = rig.pass(10);
    assert_eq!(rig.graphql().len(), 2, "the failing call was made");
    assert_eq!(rig.pr_state(1).as_deref(), Some("open"), "the row is kept");
    assert_eq!(
        rig.text("SELECT title FROM pull_requests WHERE number = 1"),
        Some("PR 1".into())
    );
    assert_eq!(
        cache::get(&mut rig.store, "github:pr:acme/pay#1")
            .unwrap()
            .unwrap()
            .data_hash,
        hash,
        "the key's data is kept"
    );
    let row = endpoint::get(&mut rig.store, "github")
        .unwrap()
        .expect("endpoint row");
    assert_eq!(row.error_count, 1, "the failure is the endpoint's");
}

#[test]
fn gh_poll_head_ref_oid_goes_through_the_runner() {
    let fakes = Fakes::new();
    let _ = (*fakes.runner).clone().on(
        "gh",
        |c| c.args.iter().any(|a| a == "headRefOid"),
        Reply::ok(r#"{"headRefOid":"abc123"}"#),
    );
    let oid = head_ref_oid_on(&*fakes.runner, Path::new("."), "acme/pay", 7).unwrap();
    assert_eq!(oid, "abc123");
    assert_eq!(
        fakes.runner.calls().len(),
        1,
        "the call is on the given runner"
    );
    let _: &Ports = &fakes.ports;
}

#[test]
fn gh_poll_keeps_excused_labels_reduction_of_checks() {
    let mut rig = Rig::new();
    rig.source = GithubSource {
        excused_labels: vec!["policy-hold".into()],
    };
    let mut excused = pr(1);
    excused.labels = vec!["POLICY-HOLD"];
    excused.checks = vec![("danger", "FAILURE"), ("build", "SUCCESS")];
    let mut also_failing = pr(2);
    also_failing.labels = vec!["policy-hold"];
    also_failing.checks = vec![("danger", "FAILURE"), ("build", "FAILURE")];
    let mut unlabelled = pr(3);
    unlabelled.checks = vec![("danger", "FAILURE")];
    rig.state().authored = vec![excused, also_failing, unlabelled];
    rig.pass(0).unwrap();
    let checks = |rig: &mut Rig, n: i64| {
        rig.text(&format!(
            "SELECT checks FROM pull_requests WHERE number = {n}"
        ))
    };
    assert_eq!(
        checks(&mut rig, 1).as_deref(),
        Some("passed"),
        "only the excused `danger` failed"
    );
    assert_eq!(
        checks(&mut rig, 2).as_deref(),
        Some("failed"),
        "another failure still wins"
    );
    assert_eq!(
        checks(&mut rig, 3).as_deref(),
        Some("failed"),
        "no label, no excuse"
    );
}
