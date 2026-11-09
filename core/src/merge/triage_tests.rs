//! Red tests for Gate failure triage (contract §3.4 "Gate failure triage", pro-d366 part 3, pro-d368): an Attempt that
//! ends `gate_failed` is classified once (`ClassifierPort`); a `spec_or_scope_violation` at 0.80 or more fails the Job at
//! once with a `Triage:` line and `data.triage`; every other verdict, and a classifier error, leaves the same-Tier retry,
//! the verdict being recorded in the `attempt-retry` Event. Red until the triage is implemented.
//!
//! No test runs git, a model, the network, a wall clock or a sleep: the git is the in-process `Universe`, the Gate hook
//! is a scripted `sh` reply of the `FakeRunner`, the classifier a `FakeClassifier`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tempfile::TempDir;

use crate::classifier::Chosen;
use crate::config::{Catalog, Config, TierModel};
use crate::error::{CpdError, ErrorCode};
use crate::fakes::classifier::FakeClassifier;
use crate::fakes::git::{GitRoute, Head, Oid, Repo, RepoId, Universe};
use crate::home::CpdHome;
use crate::host::runner::Reply;
use crate::ids::{AttemptId, PlanId};
use crate::merge::{self, Attempt, Ctx};
use crate::ports::{Fakes, Ports};
use crate::store::Store;

fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![1, 2, 3],
    };
    Catalog {
        j: crate::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    }
}

/// A commit of exactly `files` over `parents`.
fn commit(
    u: &mut Universe,
    repo: RepoId,
    parents: &[Oid],
    files: &[(&str, &str)],
    message: &str,
) -> Oid {
    let blobs: Vec<(&str, Oid)> = files
        .iter()
        .map(|(path, text)| (*path, u.blob(repo, text.as_bytes())))
        .collect();
    let entries: Vec<(&str, &Oid)> = blobs.iter().map(|(path, blob)| (*path, blob)).collect();
    let tree = u.tree(repo, &entries);
    u.commit(repo, &tree, parents, message)
}

/// A temp `$CPD_HOME` whose Workflow `pay/api` is `open` on its Lead Branch, with one `merging` linear Attempt `a-a1`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    home: CpdHome,
    store: Store,
    plan: PlanId,
    id: AttemptId,
}

/// The store and the Lead worktree of a linear Attempt whose commit `attempt_tree` is `submitted`,
/// optionally followed by a sibling's commit `sibling_tree` on top (the Lead Branch tip is past `submitted`).
fn rig(hook: Reply) -> Rig {
    let attempt_tree: &[(&str, &str)] = &[("f.txt", "attempt\n"), ("protected.txt", "safe\n")];
    let sibling_tree: Option<&[(&str, &str)]> = None;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let fakes = Fakes::new();
    let mut universe = Universe::new(fakes.clock.clone());
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let _ = (*fakes.runner).clone().with_git(&route);

    let repo = Repo::init(&universe, &root.join("r"));
    repo.write("f.txt", "base\n");
    repo.write("protected.txt", "safe\n");
    let base = repo.commit("base");
    let lead = "cpd/pay/api";
    repo.branch(lead);
    let (submitted, _top) = {
        let mut u = universe.lock().unwrap();
        let submitted = commit(
            &mut u,
            repo.id(),
            std::slice::from_ref(&base),
            attempt_tree,
            "attempt",
        );
        let top = match sibling_tree {
            Some(tree) => commit(
                &mut u,
                repo.id(),
                std::slice::from_ref(&submitted),
                tree,
                "sibling",
            ),
            None => submitted.clone(),
        };
        u.set_ref(
            repo.id(),
            &format!("refs/heads/{lead}"),
            &top,
            "commit: top",
        );
        (submitted, top)
    };
    let worktree = root.join("r.lead");
    universe
        .lock()
        .unwrap()
        .add_worktree(repo.id(), &worktree, Head::Branch(lead.to_owned()));

    let _ = (*fakes.runner).clone().on("sh", |_| true, hook);
    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.root().join("policy/acme")).unwrap();
    std::fs::write(home.root().join("policy/acme/owner.toml"), OWNER_POLICY).unwrap();
    std::fs::write(home.root().join("policy/acme/r.toml"), REPO_POLICY).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let placeholder = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '{}', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', 't', 't');
                 INSERT INTO workflows (plan, key, name, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', 'r', '{}', 'main', 'dispatch', '{lead}', '{}', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"f.txt\", \"src/**\"]', '[\"b\"]', '[]', 'running', 0, '{placeholder}', cpd_txn(), 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, submitted_sha, submitted_at, driver_pid, started_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'merging', '{lead}', '{}', '{}', '{}', 't', 4242, 't');",
                worktree.display(),
                repo.dir().display(),
                worktree.display(),
                worktree.display(),
                base.as_str(),
                submitted.as_str(),
            ))?;
            Ok(())
        })
        .unwrap();
    Rig {
        _tmp: tmp,
        fakes,
        home,
        store,
        plan: PlanId::parse("pay").unwrap(),
        id: AttemptId::parse("a-a1").unwrap(),
    }
}

/// cpd's compiled default policy, as a complete owner scope (so `policy_of_job` resolves).
/// The repo scope: the merge Gate is local and its `pre-commit` is `make check`.
const REPO_POLICY: &str = "[gate]\nmerge = \"local\"\n\n[hooks]\npre-commit = \"make check\"\n";

const OWNER_POLICY: &str = "\
[gate]
merge = \"none\"
pr = \"none\"
text = \"\"

[pr]
text = \"\"

[pr.feedback]
checks = \"any\"

[deploy]
text = \"\"

[code]
text = \"\"

[tests]
text = \"\"

[monitor]
text = \"\"

[instructions]
text = \"\"
";

fn config(flag: bool) -> Config {
    Config::parse(
        &format!("version = 1\nmachine = \"test\"\n[experimental]\nfailure_triage = {flag}\n"),
        Path::new("config.toml"),
    )
    .unwrap()
    .0
}

/// A Gate whose `make check` exits 2 after printing `lines` numbered lines and a last line `error: boom`.
fn failing(lines: usize) -> Reply {
    let mut out: String = (1..=lines).map(|n| format!("line {n}\n")).collect();
    out.push_str("error: boom\n");
    Reply::exit(2, &out, "")
}

fn chosen(label: &str, confidence: Option<f32>) -> Chosen {
    Chosen {
        label: label.to_owned(),
        confidence,
        usage: None,
        model: "kev-4b".to_owned(),
    }
}

/// What a merge left: the Job, and its Events of one kind, as `(text, data)`.
struct Outcome {
    result: Result<merge::Merge, CpdError>,
    job: (String, Option<String>),
    failed: Vec<(String, Value)>,
    retried: Vec<(String, Value)>,
}

fn run(hook: Reply, flag: bool, classifier: &FakeClassifier) -> Outcome {
    let mut rig = rig(hook);
    let config = config(flag);
    let catalog = catalog();
    let metrics = |_a: &Attempt| None;
    let ctx = Ctx {
        home: &rig.home,
        config: &config,
        catalog: &catalog,
        source: "test",
        metrics: &metrics,
        classifier,
    };
    let result = Ports::scope(&rig.fakes.ports, || {
        merge::merge(&mut rig.store, &ctx, &rig.plan, &rig.id)
    });
    let events = |rig: &mut Rig, kind: &str| -> Vec<(String, Value)> {
        rig.store
            .read(|conn| {
                let mut stmt =
                    conn.prepare("SELECT text, data FROM events WHERE kind = ?1 ORDER BY id")?;
                let rows = stmt.query_map([kind], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        serde_json::from_str::<Value>(&r.get::<_, String>(1)?).unwrap(),
                    ))
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    };
    let failed = events(&mut rig, "job-failed");
    let retried = events(&mut rig, "attempt-retry");
    let job = rig
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status, failed_cause FROM jobs WHERE plan = 'pay' AND key = 'a'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )?)
        })
        .unwrap();
    Outcome {
        result,
        job,
        failed,
        retried,
    }
}

fn assert_retried(o: &Outcome) {
    assert!(
        o.result.is_ok(),
        "the merge itself does not fail: {:?}",
        o.result
    );
    assert_eq!(o.job.0, "queued", "the same-Tier retry runs");
    assert_eq!(o.retried.len(), 1, "one attempt-retry Event");
    assert!(o.failed.is_empty(), "no job-failed Event");
    assert_eq!(o.retried[0].1["next_tier"], "S2", "at the same Tier");
}

#[test]
fn a_scope_violation_at_080_fails_the_job_with_no_retry() {
    let c = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.80)));
    let o = run(failing(3), true, &c);
    assert!(o.result.is_ok(), "{:?}", o.result);
    assert_eq!(o.job, ("failed".to_owned(), Some("gate_failed".to_owned())));
    assert!(o.retried.is_empty(), "no attempt-retry Event");
    assert_eq!(o.failed.len(), 1);
    let (text, data) = &o.failed[0];
    assert_eq!(data["cause"], "gate_failed");
    assert_eq!(data["triage"]["label"], "spec_or_scope_violation");
    assert_eq!(
        data["triage"]["confidence"]
            .as_f64()
            .map(|v| (v * 100.0).round()),
        Some(80.0)
    );
    assert_eq!(data["triage"]["model"], "kev-4b");
    assert!(
        text.contains("Triage: spec_or_scope_violation (0.80), not retried."),
        "{text}"
    );
}

#[test]
fn a_scope_violation_below_080_retries_at_the_same_tier() {
    for confidence in [Some(0.79), None] {
        let c = FakeClassifier::answering(chosen("spec_or_scope_violation", confidence));
        let o = run(failing(3), true, &c);
        assert_retried(&o);
        let triage = &o.retried[0].1["triage"];
        assert_eq!(triage["label"], "spec_or_scope_violation", "{confidence:?}");
        assert_eq!(triage["model"], "kev-4b");
        if confidence.is_none() {
            assert!(triage["confidence"].is_null());
        }
    }
}

#[test]
fn a_code_or_test_defect_retries_and_records_the_verdict() {
    let c = FakeClassifier::answering(chosen("code_or_test_defect", Some(0.95)));
    let o = run(failing(3), true, &c);
    assert_retried(&o);
    assert_eq!(o.retried[0].1["triage"]["label"], "code_or_test_defect");
}

#[test]
fn a_transient_or_infra_verdict_retries_at_the_same_tier() {
    let c = FakeClassifier::answering(chosen("transient_or_infra", Some(0.95)));
    let o = run(failing(3), true, &c);
    assert_retried(&o);
    assert_eq!(o.retried[0].1["triage"]["label"], "transient_or_infra");
}

#[test]
fn a_classifier_error_retries_and_never_fails_the_merge() {
    for code in [
        ErrorCode::ClassifierUnreachable,
        ErrorCode::ClassifierBadReply,
    ] {
        let c = FakeClassifier::new(vec![Err(CpdError::new(code, "no answer"))]);
        let o = run(failing(3), true, &c);
        assert_retried(&o);
        assert!(
            o.retried[0].1.get("triage").is_none(),
            "no triage key on {code:?}"
        );
        assert_eq!(
            c.calls().len(),
            1,
            "the classifier was asked once ({code:?})"
        );
    }
}

#[test]
fn the_flag_off_makes_no_classifier_call_and_retries() {
    let c = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.99)));
    let o = run(failing(3), false, &c);
    assert_retried(&o);
    assert!(c.calls().is_empty(), "no classifier call with the flag off");
    assert!(o.retried[0].1.get("triage").is_none());
    // The control: the same Gate failure with the flag on does ask the classifier.
    let on = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.99)));
    run(failing(3), true, &on);
    assert_eq!(
        on.calls().len(),
        1,
        "with the flag on the classifier is asked once"
    );
}

#[test]
fn a_gate_error_is_never_classified() {
    let c = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.99)));
    let o = run(Reply::signal(9, "", ""), true, &c);
    assert!(o.result.is_ok(), "{:?}", o.result);
    assert_eq!(o.job, ("failed".to_owned(), Some("gate_error".to_owned())));
    assert_eq!(o.failed.len(), 1);
    assert!(o.failed[0].1.get("triage").is_none());
    // The assertion that makes it red today: the classifier is asked about a gate_failed first, so
    // prove the Gate error is judged at all by the Event, then that the fake was never called.
    assert!(c.calls().is_empty(), "a gate_error is never classified");
    let c = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.99)));
    let o = run(failing(3), true, &c);
    assert_eq!(
        c.calls().len(),
        1,
        "a gate_failed, by contrast, is classified once: {:?}",
        o.job
    );
}

#[test]
fn the_job_failed_event_carries_the_triage_in_data_and_text() {
    let c = FakeClassifier::answering(chosen("spec_or_scope_violation", Some(0.85)));
    let long = "x".repeat(400);
    let hook = Reply::exit(2, &format!("{long}\n{long}\n{long}\nerror: boom\n"), "");
    let o = run(hook, true, &c);
    assert_eq!(o.failed.len(), 1, "the Job failed at once");
    let (text, data) = &o.failed[0];
    assert!(
        text.chars().count() <= 1500,
        "{} chars",
        text.chars().count()
    );
    let lines: Vec<&str> = text.lines().collect();
    let triage = lines
        .iter()
        .position(|l| *l == "Triage: spec_or_scope_violation (0.85), not retried.")
        .unwrap_or_else(|| panic!("no Triage line in {text}"));
    let decide = lines
        .iter()
        .position(|l| l.starts_with("Decide: "))
        .expect("a Decide line");
    assert!(
        triage < decide,
        "the Triage line comes before the Decide line"
    );
    assert_eq!(data["triage"]["label"], "spec_or_scope_violation");
    assert_eq!(data["triage"]["model"], "kev-4b");
}

#[test]
fn the_classifier_is_asked_the_gate_facts_and_the_job_kind_tier_and_touches() {
    let c = FakeClassifier::answering(chosen("code_or_test_defect", Some(0.9)));
    let o = run(failing(100), true, &c);
    assert!(o.result.is_ok());
    let calls = c.calls();
    assert_eq!(calls.len(), 1, "the classifier is asked once");
    let state = &calls[0].state;
    assert_eq!(state["hook"], "pre-commit");
    assert_eq!(state["exit"], 2);
    assert!(
        state["command"]
            .as_str()
            .is_some_and(|s| s.contains("make check")),
        "{state}"
    );
    let tail = state["tail"].as_str().expect("the tail is a string");
    assert!(tail.lines().count() <= 60, "at most 60 lines");
    assert!(
        tail.ends_with("error: boom"),
        "the last lines, got {tail:?}"
    );
    assert_eq!(state["kind"], "work");
    assert_eq!(state["tier"], "S2");
    assert_eq!(state["touches"], serde_json::json!(["f.txt", "src/**"]));
    let names: Vec<&str> = calls[0].labels.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "code_or_test_defect",
            "spec_or_scope_violation",
            "transient_or_infra"
        ]
    );
    assert!(
        calls[0].labels.iter().all(|(_, d)| !d.is_empty()),
        "each label has a definition"
    );
}
