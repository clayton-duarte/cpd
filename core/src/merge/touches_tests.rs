//! Red tests for the touches guard at the merge (contract §7.5 "The Job's touches", §7.4; hs-n641): a merge whose diff
//! leaves the Job's Spec `touches` ends `failed` / `tampered`, its detail naming the paths, while a path a base merge
//! brought merges clean.
//!
//! No test runs git, reads the wall clock or sleeps: the git is the in-process [`Universe`] behind a [`GitRoute`], the
//! store is a temp `$CPD_HOME` fixture, and every call is scoped on a `Ports`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tempfile::TempDir;

use crate::config::{Catalog, Config, TierModel};
use crate::fakes::git::{GitRoute, Head, Oid, Repo, RepoId, Universe};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::merge::{self, Attempt, Ctx};
use crate::ports::{Fakes, Ports};
use crate::store::Store;

/// The files every fixture's base commit holds: the Job's one `touches` entry and a source file.
const BASE: &[(&str, &str)] = &[
    ("docs/research/x.md", "# Findings\n"),
    ("src/app.rs", "// app\n"),
];

/// The `touches` of the Job: one research note, no source file.
const TOUCHES: &str = "[\"docs/research/x.md\"]";

fn config() -> Config {
    Config::parse(
        "version = 1\nmachine = \"test\"\n",
        Path::new("config.toml"),
    )
    .unwrap()
    .0
}

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

/// cpd's compiled default policy as a complete owner scope, so `policy_of_job` resolves.
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

/// A temp `$CPD_HOME` whose PR `pay/api` is `dispatch` on its Lead Branch `cpd/pay/api`, with one `merging` linear Attempt
/// `a-a1` of a `work` Job, whose submitted commit is `BASE` plus `changed`. `base_ahead` is the base branch's own commit
/// over the branch point, which the Attempt merges when `brings_base`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    home: CpdHome,
    store: Store,
    plan: PlanId,
    id: AttemptId,
}

fn rig(changed: &[(&str, &str)], base_ahead: &[(&str, &str)], brings_base: bool) -> Rig {
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
    for (path, text) in BASE {
        repo.write(path, text);
    }
    let base = repo.commit("base");
    let lead = "cpd/pay/api";
    repo.branch(lead);
    // The base branch's own commit, past the branch point.
    if !base_ahead.is_empty() {
        let mut ahead = BASE.to_vec();
        ahead.extend(base_ahead.iter().copied());
        let mut u = universe.lock().unwrap();
        let tip = commit(
            &mut u,
            repo.id(),
            std::slice::from_ref(&base),
            &ahead,
            "the base moved",
        );
        u.set_ref(repo.id(), "refs/heads/main", &tip, "the base moved");
    }
    // The Attempt is linear: its branch is the Lead Branch, and its commit is the Lead tip.
    let mut tree = BASE.to_vec();
    if brings_base {
        tree.extend(base_ahead.iter().copied());
    }
    tree.extend(changed.iter().copied());
    let submitted = {
        let mut u = universe.lock().unwrap();
        let parents = if brings_base {
            vec![
                base.clone(),
                u.ref_oid(repo.id(), "refs/heads/main")
                    .expect("the base moved"),
            ]
        } else {
            vec![base.clone()]
        };
        let submitted = commit(&mut u, repo.id(), &parents, &tree, "attempt");
        u.set_ref(
            repo.id(),
            &format!("refs/heads/{lead}"),
            &submitted,
            "commit: top",
        );
        submitted
    };
    let worktree = root.join("r.lead");
    universe
        .lock()
        .unwrap()
        .add_worktree(repo.id(), &worktree, Head::Branch(lead.to_owned()));

    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"test\"\n").unwrap();
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
                 INSERT INTO workflows (plan, key, name, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', 'cpd', '{}', 'main', 'dispatch', '{lead}', '{}', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status,
                                   appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '{TOUCHES}', '[\"b\"]', '[]', 'work', 'running', 0, '{placeholder}', cpd_txn(), 't');
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

impl Rig {
    /// `merge` of the Attempt on the fakes' ports.
    fn merge(&mut self) -> merge::Merge {
        let config = config();
        let catalog = catalog();
        let metrics = |_a: &Attempt| None;
        let ctx = Ctx {
            home: &self.home,
            config: &config,
            catalog: &catalog,
            source: "test",
            metrics: &metrics,
            classifier: crate::merge::no_verdict_classifier(),
        };
        Ports::scope(&self.fakes.ports, || {
            merge::merge(&mut self.store, &ctx, &self.plan, &self.id)
        })
        .expect("the merge itself does not fail")
    }

    /// The `job-merged` Events' data, in order.
    fn merged_events(&mut self) -> Vec<Value> {
        self.store
            .read(|conn| {
                let mut stmt =
                    conn.prepare("SELECT data FROM events WHERE kind = 'job-merged' ORDER BY id")?;
                let rows = stmt.query_map([], |r| {
                    Ok(serde_json::from_str::<Value>(&r.get::<_, String>(0)?).unwrap())
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }
}

#[test]
fn a_path_outside_touches_ends_the_merge_tampered() {
    // The Attempt's own commit changes a source file: the submit would have refused it, and so does the merge.
    let mut stray = rig(&[("src/stray.rs", "// stray\n")], &[], false);
    let out = stray.merge();
    match &out.end {
        merge::End::Failed { cause, detail } => {
            assert_eq!(
                cause,
                &merge::Failure::Tampered,
                "a path outside the touches is tampering: {out:?}"
            );
            assert!(
                detail.contains("src/stray.rs"),
                "the detail names the path: {detail}"
            );
        }
        other => panic!("expected failed/tampered, got {other:?}"),
    }
    assert!(
        stray.merged_events().is_empty(),
        "a tampered merge records no job-merged Event"
    );
    let status: String = stray
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM attempts WHERE plan = 'pay' AND id = 'a-a1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "ended", "the failed merge ends the Attempt");
    let outcome: Option<String> = stray
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT outcome FROM attempts WHERE plan = 'pay' AND id = 'a-a1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(outcome.as_deref(), Some("failed"), "the Attempt failed");
}

#[test]
fn a_base_merge_change_merges() {
    // The Attempt merged the base, whose own commit changed `src/from_base.rs`: the same blob as `main`'s, so clean.
    let ahead = [("src/from_base.rs", "// from the base\n")];
    let mut clean = rig(&[], &ahead, true);
    let out = clean.merge();
    assert!(
        matches!(&out.end, merge::End::Merged { .. }),
        "a path a base merge brought merges clean: {:?}",
        out.end
    );
    assert_eq!(
        clean.merged_events().len(),
        1,
        "the merge records one job-merged Event"
    );

    // The control: a commit of the Attempt's own that writes the same path with other content is tampering.
    let mut stray = rig(
        &[("src/from_base.rs", "// the Attempt's own edit\n")],
        &ahead,
        false,
    );
    let out = stray.merge();
    match &out.end {
        merge::End::Failed { cause, detail } => {
            assert_eq!(cause, &merge::Failure::Tampered, "the control is tampering");
            assert!(detail.contains("src/from_base.rs"), "{detail}");
        }
        other => panic!("expected failed/tampered, got {other:?}"),
    }
}
