//! Red tests for the enforced `touches` guard at `attempt submit` (contract §7.5 "The Job's touches", §7.7, §1.2;
//! hs-n641). The hs-n641 regression: a Job whose Spec `touches` name a research note had a source file changed by its
//! Attempt, and nothing refused it. The guard refuses `touches_violation` (exit 3, `details.paths`/`touches`/`generated`)
//! and leaves the Attempt `running`, with three exemptions: a path the base merge brings, the repo's `[generated] paths`,
//! and the Job's `touches_approved`. An `open`, an `update` and an explore Job run no check at all.
//!
//! No test runs git, reads the wall clock, sleeps or spawns: the git is the in-process [`Universe`] behind a [`GitRoute`],
//! the store is a temp `$CPD_HOME` fixture, and every call is scoped on a `Ports`. The caller's cwd is the Attempt's
//! worktree while `attempt submit` runs, as the Worker's is (nextest gives each test a process of its own).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tempfile::TempDir;

use super::SubmitArgs;
use super::submit::submit_in;
use crate::args::PlanArg;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::git::{GitRoute, Head, Oid, Repo, RepoId, Universe};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId};
use crate::output::Output;
use crate::ports::{Fakes, Ports};

/// The files every fixture's base commit holds: the Job's one `touches` entry and a source file.
const BASE: &[(&str, &str)] = &[
    ("docs/research/x.md", "# Findings\n"),
    ("src/app.rs", "// app\n"),
];

/// The `touches` of the regression Job: one research note, no source file.
const TOUCHES: &str = "[\"docs/research/x.md\"]";

/// The repo policy's `[generated] paths` of most fixtures.
const LOCKFILE: &str = "pnpm-lock.yaml";

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

/// Writes `files` into the worktree `root`, exactly: a file the previous tree held and this one does not is removed.
fn materialize(root: &Path, files: &[(&str, &str)]) {
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&path).unwrap();
        } else {
            std::fs::remove_file(&path).unwrap();
        }
    }
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().expect("a file has a parent")).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

/// What a fixture models.
struct Spec<'a> {
    /// The Job's kind.
    kind: &'a str,
    /// The Job's stored Spec `touches`, as the JSON array `jobs.touches` holds.
    touches: &'a str,
    /// The repo policy's `[generated] paths`.
    generated: &'a [&'a str],
    /// The files the Attempt's commit changes, over the base's tree.
    changed: &'a [(&'a str, &'a str)],
    /// The files the base branch `main` gains after the Attempt branched, which the Attempt merges when `brings_base`.
    base_ahead: &'a [(&'a str, &'a str)],
    /// Whether the Attempt's commit is the merge of the base's tip (a base merge), or its own commit on the branch point.
    brings_base: bool,
}

impl<'a> Default for Spec<'a> {
    fn default() -> Spec<'static> {
        Spec {
            kind: "work",
            touches: TOUCHES,
            generated: &[LOCKFILE],
            changed: &[],
            base_ahead: &[],
            brings_base: false,
        }
    }
}

/// A PR `pay/api` on its Lead Branch `cpd/pay/api`, one Job `a` of the Spec's kind whose Attempt `a-a1` is `running` on
/// `cpd/pay/a` in a worktree of the fake git, and a `$CPD_HOME` whose owner and repo policy files are written.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    env: Env,
    universe: Arc<Mutex<Universe>>,
    repo: RepoId,
    /// The branch point of the Attempt: the Lead Branch tip, and the merge base of the base branch.
    base: Oid,
    /// The Attempt's worktree, a real temp directory of the fake git.
    root: PathBuf,
    plan: PlanId,
    id: AttemptId,
}

fn rig(spec: Spec<'_>) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let fakes = Fakes::new();
    let mut universe = Universe::new(fakes.clock.clone());
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let _ = (*fakes.runner).clone().with_git(&route);

    // The repo: `main` and the Lead Branch at the base commit.
    let repo = Repo::init(&universe, &dir.join("r"));
    for (path, text) in BASE {
        repo.write(path, text);
    }
    let base = repo.commit("base");
    repo.branch("cpd/pay/api");
    // The base branch may move ahead: a commit over the branch point that the Attempt does not have.
    if !spec.base_ahead.is_empty() {
        let mut ahead = BASE.to_vec();
        ahead.extend(spec.base_ahead.iter().copied());
        let mut u = universe.lock().unwrap();
        let tip = commit(&mut u, repo.id(), &[base.clone()], &ahead, "the base moved");
        u.set_ref(repo.id(), "refs/heads/main", &tip, "the base moved");
    }
    // The Attempt: its own commit, or the merge of the base's tip.
    let mut tree = BASE.to_vec();
    if spec.brings_base {
        tree.extend(spec.base_ahead.iter().copied());
    }
    tree.extend(spec.changed.iter().copied());
    let attempt = {
        let mut u = universe.lock().unwrap();
        let parents = if spec.brings_base {
            vec![
                base.clone(),
                u.ref_oid(repo.id(), "refs/heads/main")
                    .expect("the base moved"),
            ]
        } else {
            vec![base.clone()]
        };
        let attempt = commit(&mut u, repo.id(), &parents, &tree, "attempt");
        u.set_ref(repo.id(), "refs/heads/cpd/pay/a", &attempt, "attempt");
        attempt
    };
    let root = dir.join("r.a");
    universe
        .lock()
        .unwrap()
        .add_worktree(repo.id(), &root, Head::Branch("cpd/pay/a".to_owned()));
    materialize(&root, &tree);

    // The home: the config, the owner policy and the repo policy.
    let home = CpdHome::resolve(Some(dir.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"test\"\n").unwrap();
    let policy = home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), OWNER_POLICY).unwrap();
    let generated = spec
        .generated
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        policy.join("cpd.toml"),
        format!("[generated]\npaths = [{generated}]\n"),
    )
    .unwrap();
    let mut env = Env::open_home_with(&fakes.ports, home).unwrap();

    let placeholder = "f".repeat(64);
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO prs (plan, key, name, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', 'cpd', '{}', 'main', 'dispatch', 'cpd/pay/api', '{}', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status,
                                   appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '{}', '[\"b\"]', '[]', '{}', 'running', 0, '{}', cpd_txn(), 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, driver_pid, started_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay/a', '{}', '{}', 4242, 't');",
                repo.dir().display(),
                dir.join("r.lead").display(),
                spec.touches,
                spec.kind,
                placeholder,
                root.display(),
                base.as_str(),
            ))?;
            Ok(())
        })
        .unwrap();

    Rig {
        _tmp: tmp,
        fakes,
        env,
        universe,
        repo: repo.id(),
        base,
        root,
        plan: PlanId::parse("pay").unwrap(),
        id: AttemptId::parse("a-a1").unwrap(),
    }
}

impl Rig {
    /// `cpd attempt submit a-a1 --plan pay`, from the Attempt's worktree, on the fakes' ports.
    fn submit(&mut self) -> Result<Output, CpdError> {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&self.root).unwrap();
        let args = SubmitArgs {
            attempt: self.id.clone(),
            summary_file: None,
            plan: PlanArg {
                plan: Some(self.plan.clone()),
            },
        };
        let result = Ports::scope(&self.fakes.ports, || submit_in(&mut self.env, args));
        std::env::set_current_dir(previous).unwrap();
        result
    }

    /// The Attempt's tree becomes `files` over the base's: the Worker reverted the stray file and committed again.
    fn recommit(&mut self, files: &[(&str, &str)]) {
        let mut tree = BASE.to_vec();
        tree.extend(files.iter().copied());
        let mut u = self.universe.lock().unwrap();
        let attempt = commit(
            &mut u,
            self.repo,
            &[self.base.clone()],
            &tree,
            "attempt again",
        );
        u.set_ref(self.repo, "refs/heads/cpd/pay/a", &attempt, "attempt again");
        drop(u);
        materialize(&self.root, &tree);
    }

    /// One column of the Attempt's row.
    fn attempt(&mut self, column: &str) -> Option<String> {
        self.one(&format!(
            "SELECT {column} FROM attempts WHERE plan = 'pay' AND id = 'a-a1'"
        ))
    }

    /// One column of Job `a`'s row.
    fn job(&mut self, column: &str) -> Option<String> {
        self.one(&format!(
            "SELECT {column} FROM jobs WHERE plan = 'pay' AND key = 'a'"
        ))
    }

    fn one(&mut self, sql: &str) -> Option<String> {
        self.env
            .store
            .read(|conn| Ok(conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?))
            .unwrap()
    }

    /// Job `a`'s `touches_approved`, as the store now holds it: an approval written after the Attempt started.
    fn approve(&mut self, paths: &[&str]) {
        let stored = serde_json::to_string(paths).unwrap();
        self.env
            .store
            .write("fixture", |tx| {
                tx.execute(
                    "UPDATE jobs SET touches_approved = ?1 WHERE plan = 'pay' AND key = 'a'",
                    rusqlite::params![stored],
                )?;
                Ok(())
            })
            .unwrap();
    }
}

/// The refusal `code` (exit 3) of `result`.
fn refused(result: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    match result {
        Ok(out) => panic!("expected {code:?}, got a success: {out:?}"),
        Err(e) => {
            assert_eq!(e.code(), code, "{e:?}");
            assert_eq!(e.exit().code(), 3, "{code:?} is a refusal");
            e
        }
    }
}

/// The Attempt is `merging`: the submit passed its checks.
fn merged_ok(rig: &mut Rig, what: &str) {
    assert_eq!(
        rig.attempt("status").as_deref(),
        Some("merging"),
        "{what}: the submit passed"
    );
    assert_eq!(
        rig.job("status").as_deref(),
        Some("running"),
        "{what}: the Job waits for the merge"
    );
}

#[test]
fn a_stray_edit_outside_touches_is_refused() {
    // The hs-n641 regression: a Job touches one research note and its Attempt changes a source file.
    let mut rig = rig(Spec {
        changed: &[("src/stray.rs", "// stray\n")],
        ..Spec::default()
    });
    let e = refused(rig.submit(), ErrorCode::TouchesViolation);
    assert_eq!(
        e.details()["paths"],
        json!(["src/stray.rs"]),
        "details.paths names the stray path: {:?}",
        e.details()
    );
    assert_eq!(
        e.details()["touches"],
        json!(["docs/research/x.md"]),
        "details.touches is the Job's list: {:?}",
        e.details()
    );
    assert_eq!(
        e.details()["generated"],
        json!([LOCKFILE]),
        "details.generated is the repo policy's list: {:?}",
        e.details()
    );
    assert!(
        e.message().contains("src/stray.rs"),
        "the message names the path: {e:?}"
    );
    assert!(
        e.message().contains("cpd job approve-touches"),
        "the message says how the Lead allows it: {e:?}"
    );
    // Nothing moved: the Attempt is still the Worker's to fix.
    assert_eq!(
        rig.attempt("status").as_deref(),
        Some("running"),
        "the refusal leaves the Attempt running"
    );
    assert_eq!(
        rig.job("status").as_deref(),
        Some("running"),
        "the Job's status did not change"
    );
}

#[test]
fn a_generated_path_change_passes() {
    // The lockfile is generated: the repo policy's `[generated] paths` allow it without an approval.
    let mut generated = rig(Spec {
        changed: &[(LOCKFILE, "lockfileVersion: 9\n")],
        ..Spec::default()
    });
    assert!(generated.submit().is_ok(), "a generated path is exempt");
    merged_ok(&mut generated, "the generated path");

    // The control: the same commit, the same Job, and no `[generated]` entry: refused.
    let mut stray = rig(Spec {
        changed: &[(LOCKFILE, "lockfileVersion: 9\n")],
        generated: &[],
        ..Spec::default()
    });
    refused(stray.submit(), ErrorCode::TouchesViolation);
    assert_eq!(
        stray.attempt("status").as_deref(),
        Some("running"),
        "the control's Attempt stays running"
    );
}

#[test]
fn a_base_merge_change_passes() {
    // The Attempt merged the base, whose own commit changed `src/from_base.rs`: the same blob as `main`'s, so exempt.
    let ahead = [("src/from_base.rs", "// from the base\n")];
    let mut merging = rig(Spec {
        base_ahead: &ahead,
        brings_base: true,
        ..Spec::default()
    });
    assert!(
        merging.submit().is_ok(),
        "a path a base merge brought is exempt"
    );
    merged_ok(&mut merging, "the base merge");

    // The control: a commit of the Attempt's own that writes the same path with other content: refused.
    let mut stray = rig(Spec {
        changed: &[("src/from_base.rs", "// the Attempt's own edit\n")],
        base_ahead: &ahead,
        ..Spec::default()
    });
    refused(stray.submit(), ErrorCode::TouchesViolation);
}

#[test]
fn an_open_and_an_update_job_are_exempt() {
    // An `open` and an `update` Job store cpd's fixed Spec as `touches`: prose, which matches no path.
    let prose =
        "[\"The base is main: run git fetch origin main and merge it as the Context says.\"]";
    // The Attempt's own commit changes a source file: outside the prose touches, and not brought by any base merge, so
    // only the kind exemption can let it pass (the base-merge exemption is isolated in `a_base_merge_change_passes`).
    let authored = [("src/authored.rs", "// authored\n")];
    for kind in ["open", "update"] {
        let mut cpd_job = rig(Spec {
            kind,
            touches: prose,
            changed: &authored,
            ..Spec::default()
        });
        assert!(
            cpd_job.submit().is_ok(),
            "{kind}: the touches check skips this kind"
        );
        merged_ok(&mut cpd_job, kind);
    }

    // The control: a `work` Job with the same prose touches and the same diff is refused.
    let mut work = rig(Spec {
        kind: "work",
        touches: prose,
        changed: &authored,
        ..Spec::default()
    });
    refused(work.submit(), ErrorCode::TouchesViolation);
}

#[test]
fn an_approved_touch_passes() {
    let mut rig = rig(Spec {
        changed: &[("src/stray.rs", "// stray\n")],
        ..Spec::default()
    });
    // Without an approval the stray path is refused.
    refused(rig.submit(), ErrorCode::TouchesViolation);
    assert_eq!(rig.attempt("status").as_deref(), Some("running"));
    // The Lead approves the path (`job approve-touches`) while the Attempt runs: the check reads it fresh.
    rig.approve(&["src/stray.rs"]);
    assert!(rig.submit().is_ok(), "an approved path is exempt");
    merged_ok(&mut rig, "the approved path");
}

#[test]
fn the_refusal_leaves_the_attempt_running() {
    let mut rig = rig(Spec {
        changed: &[("src/stray.rs", "// stray\n")],
        ..Spec::default()
    });
    refused(rig.submit(), ErrorCode::TouchesViolation);
    assert_eq!(
        rig.attempt("status").as_deref(),
        Some("running"),
        "the Attempt is running, not ended"
    );
    assert_eq!(rig.attempt("outcome"), None, "the refusal ends nothing");
    assert_eq!(rig.attempt("cause"), None, "the refusal ends nothing");
    assert_eq!(
        rig.job("status").as_deref(),
        Some("running"),
        "the Job's status did not change"
    );

    // The loop is recoverable: the Worker reverts the stray file and submits again.
    rig.recommit(&[("docs/research/x.md", "# Findings\n\nMore.\n")]);
    assert!(rig.submit().is_ok(), "a diff within the touches passes");
    merged_ok(&mut rig, "the second submit");
}

#[test]
fn an_explore_job_runs_no_touches_check() {
    // An explore Job has no worktree to diff (§7.7): its submit writes the artefact and ends in one transaction.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let fakes = Fakes::new();
    let home = CpdHome::resolve(Some(dir.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"test\"\n").unwrap();
    let policy = home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), OWNER_POLICY).unwrap();
    let mut env = Env::open_home_with(&fakes.ports, home.clone()).unwrap();
    let placeholder = "f".repeat(64);
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status,
                                   appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', NULL, 'a', 'A', 'H2', 'g', 'c', '[\"run/pay/explore/a.md\"]', '[\"b\"]', '[]', 'scout', 'running',
                           0, '{placeholder}', cpd_txn(), 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'H2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 4242, 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    let artefact = crate::driver::explore_dir(&home, &PlanId::parse("pay").unwrap()).join("a.md");
    std::fs::create_dir_all(artefact.parent().unwrap()).unwrap();
    std::fs::write(&artefact, "# Findings\n").unwrap();
    let summary = dir.join("summary.md");
    std::fs::write(&summary, "Found it.\n").unwrap();
    let args = SubmitArgs {
        attempt: AttemptId::parse("a-a1").unwrap(),
        summary_file: Some(summary),
        plan: PlanArg {
            plan: Some(PlanId::parse("pay").unwrap()),
        },
    };
    let out = Ports::scope(&fakes.ports, || submit_in(&mut env, args));
    assert!(
        out.is_ok(),
        "an explore Attempt submits its artefact: {out:?}"
    );
    let status: String = env
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM jobs WHERE plan = 'pay' AND key = 'a'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "merged", "the explore Job merged");

    // The control: the same Job of a kind that has a worktree and a check, with the same source change, is refused.
    let mut work = rig(Spec {
        changed: &[("src/stray.rs", "// stray\n")],
        ..Spec::default()
    });
    refused(work.submit(), ErrorCode::TouchesViolation);
}
