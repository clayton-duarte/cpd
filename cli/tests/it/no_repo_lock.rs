//! No per-repo lock (hs-d243, design `docs/research/no-repo-lock.md` §4): the built
//! driver (`cpd attempt run`, A8) and `pr rebase` against one temporary repo with a
//! bare `origin`, with Plans written straight into the database.
//!
//! - Two Plans' Attempts merge into their two PRs of the one repo at once: a slow
//!   `git` first on `PATH` holds each merge's compare-and-swap until both have
//!   reached it, and records when each ran. With a lock between them the second
//!   never reaches it while the first waits, and the windows do not overlap.
//! - A Job merges into a PR while `pr rebase`'s Gate runs (risk 3): the merge's own
//!   push is rejected, an accepted `push_failed`, and the rebase's push brings the
//!   merge to `origin` by rule P.

use crate::common;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::{CPD, TestEnv, write_executable};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

/// How long the slow `git` holds a merge's compare-and-swap for the other merge to arrive.
const BARRIER_SECONDS: u32 = 10;

/// The repo `pay` with a bare `origin` holding `main`, a config, a fake pi install
/// whose Tier catalog is in the cache, and Context `ctx`. No Plan yet.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
}

/// A Plan's one PR `repo` in [`Fx::repo`]: its Lead Branch `cpd/<plan>` and Lead worktree.
struct Lead {
    plan: PlanId,
    branch: String,
    path: PathBuf,
}

/// An Attempt in `merging`, as `attempt submit` leaves it, and its driver lock file.
struct Submitted {
    plan: PlanId,
    attempt: AttemptId,
    lock: PathBuf,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    // Every test here is about the Gate, which runs only under `pr.gate = "local"` (contract §8.3).
    env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = env.git_repo("pay").canonicalize().unwrap();
    // git runs in this process too (the worktrees), whose environment is not the TestEnv's.
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let origin = env.root().join("origins/pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    // The pi install the driver locates at A1 for its catalog, which is read from the cache.
    env.fake_pi("echo 'no Worker in these tests' >&2; exit 97");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let fx = Fx { env, home, repo, origin };
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!("{catalog} INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');"));
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| Ok(tx.execute_batch(sql)?)).unwrap();
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    /// Plan `id` (locked) with its PR `repo` in [`Fx::repo`], started: Lead Branch
    /// `cpd/<id>` cut from `main` and pushed. `open` makes the PR `open` with a pull
    /// request (Plan phase `deploy`); otherwise it is in `dispatch`.
    fn plan(&self, id: &str, open: bool) -> Lead {
        let plan = PlanId::parse(id).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("lead.log") };
        let created = create_lead_worktree(&self.repo, &plan, "main", &setup).unwrap();
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
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
               VALUES ('{id}', 'repo', '{repo}', 'main', '{status}', '{branch}', '{lead}', cpd_txn(), 't', 't'{values});",
            repo = self.repo.display(),
            branch = created.branch,
            lead = created.path.display(),
        ));
        Lead { plan, branch: created.branch, path: created.path }
    }

    /// Job `key` of `lead`'s PR (`running`, Tier S2, with its Locked-Job fingerprint) and its
    /// Attempt `<key>-a1` in `merging`: its worktree cut from the Lead Branch with one commit
    /// writing `<key>.txt`, and its driver lock file.
    fn submitted(&self, lead: &Lead, key: &str) -> Submitted {
        let plan = lead.plan.as_str();
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('{plan}', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ));
        let all = self.store().read(|conn| jobs::load(conn, &lead.plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{plan}' AND key = '{key}';
             UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = '{key}';",
            locked_fingerprint(job)
        ));

        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&self.repo, &lead.path, &lead.plan, &attempt, &setup).unwrap();
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
        let lock = self.home.attempt_lock(&lead.plan, &attempt);
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        File::create(&lock).unwrap();
        Submitted { plan: lead.plan.clone(), attempt, lock }
    }

    /// The driver of `submitted`, as `job dispatch` starts it: `cpd attempt run` with its
    /// lock as fd 3, in this env (its `PATH` first). It takes the `merging` Attempt up at A8.
    fn drive(&self, submitted: &Submitted) -> Child {
        let mut command = self.env.command("sh");
        command
            .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
            .arg(&submitted.lock)
            .args([CPD, "attempt", "run", &submitted.attempt.to_string(), "--plan", submitted.plan.as_str()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Child(Some(common::spawn(&mut command).unwrap()))
    }

    /// `cpd pr rebase --plan <plan>`, started in the background.
    fn spawn_rebase(&self, lead: &Lead) -> Child {
        let mut command = self.env.command(CPD);
        command.args(["pr", "rebase", "--plan", lead.plan.as_str()]).stdout(Stdio::piped()).stderr(Stdio::piped());
        Child(Some(common::spawn(&mut command).unwrap()))
    }

    fn tip(&self, lead: &Lead) -> String {
        self.git(&self.repo, &["rev-parse", &format!("refs/heads/{}", lead.branch)])
    }

    fn remote_tip(&self, lead: &Lead) -> String {
        self.git(&self.origin, &["rev-parse", &format!("refs/heads/{}", lead.branch)])
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
                let mut stmt = c.prepare("SELECT plan, attempt, data FROM events WHERE kind = 'error' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&r.get::<_, String>(2)?).unwrap())))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// Installs the slow `git` first on the env's `PATH`. It runs the real git, except for a
    /// merge's compare-and-swap (`update-ref -m "cpd: merge <plan>/<attempt>"`): that one
    /// logs `start <plan>/<attempt> <ns>` to `merges.log`, waits until two merges have
    /// arrived there (at most [`BARRIER_SECONDS`]), runs, then logs `end … <ns>`.
    fn slow_git(&self) {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let real = std::env::split_paths(&path)
            .filter(|dir| *dir != self.env.bin())
            .map(|dir| dir.join("git"))
            .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
            .expect("git on PATH");
        let root = self.env.root();
        write_executable(
            &self.env.bin().join("git"),
            &format!(
                r#"#!/bin/sh
real='{real}'
dir='{arrived}'
log='{log}'
cas= merge=
for a in "$@"; do
  case "$a" in
    update-ref) cas=1 ;;
    "cpd: merge "*) merge=${{a#cpd: merge }} ;;
  esac
done
if [ -z "$cas" ] || [ -z "$merge" ]; then exec "$real" "$@"; fi
mkdir -p "$dir"
: > "$dir/$(printf '%s' "$merge" | tr / _)"
echo "start $merge $(date +%s%N)" >> "$log"
i=0
while [ "$i" -lt {ticks} ]; do
  n=0
  for f in "$dir"/*; do [ -e "$f" ] && n=$((n+1)); done
  [ "$n" -ge 2 ] && break
  sleep 0.1
  i=$((i+1))
done
"$real" "$@"
rc=$?
echo "end $merge $(date +%s%N)" >> "$log"
exit $rc
"#,
                real = real.display(),
                arrived = root.join("arrived").display(),
                log = root.join("merges.log").display(),
                ticks = BARRIER_SECONDS * 10,
            ),
        );
    }

    /// The window `(start, end)` in ns of each merge's compare-and-swap, from the slow `git`'s log.
    fn windows(&self) -> BTreeMap<String, (u128, u128)> {
        let log = fs::read_to_string(self.env.root().join("merges.log")).unwrap_or_default();
        let mut windows: BTreeMap<String, (Option<u128>, Option<u128>)> = BTreeMap::new();
        for line in log.lines() {
            let [what, merge, ns] = line.split(' ').collect::<Vec<_>>()[..] else { panic!("bad log line {line:?}: {log}") };
            let ns: u128 = ns.parse().unwrap_or_else(|e| panic!("{e}: {log}"));
            let window = windows.entry(merge.to_owned()).or_default();
            match what {
                "start" => assert!(window.0.replace(ns).is_none(), "two starts of {merge}: {log}"),
                "end" => assert!(window.1.replace(ns).is_none(), "two ends of {merge}: {log}"),
                _ => panic!("bad log line {line:?}: {log}"),
            }
        }
        windows
            .into_iter()
            .map(|(merge, w)| match w {
                (Some(start), Some(end)) => (merge, (start, end)),
                _ => panic!("{merge} has no whole window: {log}"),
            })
            .collect()
    }
}

/// A child process, killed when dropped: a test that panics leaves none behind.
struct Child(Option<std::process::Child>);

impl Child {
    /// Its stdout as JSON, once it has exited (at most 120 s).
    fn finish(mut self) -> Value {
        let child = self.0.as_mut().unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the child did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
        let out = self.0.take().unwrap().wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let last = stdout.lines().last().unwrap_or_else(|| panic!("no output; stderr: {stderr}"));
        serde_json::from_str(last).unwrap_or_else(|e| panic!("{e}: {stdout}\nstderr: {stderr}"))
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// ------------------------------------------------------- two Plans, one repo

#[test]
fn two_plans_merge_into_their_prs_of_one_repo_at_once_with_overlapping_windows_and_no_error() {
    let fx = fx();
    let (pay, web) = (fx.plan("pay", false), fx.plan("web", false));
    let (pay_old, web_old) = (fx.tip(&pay), fx.tip(&web));
    let api = fx.submitted(&pay, "api");
    let ui = fx.submitted(&web, "ui");
    fx.slow_git();

    let (first, second) = (fx.drive(&api), fx.drive(&ui));
    let outs = [first.finish(), second.finish()];

    for (out, id) in outs.iter().zip(["api-a1", "ui-a1"]) {
        assert_eq!(out, &json!({"ok": true, "attempt": id, "outcome": "merged", "cause": "merged"}));
    }
    // Both compare-and-swaps were under way at once: neither merge waited for the other.
    let windows = fx.windows();
    assert_eq!(windows.keys().collect::<Vec<_>>(), ["pay/api-a1", "web/ui-a1"], "{windows:?}");
    let ((api_start, api_end), (ui_start, ui_end)) = (windows["pay/api-a1"], windows["web/ui-a1"]);
    assert!(api_start < ui_end && ui_start < api_end, "the merge windows do not overlap: {windows:?}");
    // Each landed on its own Plan's Lead Branch, was pushed, and its Lead worktree followed it.
    for (lead, submitted, old, mine, theirs) in [(&pay, &api, &pay_old, "api.txt", "ui.txt"), (&web, &ui, &web_old, "ui.txt", "api.txt")] {
        let tip = fx.tip(lead);
        assert_eq!(fx.ended(submitted), ("ended".into(), Some("merged".into()), Some(tip.clone()), "merged".into()));
        assert_eq!(&fx.git(&fx.repo, &["rev-parse", &format!("{tip}~1")]), old, "one commit on {}", lead.branch);
        assert_eq!(fx.remote_tip(lead), tip);
        assert_eq!(fx.git(&lead.path, &["rev-parse", "HEAD"]), tip);
        assert_eq!(fx.git(&lead.path, &["status", "--porcelain"]), "");
        assert!(lead.path.join(mine).exists() && !lead.path.join(theirs).exists(), "{}", lead.branch);
    }
    assert_eq!(fx.errors(), Vec::<(Option<String>, Option<String>, Value)>::new());
}

// ------------------------------------------------- a merge during pr rebase's Gate

/// A `pre-merge` paused mid-run: it creates `<root>/gate-started`, then waits for
/// `<root>/gate-go` (at most 60 s).
fn paused_gate(root: &Path) -> String {
    let (started, go) = (root.join("gate-started"), root.join("gate-go"));
    format!(
        "pre-merge = \"touch {}; i=0; while [ ! -e {} ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done\"\n",
        started.display(),
        go.display()
    )
}

#[test]
fn a_merge_during_pr_rebases_gate_lands_and_reaches_origin_through_the_rebases_push() {
    let fx = fx();
    let pay = fx.plan("pay", true);
    // The Gate's hooks on the Lead Branch, pushed: the PR's head and the lease.
    fs::create_dir_all(pay.path.join(".config")).unwrap();
    fs::write(pay.path.join(".config/wt.toml"), paused_gate(fx.env.root())).unwrap();
    fx.git(&pay.path, &["add", "-A"]);
    fx.git(&pay.path, &["commit", "-q", "-m", "hooks"]);
    fx.git(&pay.path, &["push", "-q", "origin", &pay.branch]);
    let api = fx.submitted(&pay, "api");
    // The base moved: the rebase rewrites the Lead Branch.
    fs::write(fx.repo.join("base.txt"), "base\n").unwrap();
    fx.git(&fx.repo, &["add", "-A"]);
    fx.git(&fx.repo, &["commit", "-q", "-m", "base"]);
    fx.git(&fx.repo, &["push", "-q", "origin", "main"]);

    let rebase = fx.spawn_rebase(&pay);
    let deadline = Instant::now() + Duration::from_secs(120);
    while !fx.env.root().join("gate-started").exists() {
        assert!(Instant::now() < deadline, "the Gate never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The rebase's compare-and-swap is done (R'); its push waits for the Gate. The merge
    // runs to its end meanwhile: it lands on R', and its own push, against the remote's
    // pre-rebase head, is rejected.
    let rebased = fx.tip(&pay);
    let driven = fx.drive(&api).finish();
    assert_eq!(driven, json!({"ok": true, "attempt": "api-a1", "outcome": "merged", "cause": "merged"}));
    let merged = fx.tip(&pay);
    fs::write(fx.env.root().join("gate-go"), "").unwrap();
    let out = rebase.finish();

    assert_eq!(out["ok"], true, "{out}");
    assert_eq!((out["rebased"].as_bool(), out["new"].as_str()), (Some(true), Some(rebased.as_str())), "{out}");
    assert_eq!(fx.ended(&api), ("ended".into(), Some("merged".into()), Some(merged.clone()), "merged".into()));
    assert_eq!(fx.git(&fx.repo, &["rev-parse", &format!("{merged}~1")]), rebased);
    // The merge's rejected push is the one Event, as accepted; the rebase pushed R' and then
    // the merge after it, by rule P.
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!((errors[0].1.as_deref(), errors[0].2["code"].as_str()), (Some("api-a1"), Some("push_failed")), "{errors:?}");
    assert_eq!(fx.remote_tip(&pay), merged);
    // The Lead worktree followed the Lead Branch (rule S).
    assert_eq!(fx.git(&pay.path, &["rev-parse", "HEAD"]), merged);
    assert_eq!(fx.git(&pay.path, &["status", "--porcelain"]), "");
    assert!(pay.path.join("api.txt").exists() && pay.path.join("base.txt").exists());
}
