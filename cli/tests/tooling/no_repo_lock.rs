//! No per-repo lock (hs-d243, design `docs/research/no-repo-lock.md` §4): the merge (`cpd_core::merge::merge`, which the
//! driver calls at A8) and `pr rebase` against one repo with a bare `origin`, with Plans written straight into the
//! database. In process, on `Fx::with_ports`: the git route, a `Runner` that holds one call, no process and no sleep.
//!
//! - Two Plans' Attempts merge into their two PRs of the one repo at once: the first merge's compare-and-swap is held,
//!   and the other merge runs whole inside that hold. With a lock between them the second merge could not run while the
//!   first holds its compare-and-swap.
//! - A Job merges into a PR while `pr rebase`'s Gate runs (risk 3): the merge's own push is rejected, an accepted
//!   `push_failed`, and the rebase's push brings the merge to `origin` by rule P.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output as Finished;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::{Fx, exit_code};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::deadline::Failed;
use cpd_core::host::runner::{Call, Reply, Runner};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, Merge};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::Value;

/// A `Runner` over `inner` that, at the first call `at` picks, first runs `during` (once) and then lets the call go on:
/// the call is held while `during` runs, on the same thread, so the order is the test's and nothing waits on time.
/// Which call to hold at: the program and its arguments.
type HoldAt = Box<dyn Fn(&str, &[String]) -> bool + Send + Sync>;

struct Hold {
    inner: Arc<dyn Runner>,
    at: HoldAt,
    during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Hold {
    fn before(&self, program: &str, args: &[String]) {
        if !(self.at)(program, args) {
            return;
        }
        let during = self.during.lock().unwrap().take();
        if let Some(during) = during {
            during();
        }
    }
}

impl Runner for Hold {
    fn output(
        &self,
        cmd: &mut std::process::Command,
        tool: &str,
        deadline: Duration,
    ) -> Result<Finished, Failed> {
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        self.before(&cmd.get_program().to_string_lossy(), &args);
        self.inner.output(cmd, tool, deadline)
    }
    fn output_unbounded(
        &self,
        cmd: &mut std::process::Command,
        tool: &str,
    ) -> std::io::Result<Finished> {
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        self.before(&cmd.get_program().to_string_lossy(), &args);
        self.inner.output_unbounded(cmd, tool)
    }
    fn run(&self, call: &Call) -> Result<Finished, Failed> {
        self.before(&call.program, &call.args);
        self.inner.run(call)
    }
}

/// `git update-ref` of a merge's compare-and-swap (`-m "cpd: merge <plan>/<attempt>"`).
fn is_merge_cas(program: &str, args: &[String]) -> bool {
    program.ends_with("git")
        && args.iter().any(|a| a == "update-ref")
        && args.iter().any(|a| a.starts_with("cpd: merge "))
}

/// pi's catalog, for the ladder's next Tier.
fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![0, 1, 2, 3, 4, 5],
    };
    Catalog {
        pi_version: "1.0.0".to_owned(),
        o: model("p/opus"),
        s: model("p/sonnet"),
        h: model("p/haiku"),
        g: TierModel {
            model: "p/gemini".to_owned(),
            efforts: vec![],
        },
    }
}

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 4] = [
    ("O", "p/opus", "[0,1,2,3,4,5]"),
    ("S", "p/sonnet", "[1,2,3,4,5]"),
    ("H", "p/haiku", "[0,1,2,3]"),
    ("G", "p/gemini", "[]"),
];

/// The repo `pay` with a bare `origin` holding `main`, a config, the Tier catalog and Context `ctx`. No Plan yet.
struct Pay {
    fx: Fx,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
}

/// Attempt `attempt` of `plan`, `merging`, merged as the driver's A8 does, on the ports in scope.
fn merged(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> Merge {
    let config = Config::load(home).unwrap();
    let catalog = catalog();
    let ctx = Ctx {
        home,
        config: &config,
        catalog: &catalog,
        source: "attempt run",
        metrics: &|_| None,
        classifier: cpd_core::merge::no_verdict_classifier(),
    };
    merge::merge(&mut Store::open(home, 5000).unwrap(), &ctx, plan, attempt).unwrap()
}

/// An Attempt in `merging`, as `attempt submit` leaves it.
struct Submitted {
    plan: PlanId,
    attempt: AttemptId,
}

/// A Plan's one PR `repo` in [`Pay::repo`]: its Lead Branch `cpd/<plan>` and Lead worktree.
struct Lead {
    plan: PlanId,
    branch: String,
    path: PathBuf,
}

fn pay() -> Pay {
    let fx = Fx::with_ports();
    // Every test here is about the Gate, which runs only under `pr.gate = "local"` (contract §8.3).
    fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    // The PRs below are of the repo `acme/pay` (their `name`): `keyed_repo` writes its empty policy file.
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    let origin = fx.env.root().join("origins/pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.local_origin(&repo, &origin);
    fx.git(&repo, &["push", "-q", "origin", "main"]);

    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fx.env.write_models();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n",
            fx.env.tmux_socket()
        ),
    )
    .unwrap();
    let pay = Pay {
        fx,
        home,
        repo,
        origin,
    };
    let rows: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    pay.fixture(&format!("{rows} INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');"));
    pay
}

impl Pay {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store()
            .write("fixture", |tx| Ok(tx.execute_batch(sql)?))
            .unwrap();
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.fx.git(dir, args)
    }

    /// Plan `id` (locked) with its PR `repo` in [`Pay::repo`], started: Lead Branch
    /// `cpd/<id>` cut from `main` and pushed. `open` makes the PR `open` with a pull
    /// request (Plan phase `deploy`); otherwise it is in `dispatch`.
    fn plan(&self, id: &str, open: bool) -> Lead {
        let plan = PlanId::parse(id).unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.fx.env.root().join("lead.log"),
        };
        let created = Ports::scope(&self.fx.fakes.ports, || {
            create_lead_worktree(&self.repo, &plan, "main", &setup)
        })
        .unwrap();
        assert!(created.push_failed.is_none(), "{:?}", created.push_failed);
        let (phase, status, columns, values, pull_request) = if open {
            (
                "deploy",
                "open",
                ", pr_number, pr_url, pr_opened_at, pull_request",
                format!(", 812, 'https://github.com/acme/{id}/pull/812', 't', (SELECT id FROM pull_requests WHERE repo = 'acme/{id}')"),
                format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                       VALUES ('acme/{id}', 812, 'https://github.com/acme/{id}/pull/812', 'PR', 0, 'open', 'none', 'none', 't', NULL, 't');"
                ),
            )
        } else {
            ("dispatch", "dispatch", "", String::new(), String::new())
        };
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead{id}', 'lead', 'ctx', '{id}', 'Lead {id}', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{id}', 'ctx', 'Plan {id}', '{phase}', 'lead{id}', 't', 't');
             {pull_request}
             INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
               VALUES ('{id}', 'repo', '{repo}', 'pay', 'main', '{status}', '{branch}', '{lead}', cpd_txn(), 't', 't'{values});",
            repo = self.repo.display(),
            branch = created.branch,
            lead = created.path.display(),
        ));
        Lead {
            plan,
            branch: created.branch,
            path: created.path,
        }
    }

    /// Job `key` of `lead`'s PR (`running`, Tier S2, with its Locked-Job fingerprint) and its
    /// Attempt `<key>-a1` in `merging`: its worktree cut from the Lead Branch with one commit
    /// writing `<key>.txt`.
    fn submitted(&self, lead: &Lead, key: &str) -> Submitted {
        let plan = lead.plan.as_str();
        self.fixture(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('{plan}', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ));
        let all = self
            .store()
            .read(|conn| jobs::load(conn, &lead.plan))
            .unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{plan}' AND key = '{key}';
             UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = '{key}';",
            locked_fingerprint(job)
        ));

        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.fx.env.root().join("attempt.log"),
        };
        let made = Ports::scope(&self.fx.fakes.ports, || {
            create_attempt_worktree(&self.repo, &lead.path, &lead.plan, &attempt, &setup)
        })
        .unwrap();
        fs::write(made.path.join(format!("{key}.txt")), format!("{key}\n")).unwrap();
        self.git(&made.path, &["add", "-A"]);
        self.git(&made.path, &["commit", "-q", "-m", key]);
        let sha = self.git(&made.path, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at)
                     VALUES (?1, ?2, ?3, 1, 'S2', 'ladder', 'merging', ?4, ?5, ?6, ?7, 't', 't')",
                    params![plan, attempt.to_string(), key, made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
                )?;
                Ok(())
            })
            .unwrap();
        Submitted {
            plan: lead.plan.clone(),
            attempt,
        }
    }

    /// `cpd pr rebase --plan <plan>` on `ports`: its exit status and its one JSON line.
    fn rebase(&self, ports: &Ports, lead: &Lead) -> Value {
        let argv =
            ["cpd", "pr", "rebase", "--plan", lead.plan.as_str()].map(std::ffi::OsString::from);
        let mut stdout = Vec::new();
        let code = exit_code(cpd::run_with(ports, argv, &mut stdout));
        let text = String::from_utf8(stdout).unwrap();
        assert_eq!(code, 0, "{text}");
        serde_json::from_str(text.lines().last().expect("an output line"))
            .unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    /// `ports` with `inner`'s clock and spawn and a [`Hold`] over its runner.
    fn held(
        &self,
        at: impl Fn(&str, &[String]) -> bool + Send + Sync + 'static,
        during: impl FnOnce() + Send + 'static,
    ) -> Ports {
        let ports = &self.fx.fakes.ports;
        let hold = Hold {
            inner: ports.runner.clone(),
            at: Box::new(at),
            during: Mutex::new(Some(Box::new(during))),
        };
        Ports {
            runner: Arc::new(hold),
            ..ports.clone()
        }
    }

    fn tip(&self, lead: &Lead) -> String {
        self.git(
            &self.repo,
            &["rev-parse", &format!("refs/heads/{}", lead.branch)],
        )
    }

    fn remote_tip(&self, lead: &Lead) -> String {
        self.git(
            &self.origin,
            &["rev-parse", &format!("refs/heads/{}", lead.branch)],
        )
    }

    /// `(status, outcome, merged_sha)` of an Attempt and the status of its Job.
    fn ended(&self, submitted: &Submitted) -> (String, Option<String>, Option<String>, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT a.status, a.outcome, a.merged_sha, j.status FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
                      WHERE a.plan = ?1 AND a.id = ?2",
                    params![submitted.plan.as_str(), submitted.attempt.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }

    /// `(plan, attempt, data)` of every `error` Event, of any Plan, in order.
    fn errors(&self) -> Vec<(Option<String>, Option<String>, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT plan, attempt, data FROM events WHERE kind = 'error' ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            serde_json::from_str(&r.get::<_, String>(2)?).unwrap(),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }
}

/// The status of an Attempt, read on its own connection.
fn status_of(home: &CpdHome, plan: &PlanId, attempt: &AttemptId) -> String {
    Store::open(home, 5000)
        .unwrap()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status FROM attempts WHERE plan = ?1 AND id = ?2",
                params![plan.as_str(), attempt.to_string()],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

// ------------------------------------------------------- two Plans, one repo

#[test]
fn two_plans_merge_into_their_prs_of_one_repo_at_once_with_overlapping_windows_and_no_error() {
    let pay = pay();
    let (pay_lead, web_lead) = (pay.plan("pay", false), pay.plan("web", false));
    let (pay_old, web_old) = (pay.tip(&pay_lead), pay.tip(&web_lead));
    let api = pay.submitted(&pay_lead, "api");
    let ui = pay.submitted(&web_lead, "ui");
    // The first merge's compare-and-swap is held; `ui`'s merge runs whole inside the hold and leaves what it saw.
    let inside: Arc<Mutex<Option<(Merge, String)>>> = Arc::default();
    let ports = {
        let (home, ui, api, inside) = (
            pay.home.clone(),
            (ui.plan.clone(), ui.attempt.clone()),
            (api.plan.clone(), api.attempt.clone()),
            inside.clone(),
        );
        pay.held(is_merge_cas, move || {
            let second = merged(&home, &ui.0, &ui.1);
            *inside.lock().unwrap() = Some((second, status_of(&home, &api.0, &api.1)));
        })
    };

    let first = Ports::scope(&ports, || merged(&pay.home, &api.plan, &api.attempt));

    // `ui` merged while `api` was held at its compare-and-swap: neither merge waited for the other.
    let (second, api_during) = inside
        .lock()
        .unwrap()
        .take()
        .expect("the other merge ran inside the hold");
    assert_eq!(api_during, "merging", "api was held");
    let (api_tip, ui_tip) = (pay.tip(&pay_lead), pay.tip(&web_lead));
    assert_eq!(
        (first.end, second.end),
        (
            merge::End::Merged {
                sha: api_tip.clone()
            },
            merge::End::Merged {
                sha: ui_tip.clone()
            }
        )
    );
    // Each landed on its own Plan's Lead Branch, was pushed, and its Lead worktree followed it.
    for (lead, submitted, old, mine, theirs) in [
        (&pay_lead, &api, &pay_old, "api.txt", "ui.txt"),
        (&web_lead, &ui, &web_old, "ui.txt", "api.txt"),
    ] {
        let tip = pay.tip(lead);
        assert_eq!(
            pay.ended(submitted),
            (
                "ended".into(),
                Some("merged".into()),
                Some(tip.clone()),
                "merged".into()
            )
        );
        assert_eq!(
            &pay.git(&pay.repo, &["rev-parse", &format!("{tip}~1")]),
            old,
            "one commit on {}",
            lead.branch
        );
        assert_eq!(pay.remote_tip(lead), tip);
        assert_eq!(pay.git(&lead.path, &["rev-parse", "HEAD"]), tip);
        assert_eq!(pay.git(&lead.path, &["status", "--porcelain"]), "");
        assert!(
            lead.path.join(mine).exists() && !lead.path.join(theirs).exists(),
            "{}",
            lead.branch
        );
    }
    assert_eq!(
        pay.errors(),
        Vec::<(Option<String>, Option<String>, Value)>::new()
    );
}

// ------------------------------------------------- a merge during pr rebase's Gate

#[test]
fn a_merge_during_pr_rebases_gate_lands_and_reaches_origin_through_the_rebases_push() {
    let pay = pay();
    let lead = pay.plan("pay", true);
    // The Gate's one hook is the repo file's; the fake `sh` runs it, and the merge runs inside that call.
    pay.fx
        .env
        .repo_policy("acme", "pay", "[hooks]\npre-merge = \"the-gate\"\n");
    pay.fx.fake("sh", Reply::ok(""));
    // The Lead Branch has a commit of its own, pushed: the PR's head and the lease, which the rebase rewrites.
    fs::write(lead.path.join("lead.txt"), "lead\n").unwrap();
    pay.git(&lead.path, &["add", "-A"]);
    pay.git(&lead.path, &["commit", "-q", "-m", "lead"]);
    pay.git(&lead.path, &["push", "-q", "origin", &lead.branch]);
    let api = pay.submitted(&lead, "api");
    // The base moved: the rebase rewrites the Lead Branch.
    fs::write(pay.repo.join("base.txt"), "base\n").unwrap();
    pay.git(&pay.repo, &["add", "-A"]);
    pay.git(&pay.repo, &["commit", "-q", "-m", "base"]);
    pay.git(&pay.repo, &["push", "-q", "origin", "main"]);
    // The rebase's compare-and-swap is done (R'); its push waits for the Gate, which is held. The merge runs to its
    // end meanwhile: it lands on R', and its own push, against the remote's pre-rebase head, is rejected.
    let inside: Arc<Mutex<Option<(Merge, String)>>> = Arc::default();
    let ports = {
        let (home, api, plan, repo, inside) = (
            pay.home.clone(),
            api.attempt.clone(),
            lead.plan.clone(),
            pay.repo.clone(),
            inside.clone(),
        );
        let (branch, fakes) = (lead.branch.clone(), pay.fx.fakes.ports.clone());
        pay.held(
            |program, args| program == "sh" && args.first().map(String::as_str) == Some("-c"),
            // A Gate hook runs on a thread of its own, which has no ambient ports: the merge runs on the fakes'.
            move || {
                Ports::scope(&fakes, || {
                    let rebased =
                        cpd_core::git::run::rev_parse(&repo, &format!("refs/heads/{branch}"))
                            .unwrap();
                    *inside.lock().unwrap() = Some((merged(&home, &plan, &api), rebased));
                })
            },
        )
    };

    let out = pay.rebase(&ports, &lead);

    let (merge, rebased) = inside
        .lock()
        .unwrap()
        .take()
        .expect("the merge ran inside the Gate");
    let merged_tip = pay.tip(&lead);
    assert_eq!(
        merge.end,
        merge::End::Merged {
            sha: merged_tip.clone()
        }
    );
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(
        (out["rebased"].as_bool(), out["new"].as_str()),
        (Some(true), Some(rebased.as_str())),
        "{out}"
    );
    assert_eq!(
        pay.ended(&api),
        (
            "ended".into(),
            Some("merged".into()),
            Some(merged_tip.clone()),
            "merged".into()
        )
    );
    // The merge landed on R'. It is an ancestor, not the parent: the fake's rebase keeps the Lead commit the merge replays
    // although R' holds it (patch-id dedup is not modelled, docs/implementing.md "The git route"), where git drops it.
    let landed_on_rebased = pay.fx.git_output(
        &pay.repo,
        &["merge-base", "--is-ancestor", &rebased, &merged_tip],
    );
    assert_eq!(
        landed_on_rebased.status.code(),
        Some(0),
        "{rebased} is not under {merged_tip}"
    );
    // The merge's rejected push is the one Event, as accepted; the rebase pushed R' and then the merge after it, by rule P.
    let errors = pay.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        (errors[0].1.as_deref(), errors[0].2["code"].as_str()),
        (Some("api-a1"), Some("push_failed")),
        "{errors:?}"
    );
    assert_eq!(pay.remote_tip(&lead), merged_tip);
    // The Lead worktree followed the Lead Branch (rule S).
    assert_eq!(pay.git(&lead.path, &["rev-parse", "HEAD"]), merged_tip);
    assert_eq!(pay.git(&lead.path, &["status", "--porcelain"]), "");
    assert!(lead.path.join("api.txt").exists() && lead.path.join("base.txt").exists());
}
