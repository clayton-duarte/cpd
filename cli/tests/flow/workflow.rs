//! The hs-d680 workflow worktree model (contract §7.1, §7.2, §7.4; Plan `gate-redefine`).
//!
//! One **workflow worktree** per PR (the PR's Lead worktree on the Lead Branch `cpd/<plan>/<key>`):
//! linear Jobs run in it, sequentially, and their commits land directly on the Lead Branch (no
//! per-Attempt worktree, no per-Attempt branch, no A8 rebase+fast-forward). Parallel sibling Jobs
//! (Jobs of one PR with no needs between them, scheduled together) get X-1 extra worktrees off the
//! Lead Branch tip and merge back like today's A8; the first sibling keeps the workflow worktree.
//! The open Job runs both gates in the workflow worktree, pushes the Lead Branch and opens the PR.
//! Cleanup on merge or cancel removes the workflow and parallel worktrees and their branches,
//! keeping hs-d517's dirty/unpushed keep rules. The needs graph is the only source of extra
//! worktrees and there is no new config key.
//!
//! The tests are red: the built binary runs the CLI against temporary repos, the merge and the
//! starter run in this process on the same store and config, and what they assert is the CLI JSON,
//! the git state and the store, never a Rust field that does not exist yet.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::TestEnv;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::driver::{self, StartCtx};
use cpd_core::error::CpdError;
use cpd_core::git::{self, Setup, create_attempt_worktree, prepare_lead_worktree, pr_lead_worktree_path};
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::FakeSpawn;
use cpd_core::ids::{AttemptId, JobKey, PlanId, PrKey};
use cpd_core::merge::{self, End, Merge};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use serde_json::{Value, json};

const PLAN: &str = "pay";
const PR: &str = "api";
const NUMBER: i64 = 812;

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

fn pr_key() -> PrKey {
    PrKey::parse(PR).unwrap()
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
    Catalog { pi_version: "1.0.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } }
}

fn metrics(_: &merge::Attempt) -> Option<Value> {
    None
}

/// A repo `pay` with a bare `origin`, the PR `api`'s **workflow worktree** on `cpd/pay/api`, and
/// the Plan, Context and Session rows a Job needs. The PR is `dispatch`.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    config: Config,
    catalog: Catalog,
    repo: PathBuf,
    origin: PathBuf,
    /// The workflow worktree (the PR's Lead worktree).
    lead: PathBuf,
    lead_branch: String,
    team: String,
}

fn real() -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    identify(&env, &repo);
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.local_origin(&repo, &origin);
    env.git(&repo, &["push", "-q", "origin", "main"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
    let config = Config::load(&home).unwrap();
    // The PR is named as its repo (`pay`): its policy file is `acme/pay`.
    env.repo_policy("acme", "pay", "");

    let lead_branch = format!("cpd/{PLAN}/{PR}");
    let lead = pr_lead_worktree_path(&repo, &plan(), &pr_key(), &lead_branch).unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    prepare_lead_worktree(&repo, &plan(), &lead, &lead_branch, "main", &setup).map_err(|f| f.error).unwrap();
    identify(&env, &lead);

    let mut fx = Fx { env, home, config, catalog: catalog(), repo, origin, lead, lead_branch, team: String::new() };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', '{PLAN}', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('{PLAN}', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, created_txn, created_at, started_at)
           VALUES ('{PLAN}', '{PR}', '{repo}', 'pay', 'main', 'dispatch', '{branch}', '{lead}', 'Charge the card at checkout', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        branch = fx.lead_branch,
        lead = fx.lead.display()
    ));
    fx
}

/// The merge runs git in this process, whose environment is not the TestEnv's: the repo's own config gives rebase an identity.
fn identify(env: &TestEnv, repo: &Path) {
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(repo, &["config", key, value]);
    }
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", &format!("refs/heads/{}", self.lead_branch)])
    }

    fn branch_exists(&self, branch: &str) -> bool {
        !self.git(&self.repo, &["branch", "--list", branch]).is_empty()
    }

    /// Commits `path` = `content` in `dir`'s checkout; returns the new tip.
    fn commit(&self, dir: &Path, path: &str, content: &str) -> String {
        let full = dir.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", &format!("edit {path}")]);
        self.git(dir, &["rev-parse", "HEAD"])
    }

    /// The PR's workflow worktree and Lead Branch as the store holds them.
    fn workflow(&self) -> (String, String) {
        let row: String = self.env.query("SELECT lead_branch || '|' || lead_worktree FROM prs WHERE plan = 'pay' AND key = 'api'");
        row.split_once('|').map(|(b, w)| (b.to_owned(), w.to_owned())).unwrap()
    }

    /// Job `key` of the PR, Tier S2, `queued`, needing `needs`, with its Locked-Job fingerprint.
    fn add_job(&self, key: &str, kind: &str, needs: &[&str]) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
               VALUES ('{PLAN}', '{PR}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, '{kind}', cpd_txn(), 't');"
        ));
        for need in needs {
            self.env.sql(&format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{PLAN}', '{key}', '{need}');"));
        }
        let all = self.store().read(|c| jobs::load(c, &plan())).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.env.sql(&format!("UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{PLAN}' AND key = '{key}';", locked_fingerprint(job)));
    }

    fn set_job(&self, key: &str, status: &str) {
        self.env.sql(&format!("UPDATE jobs SET status = '{status}' WHERE plan = '{PLAN}' AND key = '{key}';"));
    }

    /// An Attempt row of Job `job`, in `status`, with its branch and worktree, submitted at `submitted`.
    fn add_attempt(&self, id: &str, job: &str, branch: &str, worktree: &Path, status: &str, submitted: Option<&str>) {
        let submitted = submitted.map_or("NULL".to_owned(), |sha| format!("'{sha}'"));
        let (outcome, cause, ended, submitted_at) = match status {
            "ended" => ("'failed'", "'crashed'", "'t'", "NULL"),
            _ => ("NULL", "NULL", "NULL", "'t'"),
        };
        self.env.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha, submitted_sha, submitted_at, started_at, ended_at)
               VALUES ('{PLAN}', '{id}', '{job}', 1, 'S2', 'ladder', '{status}', {outcome}, {cause}, '{branch}', '{worktree}', 'sha', {submitted}, {submitted_at}, 't', {ended});",
            worktree = worktree.display()
        ));
    }

    /// A linear Job's Attempt: its commit is made **in the workflow worktree on the Lead Branch**, and the Attempt's
    /// `branch` is the Lead Branch and its `worktree` the workflow worktree. `status` is `merging` (submitted) or `ended`.
    fn linear(&self, key: &str, kind: &str, path: &str, status: &str) -> (AttemptId, String) {
        self.add_job(key, kind, &[]);
        self.set_job(key, if status == "ended" { "merged" } else { "running" });
        let sha = self.commit(&self.lead, path, &format!("{path}\n"));
        self.add_attempt(&format!("{key}-a1"), key, &self.lead_branch, &self.lead, status, (status == "merging").then_some(sha.as_str()));
        (AttemptId::parse(&format!("{key}-a1")).unwrap(), sha)
    }

    /// A parallel sibling's Attempt: its own worktree off the Lead Branch tip on its own per-Attempt branch, with one commit.
    fn sibling(&self, key: &str, path: &str, status: &str) -> (AttemptId, PathBuf, String) {
        self.add_job(key, "work", &[]);
        self.set_job(key, if status == "ended" { "merged" } else { "running" });
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("sibling.log") };
        let made = create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup).unwrap();
        identify(&self.env, &made.path);
        let sha = self.commit(&made.path, path, &format!("{path}\n"));
        self.add_attempt(&attempt.to_string(), key, &made.branch, &made.path, status, (status == "merging").then_some(sha.as_str()));
        (attempt, made.path, sha)
    }

    /// Starts Job `key` through the starter `job dispatch`/E3 use, on a `FakeSpawn`: no driver process runs.
    fn start(&self, key: &str) -> Result<driver::Started, CpdError> {
        let spawn = std::sync::Arc::new(FakeSpawn::new());
        let exe = self.env.root().join("cpd-fake");
        fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
        spawn.set_self_exe(exe);
        let ports = Ports { clock: Ports::current().clock, runner: Ports::current().runner, spawn };
        let ctx = StartCtx { home: &self.home, config: &self.config, catalog: &self.catalog, command: "workflow test" };
        driver::start_with(&ports, &mut self.store(), &ctx, &plan(), &JobKey::parse(key).unwrap())
    }

    /// Driver step A8, as `attempt run` runs it.
    fn merge(&self, attempt: &AttemptId) -> Merge {
        let ctx = merge::Ctx { home: &self.home, config: &self.config, catalog: &self.catalog, source: "workflow test", metrics: &metrics };
        merge::merge(&mut self.store(), &ctx, &plan(), attempt).unwrap()
    }

    /// The Attempt's `(status, outcome, cause, merged_sha, branch, worktree)`.
    fn attempt_row(&self, id: &str) -> (String, Option<String>, Option<String>, Option<String>, String, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, outcome, cause, merged_sha, branch, worktree FROM attempts WHERE plan = 'pay' AND id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?)
            })
            .unwrap()
    }

    fn policy(&self, toml: &str) {
        self.env.repo_policy("acme", "pay", toml);
    }

    /// Writes `$PWD` to `name` under the env's root (a Gate hook's cwd is the worktree it runs in).
    fn pwd_hook(&self, name: &str) -> PathBuf {
        let marker = self.env.root().join(name);
        self.policy(&format!(
            "[gate]\nmerge = \"local\"\npr = \"local\"\n\n[hooks]\npre-commit = \"pwd > {}\"\npre-merge = \"pwd > {}\"\n",
            marker.display(),
            marker.display()
        ));
        marker
    }

    /// A fake `gh` answering the poll's search, lookup and `pr view` from files in `gh/`.
    fn gh_script(&self) {
        let gh = self.env.root().join("gh");
        fs::create_dir_all(&gh).unwrap();
        fs::write(gh.join("search.json"), search_answer(&[])).unwrap();
        fs::write(gh.join("lookup.json"), r#"{"data":{}}"#).unwrap();
        self.env.fake_gh(GH);
    }

    fn gh(&self, file: &str, text: &str) {
        fs::write(self.env.root().join("gh").join(file), format!("{text}\n")).unwrap();
    }

    /// The pull requests GitHub reports: `open` ones in the search, `merged` ones in the lookup, the rest `OPEN`.
    fn github(&self, open: &[&str], merged: &[&str]) {
        let nodes: Vec<String> = open.iter().map(|_| node(NUMBER, "OPEN")).collect();
        self.gh("search.json", &search_answer(&nodes));
        let aliases: Vec<String> = if merged.is_empty() {
            vec![format!(r#""p0":{{"pullRequest":{}}}"#, node(NUMBER, "OPEN"))]
        } else {
            merged.iter().enumerate().map(|(i, _)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(NUMBER, "MERGED"))).collect()
        };
        self.gh("lookup.json", &format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")));
    }

    /// Makes the PR `open` with a stored pull request, as `pr open` leaves it.
    fn open_pr(&self) {
        self.env.sql(&format!(
            "UPDATE plans SET phase = 'deploy' WHERE id = '{PLAN}';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {NUMBER}, 'https://github.com/acme/pay/pull/{NUMBER}', 'PR', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
             UPDATE prs SET status = 'open', pr_number = {NUMBER}, pr_url = 'https://github.com/acme/pay/pull/{NUMBER}', pr_opened_at = 'opened',
                            pull_request = (SELECT id FROM pull_requests WHERE number = {NUMBER})
               WHERE plan = '{PLAN}' AND key = '{PR}';"
        ));
    }

    /// The `data` of the Plan's `error` Events, in order.
    fn errors(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'error' ORDER BY id")?;
                let rows = stmt.query_map([], |r| Ok(serde_json::from_str(&r.get::<_, String>(0)?).unwrap()))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn poll(&self) -> Value {
        self.env.cpd(&["pr", "poll"]).ok()
    }
}

/// A pull request of `acme/pay` as the search and the lookup print it.
fn node(number: i64, state: &str) -> String {
    let merged = if state == "MERGED" { r#""2026-10-14T10:00:00Z""# } else { "null" };
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"PR {number}",
        "isDraft":false,"state":"{state}","createdAt":"opened","mergedAt":{merged},"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

fn search_answer(nodes: &[String]) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":{},"nodes":[{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#,
        nodes.len(),
        nodes.join(",")
    )
}

const GH: &str = r#"
dir="$(dirname "$0")/../gh"
case "$1 $2" in
  "api graphql")
    case "$4" in
      *"search(type: ISSUE"*) cat "$dir/search.json" ;;
      *"p0: repository"*) cat "$dir/lookup.json" ;;
      *) echo "unexpected graphql query: $4" >&2; exit 9 ;;
    esac ;;
  "pr view") cat "$dir/view.json" ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

// ------------------------------------------------------------ the workflow worktree

#[test]
fn a_linear_plan_runs_its_jobs_in_the_workflow_worktree_on_the_lead_branch() {
    let fx = real();
    fx.add_job("api", "work", &[]);
    let (branch, worktree) = fx.workflow();

    let started = fx.start("api").unwrap();

    let (status, _, _, _, got_branch, got_worktree) = fx.attempt_row(&started.attempt.to_string());
    assert_eq!(status, "starting");
    assert_eq!(got_branch, branch, "a linear Job runs on the Lead Branch");
    assert_eq!(got_worktree, worktree, "a linear Job runs in the workflow worktree");
}

#[test]
fn a_linear_job_creates_no_attempt_worktree_or_attempt_branch() {
    let fx = real();
    fx.add_job("api", "work", &[]);
    let (branch, worktree) = fx.workflow();

    let started = fx.start("api").unwrap();

    let (_, _, _, _, got_branch, got_worktree) = fx.attempt_row(&started.attempt.to_string());
    let attempt_branch = git::attempt_branch(&plan(), &started.attempt);
    let attempt_worktree = git::attempt_worktree_path(&fx.repo, &plan(), &started.attempt).unwrap();
    assert_eq!(got_branch, branch, "the Attempt has no per-Attempt branch");
    assert_eq!(got_worktree, worktree, "the Attempt has no per-Attempt worktree");
    assert_ne!(got_branch, attempt_branch);
    assert_ne!(got_worktree, attempt_worktree.display().to_string());
    assert!(!fx.branch_exists(&attempt_branch), "no per-Attempt branch was created");
    assert!(!attempt_worktree.exists(), "no per-Attempt worktree was created");
}

#[test]
fn parallel_sibling_jobs_get_extra_worktrees_off_the_lead_branch_tip() {
    let fx = real();
    fx.add_job("api", "work", &[]);
    fx.add_job("web", "work", &[]);
    let (branch, worktree) = fx.workflow();
    let tip = fx.lead_sha();

    let first = fx.start("api").unwrap();
    let second = fx.start("web").unwrap();

    let (_, _, _, _, b1, w1) = fx.attempt_row(&first.attempt.to_string());
    assert_eq!((b1.as_str(), w1.as_str()), (branch.as_str(), worktree.as_str()), "the first sibling keeps the workflow worktree");
    let (_, _, _, _, b2, w2) = fx.attempt_row(&second.attempt.to_string());
    let extra = git::attempt_worktree_path(&fx.repo, &plan(), &second.attempt).unwrap();
    assert_eq!(b2, git::attempt_branch(&plan(), &second.attempt), "the second sibling gets its own branch");
    assert_eq!(w2, extra.display().to_string(), "the second sibling gets an extra worktree off the Lead Branch tip");
    // A2 cuts that extra worktree from the Lead Branch tip.
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("extra.log") };
    let made = create_attempt_worktree(&fx.repo, &fx.lead, &plan(), &second.attempt, &setup).unwrap();
    assert_eq!(made.base_sha, tip, "the extra worktree is cut from the Lead Branch tip");
}

#[test]
fn the_scheduler_derives_extra_worktrees_from_the_needs_graph() {
    let fx = real();
    fx.add_job("api", "work", &[]);
    fx.add_job("web", "work", &[]);
    // `app` needs `api`: it may not run beside it, and gets no worktree.
    fx.add_job("app", "work", &["api"]);
    let (branch, worktree) = fx.workflow();

    let first = fx.start("api").unwrap();
    let second = fx.start("web").unwrap();
    let third = fx.start("app");

    let (_, _, _, _, b1, w1) = fx.attempt_row(&first.attempt.to_string());
    assert_eq!((b1, w1), (branch, worktree));
    assert_ne!(fx.attempt_row(&second.attempt.to_string()).4, fx.workflow().0);
    let err = third.unwrap_err();
    assert_eq!(err.code(), cpd_core::error::ErrorCode::JobNotReady, "{err:?}");
    let attempts: i64 = fx.env.query("SELECT count(*) FROM attempts");
    assert_eq!(attempts, 2, "the only extra worktree is the one the needs graph allows");
}

// ------------------------------------------------------------ landing

#[test]
fn a_linear_job_lands_its_commits_directly_on_the_lead_branch() {
    let fx = real();
    let (attempt, _) = fx.linear("api", "work", "api.txt", "merging");
    let before = fx.lead_sha();

    let result = fx.merge(&attempt);

    assert_eq!(result.end, End::Merged { sha: before.clone() }, "a linear Job lands at the Lead Branch tip");
    assert_eq!(fx.lead_sha(), before, "no rebase, no fast-forward moved the Lead Branch");
    let (status, outcome, cause, merged, branch, _) = fx.attempt_row(&attempt.to_string());
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged.as_deref()), ("ended", Some("merged"), Some("merged"), Some(before.as_str())));
    assert_eq!(branch, fx.lead_branch, "the Attempt's branch is the Lead Branch");
    assert!(!fx.branch_exists(&format!("cpd/{PLAN}--{attempt}")), "no per-Attempt branch was left");
}

#[test]
fn a_parallel_sibling_merges_back_without_touching_the_workflow_worktree() {
    let fx = real();
    // A linear Job already landed on the Lead Branch, in the workflow worktree.
    let (_, linear_sha) = fx.linear("api", "work", "api.txt", "merging");
    let (sibling, sibling_worktree, sibling_sha) = fx.sibling("web", "web.txt", "merging");
    let linear_tree = fx.git(&fx.lead, &["rev-parse", "HEAD^{tree}"]);

    let result = fx.merge(&sibling);

    assert!(matches!(result.end, End::Merged { .. }), "{result:?}");
    let contained = fx.git(&fx.repo, &["merge-base", "--is-ancestor", &sibling_sha, &fx.lead_branch]);
    assert_eq!(contained, "", "the sibling fast-forwarded into the Lead Branch");
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &linear_sha, &fx.lead_branch]).is_empty(), "the linear commit is still on the Lead Branch");
    // The workflow worktree follows the Lead Branch (rule S) and holds both jobs' files, clean.
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD^{tree}"]), fx.git(&fx.repo, &["rev-parse", &format!("{}^{{tree}}", fx.lead_branch)]));
    assert_ne!(fx.git(&fx.lead, &["rev-parse", "HEAD^{tree}"]), linear_tree);
    assert!(fx.lead.join("web.txt").exists(), "the workflow worktree followed the merged sibling");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "", "the workflow worktree is clean");
    assert!(sibling_worktree.exists(), "the sibling worktree is the driver A9's to remove, not the merge's");
}

#[test]
fn both_gates_run_in_the_workflow_worktree_before_the_pr_lands() {
    let fx = real();
    let (attempt, _) = fx.linear("api", "open", "api.txt", "merging");
    let merge_marker = fx.pwd_hook("gate.merge.pwd");
    let pr_marker = fx.env.root().join("gate.pr.pwd");
    fx.policy(&format!(
        "[gate]\nmerge = \"local\"\npr = \"local\"\n\n[hooks]\npre-commit = \"pwd > {}\"\npre-merge = \"pwd > {}\"\n",
        merge_marker.display(),
        pr_marker.display()
    ));

    // The merge runs in this process: put the env's `bin/` first so its `gh` shim answers, never the network.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let shimmed = std::env::join_paths(std::iter::once(fx.env.bin()).chain(std::env::split_paths(&path))).unwrap();
    unsafe { std::env::set_var("PATH", &shimmed) };
    let _ = fx.merge(&attempt);
    // SAFETY: nextest gives this test a process of its own.
    unsafe { std::env::set_var("PATH", &path) };

    let workflow = fs::canonicalize(&fx.lead).unwrap();
    let read = |p: &Path| fs::canonicalize(fs::read_to_string(p).unwrap().trim()).unwrap();
    assert!(merge_marker.exists(), "gate.merge (the repo's pre-commit) did not run");
    assert!(pr_marker.exists(), "gate.pr (the repo's pre-merge) did not run");
    assert_eq!(read(&merge_marker), workflow, "gate.merge ran in the workflow worktree");
    assert_eq!(read(&pr_marker), workflow, "gate.pr ran in the workflow worktree");
}

// ------------------------------------------------------------ the state machine

#[test]
fn the_attempt_state_machine_is_unchanged() {
    let fx = real();
    fx.add_job("api", "work", &[]);
    let (branch, worktree) = fx.workflow();

    let started = fx.start("api").unwrap();
    assert_eq!(fx.attempt_row(&started.attempt.to_string()).0, "starting");

    fx.env.sql(&format!("UPDATE attempts SET status = 'running' WHERE plan = '{PLAN}' AND id = '{}';", started.attempt));
    assert_eq!(fx.attempt_row(&started.attempt.to_string()).0, "running");

    fx.set_job("api", "running");
    let sha = fx.commit(&fx.lead, "api.txt", "api\n");
    fx.env.sql(&format!(
        "UPDATE attempts SET status = 'merging', submitted_sha = '{sha}', submitted_at = 't', branch = '{branch}', worktree = '{worktree}' WHERE plan = '{PLAN}' AND id = '{}';",
        started.attempt
    ));
    assert_eq!(fx.attempt_row(&started.attempt.to_string()).0, "merging");

    let result = fx.merge(&started.attempt);
    assert!(matches!(result.end, End::Merged { .. }), "{result:?}");
    let (status, outcome, cause, _, got_branch, got_worktree) = fx.attempt_row(&started.attempt.to_string());
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref()), ("ended", Some("merged"), Some("merged")));
    assert_eq!((got_branch, got_worktree), (branch, worktree), "the same four states, only the branch and worktree change");
}

// ------------------------------------------------------------ clean-up

/// A PR as cleanup sees it: an open PR, its workflow worktree and a parallel sibling's worktree, both idle.
fn cleanup_fixture() -> Fx {
    let fx = real();
    fx.gh_script();
    let _ = fx.linear("api", "work", "api.txt", "ended");
    let _ = fx.sibling("web", "web.txt", "ended");
    fx.git(&fx.lead, &["push", "-q", "-u", "origin", &fx.lead_branch]);
    fx.open_pr();
    fx
}

#[test]
fn cleanup_removes_the_workflow_and_parallel_worktrees_on_merge_or_cancel() {
    // On merge.
    let fx = cleanup_fixture();
    let sibling = fx.git(&fx.repo, &["worktree", "list", "--porcelain"]).lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).find(|p| p != &fx.repo && p != &fx.lead).unwrap();
    let sibling_branch = fx.git(&sibling, &["rev-parse", "--abbrev-ref", "HEAD"]);
    fx.github(&[], &["api"]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": PLAN, "pr": PR}]));
    assert!(!fx.lead.exists(), "the workflow worktree is gone");
    assert!(!fx.branch_exists(&fx.lead_branch), "the Lead Branch is gone");
    assert!(!sibling.exists(), "the parallel sibling's worktree is gone");
    assert!(!fx.branch_exists(&sibling_branch), "the per-Attempt branch is gone");
    assert_eq!(fx.errors(), Vec::<Value>::new());

    // On cancel (a PR `pr cancel` left cancelled): the same worktrees go.
    let fx = cleanup_fixture();
    let sibling = fx.git(&fx.repo, &["worktree", "list", "--porcelain"]).lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).find(|p| p != &fx.repo && p != &fx.lead).unwrap();
    let sibling_branch = fx.git(&sibling, &["rev-parse", "--abbrev-ref", "HEAD"]);
    fx.env.sql(&format!(
        "UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL, pr_number = NULL, pr_url = NULL, pr_opened_at = NULL, pull_request = NULL WHERE plan = '{PLAN}' AND key = '{PR}';"
    ));
    fx.github(&[], &[]);

    fx.poll();

    assert!(!fx.lead.exists(), "the workflow worktree is gone");
    assert!(!fx.branch_exists(&fx.lead_branch), "the Lead Branch is gone");
    assert!(!sibling.exists(), "the parallel sibling's worktree is gone");
    assert!(!fx.branch_exists(&sibling_branch), "the per-Attempt branch is gone");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_dirty_or_unpushed_workflow_worktree_is_kept_on_cleanup() {
    let fx = cleanup_fixture();
    let sibling = fx.git(&fx.repo, &["worktree", "list", "--porcelain"]).lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).find(|p| p != &fx.repo && p != &fx.lead).unwrap();
    let sibling_branch = fx.git(&sibling, &["rev-parse", "--abbrev-ref", "HEAD"]);
    fs::write(fx.lead.join("untracked.txt"), "wip\n").unwrap();
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(fx.lead.exists() && fx.branch_exists(&fx.lead_branch), "a dirty workflow worktree is kept");
    assert!(!sibling.exists() && !fx.branch_exists(&sibling_branch), "the sibling's worktree goes anyway");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["path"], fx.lead.to_str().unwrap());
    assert_eq!(errors[0]["details"]["reason"], "dirty");
}
