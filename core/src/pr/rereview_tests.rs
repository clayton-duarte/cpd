//! The bot re-review comment (pro-d588, b71): the engine queues it through the `swr_mutations` outbox, once per
//! (plan, workflow, head_sha), only for bots that reviewed and are in `pr.rereview_bots`.

use std::path::Path;

use super::comments::{MARKER, parse};
use super::pulls::ready_to_merge_sql;
use super::rereview::{Push, comment_text, pass, post};
use crate::config::Config;
use crate::home::CpdHome;
use crate::janitor_tests::Rig;
use crate::store::Store;
use crate::swr::source::github_write::target_of;

const CODEX: &str = "chatgpt-codex-connector[bot]";
const CLAUDE: &str = "mission-lane-claude-code-assistant";

fn store(dir: &tempfile::TempDir) -> Store {
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    Store::open(&home, 5000).unwrap()
}

fn now() -> time::OffsetDateTime {
    time::macros::datetime!(2026-01-01 00:00 UTC)
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn push<'a>(sha: &'a str, reviewers: &'a [String], configured: &'a [String]) -> Push<'a> {
    Push {
        plan: "p1",
        workflow: "w1",
        repo: "acme/shop",
        number: 7,
        head_sha: sha,
        reviewers,
        configured,
    }
}

/// (target_key, op_kind, body) of every github comment mutation.
fn comments(store: &mut Store) -> Vec<(String, String, String)> {
    store
        .read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT target_key, op_kind, vars_json FROM swr_mutations WHERE system = 'github' AND op_kind = 'comment' ORDER BY id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    let vars: serde_json::Value =
                        serde_json::from_str(&r.get::<_, String>(2)?).unwrap();
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        vars["body"].as_str().unwrap_or_default().to_owned(),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

#[test]
fn rereview_posts_once_per_push_for_reviewing_bot() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    let (reviewers, configured) = (names(&[CODEX]), names(&[CLAUDE, CODEX]));
    let asked = post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    assert_eq!(asked, names(&[CODEX]));
    let rows = comments(&mut s);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, target_of("acme/shop", 7));
}

#[test]
fn rereview_skips_bot_that_did_not_review() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    let (reviewers, configured) = (names(&["alice"]), names(&[CLAUDE, CODEX]));
    let asked = post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    assert!(asked.is_empty());
    assert!(comments(&mut s).is_empty());
}

#[test]
fn rereview_skips_bot_not_in_configured_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    let (reviewers, configured) = (names(&[CODEX, "other-bot[bot]"]), names(&[CLAUDE]));
    let asked = post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    assert!(asked.is_empty());
    assert!(comments(&mut s).is_empty());
}

#[test]
fn rereview_rerun_same_head_posts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    let (reviewers, configured) = (names(&[CODEX]), names(&[CODEX]));
    post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    let again = post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    assert!(again.is_empty());
    assert_eq!(comments(&mut s).len(), 1);
    // A new push is a new comment.
    let next = post(&mut s, now(), &push("bbb", &reviewers, &configured)).unwrap();
    assert_eq!(next, names(&[CODEX]));
    assert_eq!(comments(&mut s).len(), 2);
}

#[test]
fn rereview_comment_text_exact() {
    assert_eq!(
        comment_text(CODEX),
        "@chatgpt-codex-connector[bot] review specifically whether your previous findings are addressed in the latest commits. Identify which are fixed, partially fixed, or still outstanding, and explain anything remaining."
    );
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    let (reviewers, configured) = (names(&[CLAUDE]), names(&[CLAUDE]));
    post(&mut s, now(), &push("aaa", &reviewers, &configured)).unwrap();
    assert_eq!(comments(&mut s)[0].2, comment_text(CLAUDE));
    assert!(comment_text(CLAUDE).starts_with("@mission-lane-claude-code-assistant review"));
}

#[test]
fn config_rereview_bots_default_and_override() {
    let base = "version = 1\nmachine = \"m1\"\n";
    let (config, _) = Config::parse(base, Path::new("config.toml")).unwrap();
    assert_eq!(config.pr.rereview_bots, names(&[CLAUDE, CODEX]));
    let (config, _) = Config::parse(
        &format!("{base}[pr]\nrereview_bots = [\"x-bot\"]\n"),
        Path::new("config.toml"),
    )
    .unwrap();
    assert_eq!(config.pr.rereview_bots, names(&["x-bot"]));
}

const CONFIGURED: [&str; 2] = [CLAUDE, CODEX];

/// A rig whose Workflow `api` is open on pull request 801 at head `new`, with the given review history.
fn rig(reviewers: &str, rereview_head: Option<&str>) -> (Rig, Store) {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    rig.sql(&format!(
        "UPDATE pull_requests SET state = 'open', head_oid = 'new', feedback = '[]', open_threads = '[\"T1\"]', reviewers = '{reviewers}', rereview_head = {} WHERE number = 801;",
        rereview_head.map_or("NULL".to_owned(), |h| format!("'{h}'"))
    ));
    let store = Store::open(&rig.home, 5000).unwrap();
    (rig, store)
}

fn pass_asks(store: &mut Store) -> Vec<String> {
    pass(store, now(), &names(&CONFIGURED)).unwrap()
}

#[test]
fn answered_thread_still_blocks_merge_ready() {
    // The thread's last comment is a `cpd pr reply`: no feedback item, but GitHub still shows it unresolved.
    let comment = |id: &str, body: &str| serde_json::json!({"id": id, "author": {"login": "codex"}, "body": body, "url": "u", "createdAt": "2026-10-20T09:00:00Z"});
    let page = |nodes: Vec<serde_json::Value>| serde_json::json!({"totalCount": nodes.len(), "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": nodes});
    let thread = serde_json::json!({"id": "T1", "isResolved": false, "isOutdated": false, "path": "a.rs",
        "comments": page(vec![comment("C1", "P1 finding"), comment("C2", &format!("fixed\n\n{MARKER}"))])});
    let pr = serde_json::json!({"id": "PR_1", "updatedAt": "2026-10-20T10:00:00Z",
        "reviewThreads": page(vec![thread]), "comments": page(vec![]), "reviews": page(vec![])});
    let answer = serde_json::json!({"data": {"viewer": {"login": "me"}, "rateLimit": {}, "p0": {"pullRequest": pr}}}).to_string();
    let read = parse(&[("acme/pay", 801)], &answer, &[]).unwrap().remove(0);
    assert!(read.items.is_empty(), "the answered thread is no feedback");
    assert_eq!(read.open_threads, names(&["T1"]));

    let (rig, mut s) = rig("[]", None);
    rig.sql(&format!(
        "UPDATE pull_requests SET merge_state = 'clean', base_ref = 'main', default_branch = 'main', base_contained = 1,
                draft = 0, feedback = '[]', open_threads = '{}' WHERE number = 801;",
        serde_json::to_string(&read.open_threads).unwrap()
    ));
    let ready = |s: &mut Store| {
        s.read(|c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {} FROM pull_requests r WHERE r.number = 801",
                    ready_to_merge_sql("r")
                ),
                [],
                |r| r.get::<_, i64>(0),
            )? != 0)
        })
        .unwrap()
    };
    assert!(!ready(&mut s), "an answered but unresolved thread blocks");
    rig.sql("UPDATE pull_requests SET open_threads = '[]' WHERE number = 801;");
    assert!(ready(&mut s), "resolved: ready (guards a vacuous pass)");
}

#[test]
fn rereview_selects_bot_with_answered_threads() {
    // Feedback is empty (every thread answered), yet the bot is in the review history and a push moved the head.
    let (_rig, mut s) = rig(&format!("[\"{CODEX}\"]"), Some("old"));
    assert_eq!(pass_asks(&mut s), names(&[CODEX]));
    assert_eq!(comments(&mut s).len(), 1);
}

#[test]
fn rereview_claim_and_enqueue_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = store(&dir);
    s.write("test", |tx| {
        tx.execute_batch(
            "CREATE TRIGGER no_mutation BEFORE INSERT ON swr_mutations BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )?;
        Ok(())
    })
    .unwrap();
    let (reviewers, configured) = (names(&[CODEX]), names(&[CODEX]));
    post(&mut s, now(), &push("aaa", &reviewers, &configured)).expect_err("the enqueue fails");
    let claims: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pr_rereview_asks", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(claims, 0, "no bot is recorded as asked without a mutation");
}

#[test]
fn rereview_not_requested_for_head_bot_already_reviewed() {
    let history = format!("[\"{CODEX}\"]");
    // First poll after the upgrade: the bot has findings, the pass has seen no head yet: it asks nothing and records the head.
    let (rig, mut s) = rig(&history, None);
    assert!(pass_asks(&mut s).is_empty());
    assert!(comments(&mut s).is_empty());
    // Same head again: nothing.
    assert!(pass_asks(&mut s).is_empty());
    // A push moves the head after the feedback: the bot is asked once.
    rig.sql("UPDATE pull_requests SET head_oid = 'newer' WHERE number = 801;");
    assert_eq!(pass_asks(&mut s), names(&[CODEX]));
    assert!(pass_asks(&mut s).is_empty());
    assert_eq!(comments(&mut s).len(), 1);
}

#[test]
fn scoped_rereview_policy_can_disable_machine_defaults() {
    let history = format!("[\"{CODEX}\"]");
    let (rig, mut s) = rig(&history, Some("old"));
    let path = rig.home.root().join("policy/acme/pay.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path.with_file_name("owner.toml"), "").unwrap();
    std::fs::write(&path, "[pr]\nrereview_bots = []\n").unwrap();
    let policy = s
        .read(|conn| {
            super::policy_of_workflow(
                conn,
                &rig.home,
                &crate::ids::PlanId::parse("pay").unwrap(),
                &crate::ids::WorkflowKey::parse("api").unwrap(),
            )
        })
        .unwrap();
    assert_eq!(policy.pr.rereview_bots, Some(vec![]));
    assert!(
        super::rereview::pass_scoped(&mut s, &rig.home, now(), &names(&CONFIGURED))
            .unwrap()
            .is_empty()
    );
    assert!(comments(&mut s).is_empty());
    std::fs::write(&path, format!("[pr]\nrereview_bots = [\"{CODEX}\"]\n")).unwrap();
    assert_eq!(
        super::rereview::pass_scoped(&mut s, &rig.home, now(), &[]).unwrap(),
        names(&[CODEX])
    );
    assert_eq!(comments(&mut s).len(), 1);
}

#[test]
fn resolved_feedback_does_not_request_rereview_on_later_pushes() {
    let history = format!("[\"{CODEX}\"]");
    let (rig, mut store) = rig(&history, Some("old"));
    rig.sql("UPDATE pull_requests SET open_threads = '[]', feedback = '[]' WHERE number = 801");
    assert!(pass_asks(&mut store).is_empty());
    assert!(comments(&mut store).is_empty());
}

#[test]
fn unresolved_scoped_policy_skips_rereview_without_failing_the_poll() {
    let history = format!("[\"{CODEX}\"]");
    let (rig, mut store) = rig(&history, Some("old"));
    assert!(
        super::rereview::pass_scoped(&mut store, &rig.home, now(), &names(&CONFIGURED))
            .unwrap()
            .is_empty()
    );
    assert!(comments(&mut store).is_empty());
}
