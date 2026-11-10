//! Publication reconciles a remote-advanced PR branch before pushing (contract §7.4 rule P, §7.6 `pr push`): when
//! `origin/<lead branch>` holds a commit the pushed sha does not contain, the remote tip is fetched and merged, then
//! pushed, so a non-fast-forward is not a `push_failed`. A conflicting remote tip is reported as a conflict.
//!
//! Assumption (the Spec names no code for the conflict): the `update` Job ends `failed` and its detail names the
//! conflict; nothing is pushed and the remote branch is left as it was.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::fx::Fx as Kit;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Attempt, Ctx, End, Merge};
use cpd_core::store::{Store, WriteTxn};
use rusqlite::params;
use serde_json::{Value, json};

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![0, 1, 2, 3, 4, 5],
    };
    Catalog {
        j: cpd_core::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.2.0".to_owned(),
        o: model("p/opus"),
        s: model("p/sonnet"),
        h: model("p/haiku"),
        g: TierModel {
            model: "p/gemini".to_owned(),
            efforts: vec![],
        },
    }
}

fn metrics(_: &Attempt) -> Option<Value> {
    Some(json!({"turns": 7}))
}

/// A repo `pay` with a bare `origin` holding `main` and `cpd/pay`, a Lead worktree for Plan `pay`, a database and a config.
struct Fx {
    kit: Kit,
    home: CpdHome,
    config: Config,
    catalog: Catalog,
    repo: PathBuf,
    origin: PathBuf,
}

impl Deref for Fx {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.kit
    }
}

fn fx() -> Fx {
    let kit = Kit::with_ports();
    let repo = kit.git_repo("pay").canonicalize().unwrap();
    let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
            kit.env.tmux_socket()
        ),
    )
    .unwrap();
    let config = Config::load(&home).unwrap();
    let origin = kit.env.root().join("origins").join("pay.git");
    kit.git(
        kit.env.root(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    kit.github_origin(&repo);
    kit.local_origin(&repo, &origin);
    kit.git(&repo, &["push", "-q", "origin", "main"]);
    let fx = Fx {
        kit,
        home,
        config,
        catalog: catalog(),
        repo,
        origin,
    };
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
            )
        })
        .unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("lead.log"),
    };
    let lead = fx
        .scoped(|| create_lead_worktree(&fx.repo, &plan(), "main", &setup))
        .unwrap()
        .path;
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
                       VALUES ('pay', 'repo', '{repo}', 'repo', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card', 'A description', cpd_txn(), 't', 't');",
                    repo = fx.repo.display(),
                    lead = lead.display()
                ),
            )
        })
        .unwrap();
    fx.env.repo_policy("acme", "repo", "");
    fx
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn ctx<'a>(&'a self, source: &'a str) -> Ctx<'a> {
        Ctx {
            home: &self.home,
            config: &self.config,
            catalog: &self.catalog,
            source,
            metrics: &metrics,
            classifier: cpd_core::merge::no_verdict_classifier(),
        }
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn remote_sha(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn commit(&self, dir: &Path, path: &str, content: &str) -> String {
        fs::write(dir.join(path), content).unwrap();
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", &format!("edit {path}")]);
        self.git(dir, &["rev-parse", "HEAD"])
    }

    /// Someone else pushes `path` = `content` onto `origin`'s `cpd/pay`; returns the new remote tip.
    fn move_remote(&self, path: &str, content: &str) -> String {
        let clone = self.env.root().join("clone");
        self.git(
            self.env.root(),
            &[
                "clone",
                "-q",
                self.origin.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        self.git(&clone, &["config", "user.name", "someone"]);
        self.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
        self.git(&clone, &["checkout", "-q", "cpd/pay"]);
        self.commit(&clone, path, content);
        self.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
        self.remote_sha()
    }

    /// An `update` Job `up-repo` with Attempt `up-repo-a1` in `merging`, one commit writing `path` = `content`.
    fn submitted(&self, path: &str, content: &str) -> (AttemptId, PathBuf) {
        let attempt = AttemptId::parse("up-repo-a1").unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.env.root().join("attempt.log"),
        };
        let lead = self
            .store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT lead_worktree FROM workflows WHERE plan = 'pay'",
                    [],
                    |r| r.get::<_, String>(0),
                )?)
            })
            .unwrap();
        let made = self
            .scoped(|| {
                create_attempt_worktree(&self.repo, Path::new(&lead), &plan(), &attempt, &setup)
            })
            .unwrap();
        let sha = self.commit(&made.path, path, content);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', 'up-repo', 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 1, 'update', ?1, ?2, 't')",
                    params!["f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at, driver_pid)
                     VALUES ('pay', ?1, 'up-repo', 1, 'S2', 'ladder', 'merging', ?2, ?3, ?4, ?5, 't', 't', 4242)",
                    params![
                        attempt.to_string(),
                        made.branch,
                        made.path.to_str().unwrap(),
                        made.base_sha,
                        sha
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        (attempt, made.path)
    }

    fn merge(&self, attempt: &AttemptId) -> Merge {
        self.scoped(|| {
            merge::merge(
                &mut self.store(),
                &self.ctx("attempt run"),
                &plan(),
                attempt,
            )
        })
        .unwrap()
    }
}

#[test]
fn publish_reconciles_remote_advanced_branch() {
    let fx = fx();
    let (attempt, _) = fx.submitted("resolved.txt", "resolved\n");
    let moved = fx.move_remote("remote.txt", "remote\n");

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Merged { .. }), "{result:?}");
    let tip = fx.remote_sha();
    assert_ne!(tip, moved, "nothing was pushed past the remote's commit");
    fx.git(&fx.origin, &["merge-base", "--is-ancestor", &moved, &tip]);
    assert_eq!(tip, fx.lead_sha(), "the Lead Branch holds what was pushed");
}

#[test]
fn publish_conflicting_remote_reports_conflict() {
    let fx = fx();
    let (attempt, _) = fx.submitted("resolved.txt", "mine\n");
    let moved = fx.move_remote("resolved.txt", "theirs\n");

    let result = fx.merge(&attempt);

    let End::Failed { detail, .. } = &result.end else {
        panic!("{result:?}")
    };
    assert!(detail.to_lowercase().contains("conflict"), "{detail}");
    assert_eq!(
        fx.remote_sha(),
        moved,
        "the remote branch is left as it was"
    );
}

// ------------------------------------------------------------ cpd pr push

mod push {
    use crate::common;

    use std::ops::Deref;
    use std::path::PathBuf;
    use std::time::Duration;

    use common::Run;
    use common::fx::Fx as Kit;
    use cpd_core::git::{Setup, create_lead_worktree};
    use cpd_core::home::CpdHome;
    use cpd_core::ids::PlanId;
    use cpd_core::store::Store;

    const URL: &str = "https://github.com/acme/pay/pull/812";

    /// A repo with a bare `origin`, the Lead worktree of Plan `pay` (branch `cpd/pay`) pushed to `origin`, and PR `repo` of
    /// the Plan with the given status. It derefs to the [`Kit`] (`env`, `git`, `calls`, ...).
    struct Fx {
        kit: Kit,
        home: CpdHome,
        repo: PathBuf,
        origin: PathBuf,
        lead: PathBuf,
    }

    impl Deref for Fx {
        type Target = Kit;

        fn deref(&self) -> &Kit {
            &self.kit
        }
    }

    fn fx(status: &str) -> Fx {
        let kit = Kit::with_ports();
        kit.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
        // The PR is of the repo `acme/pay` (its `name`): its policy file is required, empty unless a test sets hooks.
        let repo = kit.keyed_repo("pay").canonicalize().unwrap();
        let origin = kit.env.root().join("repos/origin.git");
        kit.git(
            kit.env.root(),
            &[
                "init",
                "--bare",
                "-q",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        kit.local_origin(&repo, &origin);
        kit.git(&repo, &["push", "-q", "origin", "main"]);
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: kit.env.root().join("lead.log"),
        };
        let lead = kit
            .scoped(|| create_lead_worktree(&repo, &PlanId::parse("pay").unwrap(), "main", &setup))
            .unwrap()
            .path;
        kit.git(&lead, &["config", "user.name", "cpd test"]);
        kit.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
        kit.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay"]);

        kit.env.write_config(&format!(
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
            kit.env.tmux_socket()
        ));
        let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
        // What `pr open` writes for a PR that has a pull request: its row, number and url.
        let (phase, state, columns, values, done) = match status {
            // `waiting` and `cancelled` have no Lead worktree and no `started_at` (the `prs` CHECK).
            "waiting" | "cancelled" => ("dispatch", "", "", String::new(), "NULL"),
            "dispatch" => ("dispatch", "", "", String::new(), "NULL"),
            "open" => (
                "deploy",
                "open",
                ", pr_number, pr_url, pr_opened_at, pull_request",
                format!(", 812, '{URL}', 't', (SELECT id FROM pull_requests)"),
                "NULL",
            ),
            "merged" => (
                "done",
                "merged",
                ", pr_number, pr_url, pr_opened_at, pull_request, merged_at",
                format!(", 812, '{URL}', 't', (SELECT id FROM pull_requests), 't'"),
                "'t'",
            ),
            other => panic!("no status {other}"),
        };
        let unstarted = matches!(status, "waiting" | "cancelled");
        let worktree = if unstarted {
            "NULL".to_owned()
        } else {
            format!("'{}'", lead.display())
        };
        let started_at = if unstarted { "NULL" } else { "'t'" };
        let merged_at = if state == "merged" { "'t'" } else { "NULL" };
        let pull_request = if state.is_empty() {
            String::new()
        } else {
            format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');"
        )
        };
        let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't', {done});
         {pull_request}
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', '{status}', 'cpd/pay', {worktree}, cpd_txn(), 't', {started_at}{values});",
        repo = repo.display(),
    );
        let fx = Fx {
            kit,
            home,
            repo,
            origin,
            lead,
        };
        fx.store()
            .write("fixture", |tx| Ok(tx.execute_batch(&sql)?))
            .unwrap();
        fx
    }

    impl Fx {
        fn store(&self) -> Store {
            Store::open(&self.home, 5000).unwrap()
        }

        fn push(&self) -> Run {
            self.cpd(&["pr", "push", "--plan", "pay"])
        }

        fn commit(&self, name: &str) -> String {
            self.commit_file(&self.lead, name, name, name);
            self.lead_sha()
        }

        fn lead_sha(&self) -> String {
            self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
        }

        fn remote_sha(&self) -> String {
            self.git(&self.origin, &["rev-parse", "refs/heads/cpd/pay"])
        }

        fn move_remote(&self) -> String {
            let clone = self.env.root().join("clone");
            self.git(
                self.env.root(),
                &[
                    "clone",
                    "-q",
                    self.origin.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            self.git(&clone, &["config", "user.name", "someone"]);
            self.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
            self.git(&clone, &["checkout", "-q", "cpd/pay"]);
            self.commit_file(&clone, "remote.txt", "remote\n", "remote");
            self.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
            self.remote_sha()
        }
    }

    #[test]
    fn pr_push_reconciles_remote_advanced_branch() {
        let fx = fx("open");
        let moved = fx.move_remote();
        fx.commit("mine.txt");

        let out = fx.push().ok();

        assert_eq!(out["pushed"], true, "{out}");
        let tip = fx.remote_sha();
        fx.git(&fx.origin, &["merge-base", "--is-ancestor", &moved, &tip]);
        assert_eq!(tip, fx.lead_sha(), "the Lead Branch holds what was pushed");
    }
}
