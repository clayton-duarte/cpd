//! The clean-up `pr poll` runs when it sees a PR merged (commands.md §9.2,
//! contract §7.2; hs-d138, hs-d142): the built binary against temporary repos
//! with a bare `origin`, a fake `gh`, and Plans written straight into the database.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::TestEnv;
use cpd_core::git::{Setup, create_attempt_worktree, prepare_lead_worktree, pr_lead_worktree_path};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::merge::RepoLock;
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

/// The fake `gh`: the search and the lookup are answered from files in `gh/`.
const GH: &str = r#"
dir="$(dirname "$0")/../gh"
case "$1 $2" in
  "api graphql")
    case "$4" in
      *"search(type: ISSUE"*) cat "$dir/search.json" ;;
      *"p0: repository"*) cat "$dir/lookup.json" ;;
      *) echo "unexpected graphql query: $4" >&2; exit 9 ;;
    esac ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

/// A pull request of `acme/pay` as the search and the lookup print it.
fn node(number: i64, state: &str) -> String {
    let merged = if state == "MERGED" { r#""2026-10-14T10:00:00Z""# } else { "null" };
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"PR {number}",
        "isDraft":false,"state":"{state}","createdAt":"opened","mergedAt":{merged},"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN",
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

/// One PR of the Plan: its Lead worktree and one failed Attempt's worktree.
struct Pr {
    key: &'static str,
    number: i64,
    lead: PathBuf,
    lead_branch: String,
    attempt: PathBuf,
    attempt_branch: String,
}

/// Plan `pay` in phase `deploy`, in one repo with a bare `origin`, with one PR per key; every PR is
/// open, its Lead Branch is pushed and its Lead worktree is clean.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    prs: Vec<Pr>,
}

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn fx(keys: &[&'static str]) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
    let gh = env.root().join("gh");
    fs::create_dir_all(&gh).unwrap();
    fs::write(gh.join("search.json"), search_answer(&[])).unwrap();
    fs::write(gh.join("lookup.json"), r#"{"data":{}}"#).unwrap();
    env.fake_gh(GH);

    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    let mut prs = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let lead_branch = format!("cpd/pay/{key}");
        let lead = pr_lead_worktree_path(&repo, &plan(), &PrKey::parse(key).unwrap(), &lead_branch).unwrap();
        let prepared = prepare_lead_worktree(&repo, &plan(), &lead, &lead_branch, "main", &setup).map_err(|f| f.error).unwrap();
        env.git(&lead, &["config", "user.name", "cpd test"]);
        env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
        fs::write(lead.join(format!("{key}.txt")), "x\n").unwrap();
        env.git(&lead, &["add", "-A"]);
        env.git(&lead, &["commit", "-q", "-m", key]);
        env.git(&lead, &["push", "-q", "-u", "origin", &lead_branch]);
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let made = create_attempt_worktree(&repo, &lead, &plan(), &attempt, &setup).unwrap();
        prs.push(Pr {
            key,
            number: 812 + i as i64,
            lead,
            lead_branch: prepared.branch,
            attempt: made.path,
            attempt_branch: made.branch,
        });
    }

    let mut sql = String::from(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'deploy', 'lead1', 't', 't');",
    );
    for pr in &prs {
        sql += &format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {n}, 'https://github.com/acme/pay/pull/{n}', 'PR {n}', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', '{key}', '{repo}', 'main', 'open', '{branch}', '{lead}', cpd_txn(), 't', 't',
                       {n}, 'https://github.com/acme/pay/pull/{n}', 'opened', (SELECT id FROM pull_requests WHERE number = {n}));",
            n = pr.number,
            key = pr.key,
            repo = repo.display(),
            branch = pr.lead_branch,
            lead = pr.lead.display(),
        );
    }
    let fx = Fx { env, home, repo, prs };
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            for pr in &fx.prs {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                     VALUES ('pay', ?1, ?1, 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 0, ?2, 'no', 't', ?3, 't')",
                    params![pr.key, "f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
                                           started_at, ended_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', ?3, ?4, 'sha', 't', 't')",
                    params![format!("{}-a1", pr.key), pr.key, pr.attempt_branch, pr.attempt.to_str().unwrap()],
                )?;
            }
            Ok(())
        })
        .unwrap();
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn gh(&self, file: &str, text: &str) {
        fs::write(self.env.root().join("gh").join(file), format!("{text}\n")).unwrap();
    }

    /// The pull requests GitHub reports: `open` ones in the search, `merged` ones in the lookup.
    fn github(&self, open: &[&str], merged: &[&str]) {
        let number = |key: &str| self.prs.iter().find(|p| p.key == key).unwrap().number;
        let nodes: Vec<String> = open.iter().map(|k| node(number(k), "OPEN")).collect();
        self.gh("search.json", &search_answer(&nodes));
        let aliases: Vec<String> =
            merged.iter().enumerate().map(|(i, k)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(number(k), "MERGED"))).collect();
        self.gh("lookup.json", &format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")));
    }

    fn poll(&self) -> Value {
        self.env.cpd(&["pr", "poll"]).ok()
    }

    fn pr(&self, key: &str) -> &Pr {
        self.prs.iter().find(|p| p.key == key).unwrap()
    }

    fn branch_exists(&self, branch: &str) -> bool {
        !self.git(&self.repo, &["branch", "--list", branch]).is_empty()
    }

    /// Whether the PR's worktrees and local branches are all gone: `(attempt, lead)`.
    fn gone(&self, key: &str) -> (bool, bool) {
        let pr = self.pr(key);
        (
            !pr.attempt.exists() && !self.branch_exists(&pr.attempt_branch),
            !pr.lead.exists() && !self.branch_exists(&pr.lead_branch),
        )
    }

    /// The `data` of the Plan's `error` Events, in order.
    fn errors(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'error' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok(serde_json::from_str(&r.get::<_, String>(0)?).unwrap()))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }
}

#[test]
fn a_merged_pr_loses_its_worktrees_and_branches_and_another_pr_keeps_everything() {
    let fx = fx(&["api", "web"]);
    fx.github(&["web"], &["api"]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "api"}]));
    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.gone("web"), (false, false));
    assert!(fx.pr("web").attempt.exists() && fx.pr("web").lead.exists());
    assert!(fx.branch_exists(&fx.pr("web").attempt_branch) && fx.branch_exists(&fx.pr("web").lead_branch));
    assert_eq!(fx.errors(), Vec::<Value>::new());
    let remote = fx.git(&fx.repo, &["ls-remote", "--heads", "origin", "cpd/pay/api"]);
    assert!(!remote.is_empty(), "the remote branch is never touched");
}

#[test]
fn a_dirty_lead_worktree_is_kept_with_one_error_event() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fs::write(api.lead.join("untracked.txt"), "wip\n").unwrap();
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch));
    assert!(!api.attempt.exists() && !fx.branch_exists(&api.attempt_branch), "the Attempt's worktree goes anyway");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["source"], "pr poll");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"], json!({"plan": "pay", "pr": "api", "path": api.lead.to_str().unwrap(), "reason": "dirty"}));

    fx.poll();

    assert_eq!(fx.errors().len(), 1, "later polls skip it silently");
    assert!(api.lead.exists());

    // Once it is clean, a later poll removes it, still with no Event.
    fs::remove_file(api.lead.join("untracked.txt")).unwrap();
    fx.poll();
    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn a_lead_worktree_with_a_commit_not_on_origin_is_kept() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fs::write(api.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&api.lead, &["add", "-A"]);
    fx.git(&api.lead, &["commit", "-q", "-m", "not pushed"]);
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch));
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["reason"], "unpushed");
    assert_eq!(errors[0]["details"]["path"], api.lead.to_str().unwrap());
}

#[test]
fn a_locked_attempt_worktree_survives_until_it_is_unlocked() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.git(&fx.repo, &["worktree", "lock", api.attempt.to_str().unwrap()]);
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.attempt.exists() && fx.branch_exists(&api.attempt_branch));
    assert!(!api.lead.exists() && !fx.branch_exists(&api.lead_branch), "the failure does not stop the Lead worktree's removal");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["path"], api.attempt.to_str().unwrap());
    assert!(errors[0]["details"]["reason"].as_str().unwrap().contains("locked"), "{errors:?}");

    fx.poll();
    assert!(api.attempt.exists(), "still locked");
    assert_eq!(fx.errors().len(), 1, "a retry appends no Event");

    fx.git(&fx.repo, &["worktree", "unlock", api.attempt.to_str().unwrap()]);
    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn a_held_repo_lock_defers_the_clean_up_to_a_later_poll_with_no_event() {
    // hs-n124: a Gate holds the repo lock in the Lead worktree while the PR merges on GitHub.
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.github(&[], &["api"]);

    let lock = RepoLock::wait(&fx.home, &fx.repo).unwrap();
    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "api"}]));
    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch), "the Lead worktree stays under a held lock");
    assert!(api.attempt.exists() && fx.branch_exists(&api.attempt_branch));
    assert_eq!(fx.errors(), Vec::<Value>::new());

    fx.poll();
    assert!(api.lead.exists(), "still held");
    assert_eq!(fx.errors(), Vec::<Value>::new());

    drop(lock);
    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors(), Vec::<Value>::new());
}
