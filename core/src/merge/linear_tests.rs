//! Red tests for a linear Attempt's merge (contract §7.4, hs-d680/hs-d682/hs-d683): its branch is the
//! Workflow's Lead Branch, so the merge diffs `base_sha...submitted_sha` for the tamper check, skips the rebase
//! and the fast-forward, keeps rule P's push and runs the Gate in the Lead worktree, and records
//! `merged_sha = submitted_sha`. R recovers a linear Attempt in `merging` whose `submitted_sha` is an
//! ancestor of the Lead Branch tip with `merged_sha = submitted_sha`. Red until `wlinear-impl` teaches the
//! merge and R the linear case.
//!
//! No test runs git, reads the wall clock or sleeps: the git is the in-process [`Universe`] behind a
//! [`GitRoute`], the store is a temp `$CPD_HOME` fixture, and every call is scoped on a `Ports`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tempfile::TempDir;

use crate::config::{Catalog, Config, TierModel};
use crate::fakes::git::{GitRoute, Head, Oid, Repo, RepoId, Universe};
use crate::home::CpdHome;
use crate::host::runner::Reply;
use crate::ids::{AttemptId, PlanId};
use crate::merge::{self, Attempt, Ctx};
use crate::ports::{Fakes, Ports};
use crate::store::Store;

fn config() -> Config {
    Config::parse("version = 1\nmachine = \"test\"\n", Path::new("config.toml")).unwrap().0
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![1, 2, 3] };
    Catalog { pi_version: "1.0.0".to_owned(), o: model("o/m"), s: model("s/m"), h: model("h/m"), g: TierModel { model: "g/m".to_owned(), efforts: vec![] } }
}

/// A commit of exactly `files` over `parents`.
fn commit(u: &mut Universe, repo: RepoId, parents: &[Oid], files: &[(&str, &str)], message: &str) -> Oid {
    let blobs: Vec<(&str, Oid)> = files.iter().map(|(path, text)| (*path, u.blob(repo, text.as_bytes()))).collect();
    let entries: Vec<(&str, &Oid)> = blobs.iter().map(|(path, blob)| (*path, blob)).collect();
    let tree = u.tree(repo, &entries);
    u.commit(repo, &tree, parents, message)
}

/// A temp `$CPD_HOME` whose Workflow `pay/api` is `open` on its Lead Branch `cpd/pay/api`, with one `merging`
/// linear Attempt `a-a1` whose `submitted_sha` is `submitted` and whose `base_sha` is `base`.
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
fn rig(attempt_tree: &[(&str, &str)], sibling_tree: Option<&[(&str, &str)]>) -> Rig {
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
        let submitted = commit(&mut u, repo.id(), &[base.clone()], attempt_tree, "attempt");
        let top = match sibling_tree {
            Some(tree) => commit(&mut u, repo.id(), &[submitted.clone()], tree, "sibling"),
            None => submitted.clone(),
        };
        u.set_ref(repo.id(), &format!("refs/heads/{lead}"), &top, "commit: top");
        (submitted, top)
    };
    let worktree = root.join("r.lead");
    universe.lock().unwrap().add_worktree(repo.id(), &worktree, Head::Branch(lead.to_owned()));

    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.root().join("policy/acme")).unwrap();
    std::fs::write(home.root().join("policy/acme/owner.toml"), OWNER_POLICY).unwrap();
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
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', '{}', 'main', 'dispatch', '{lead}', '{}', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{placeholder}', cpd_txn(), 't');
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
    Rig { _tmp: tmp, fakes, home, store, plan: PlanId::parse("pay").unwrap(), id: AttemptId::parse("a-a1").unwrap() }
}

/// cpd's compiled default policy, as a complete owner scope (so `policy_of_job` resolves).
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

fn merge_ctx<'a>(home: &'a CpdHome, config: &'a Config, catalog: &'a Catalog, metrics: &'a dyn Fn(&Attempt) -> Option<Value>) -> Ctx<'a> {
    Ctx { home, config, catalog, source: "test", metrics }
}

#[test]
fn a_linear_merge_uses_the_base_sha_for_the_tamper_check() {
    // `submitted` adds a file under `.cpd/` (always protected): the diff `base_sha...submitted_sha`
    // touches it, while `old...branch` (the Lead tip is `submitted`) is empty.
    let attempt = [("f.txt", "base\n"), ("protected.txt", "safe\n"), (".cpd/tampered", "x\n")];
    let mut rig = rig(&attempt, None);
    let config = config();
    let catalog = catalog();
    let metrics = |_a: &Attempt| None;
    let ctx = merge_ctx(&rig.home, &config, &catalog, &metrics);
    let out = Ports::scope(&rig.fakes.ports, || merge::merge(&mut rig.store, &ctx, &rig.plan, &rig.id)).unwrap();
    assert!(
        matches!(&out.end, merge::End::Failed { cause: merge::Failure::Tampered, .. }),
        "a protected path in `base_sha...submitted_sha` is tampering, got {:?}",
        out.end
    );
}

#[test]
fn a_linear_merge_records_the_submitted_commit() {
    // A sibling merged on top of the linear Attempt's commit: the Lead tip is past `submitted_sha`.
    let attempt = [("f.txt", "attempt\n"), ("protected.txt", "safe\n")];
    let sibling = [("f.txt", "attempt\n"), ("protected.txt", "safe\n"), ("other.txt", "sibling\n")];
    let mut rig = rig(&attempt, Some(&sibling));
    let submitted = rig
        .store
        .read(|conn| {
            Ok(conn.query_row("SELECT submitted_sha FROM attempts WHERE plan = 'pay' AND id = 'a-a1'", [], |r| r.get::<_, String>(0))?)
        })
        .unwrap();
    let config = config();
    let catalog = catalog();
    let metrics = |_a: &Attempt| None;
    let ctx = merge_ctx(&rig.home, &config, &catalog, &metrics);
    let out = Ports::scope(&rig.fakes.ports, || merge::merge(&mut rig.store, &ctx, &rig.plan, &rig.id)).unwrap();
    assert!(matches!(&out.end, merge::End::Merged { .. }), "the linear merge stands, got {:?}", out.end);
    let merged = rig
        .store
        .read(|conn| {
            Ok(conn.query_row("SELECT merged_sha FROM attempts WHERE plan = 'pay' AND id = 'a-a1'", [], |r| r.get::<_, Option<String>>(0))?)
        })
        .unwrap();
    assert_eq!(merged.as_deref(), Some(submitted.as_str()), "a linear merge records merged_sha = submitted_sha");
    let verbs: Vec<String> = rig
        .fakes
        .runner
        .calls()
        .iter()
        .filter(|c| c.program == "git")
        .flat_map(|c| c.args.iter().cloned())
        .collect();
    assert!(!verbs.iter().any(|a| a == "rebase"), "a linear merge never rebases: {verbs:?}");
    assert!(!verbs.iter().any(|a| a == "update-ref"), "a linear merge never fast-forwards: {verbs:?}");
}

#[test]
fn r_recovers_a_linear_merging_attempt() {
    // The same shape: `submitted_sha` is an ancestor of the Lead Branch tip, so the merge happened.
    let attempt = [("f.txt", "attempt\n"), ("protected.txt", "safe\n")];
    let sibling = [("f.txt", "attempt\n"), ("protected.txt", "safe\n"), ("other.txt", "sibling\n")];
    let mut rig = rig(&attempt, Some(&sibling));
    let submitted = rig
        .store
        .read(|conn| {
            Ok(conn.query_row("SELECT submitted_sha FROM attempts WHERE plan = 'pay' AND id = 'a-a1'", [], |r| r.get::<_, String>(0))?)
        })
        .unwrap();
    // R's A9 kills the Attempt's tmux window: the fake lists none.
    let _ = (*rig.fakes.runner).clone().on("tmux", |_| true, Reply::ok(""));
    let config = config();
    let catalog = catalog();
    let metrics = |_a: &Attempt| None;
    let ctx = merge_ctx(&rig.home, &config, &catalog, &metrics);
    let ports = Ports { clock: rig.fakes.ports.clock.clone(), runner: rig.fakes.ports.runner.clone(), spawn: rig.fakes.ports.spawn.clone() };
    let _ = Ports::scope(&ports, || merge::recover_attempt(&mut rig.store, &ctx, &rig.plan, &rig.id));
    let (outcome, merged): (Option<String>, Option<String>) = rig
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT outcome, merged_sha FROM attempts WHERE plan = 'pay' AND id = 'a-a1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(outcome.as_deref(), Some("merged"), "R ends a recovered linear Attempt merged");
    assert_eq!(merged.as_deref(), Some(submitted.as_str()), "R records merged_sha = submitted_sha for a linear Attempt");
}
