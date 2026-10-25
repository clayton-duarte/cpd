//! The clean-up `pr poll` runs when it sees a PR cancelled (commands.md §9.2 "After a PR is merged or cancelled",
//! §9.6; hs-d517): the same fixtures as pr_cleanup.rs, with the PR then cancelled. A cancelled PR's row has no
//! lead_worktree, so its paths come from contract §7.1's naming rule.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::TestEnv;
use cpd_core::git::{Setup, create_attempt_worktree, prepare_lead_worktree, pr_lead_worktree_path};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

/// The fake `gh`: the search, the lookup and `gh pr view` (the pull request's `headRefOid`, contract §7.2 "The recorded
/// head") are answered from files in `gh/`; without a `view.json`, `gh pr view` fails.
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
    env.local_origin(&repo, &origin);
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
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
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
        // The direct read names every stored open PR the search missed, in id order: a merged one answers MERGED, the
        // others (a cancelled PR's row is still stored `open` and authored) answer OPEN.
        let aliases: Vec<String> = self
            .prs
            .iter()
            .filter(|p| !open.contains(&p.key))
            .enumerate()
            .map(|(i, p)| {
                let state = if merged.contains(&p.key) { "MERGED" } else { "OPEN" };
                format!(r#""p{i}":{{"pullRequest":{}}}"#, node(number(p.key), state))
            })
            .collect();
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

/// PR `key` as `pr cancel` leaves it: `status` as given, no Lead worktree, no pull request (the `prs` table allows none).
fn cancel_in_db(fx: &Fx, key: &str) {
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute(
                "UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL, pr_number = NULL, pr_url = NULL,
                                pr_opened_at = NULL, pull_request = NULL WHERE plan = 'pay' AND key = ?1",
                [key],
            )?)
        })
        .unwrap();
}

#[test]
fn cancelled_dispatch_pr_worktrees_are_removed() {
    let fx = fx(&["api", "web"]);
    // A started PR whose pull request was never opened, cancelled: the row as `pr cancel` leaves it.
    cancel_in_db(&fx, "api");
    fx.store()
        .write("fixture", |tx| Ok(tx.execute("DELETE FROM pull_requests WHERE number = 812", [])?))
        .unwrap();
    fx.github(&["web"], &[]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.gone("web"), (false, false), "another PR keeps everything");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn cancelled_open_pr_worktrees_are_removed() {
    let fx = fx(&["api", "web"]);
    cancel_in_db(&fx, "api");
    fx.github(&["web"], &[]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.gone("web"), (false, false), "another PR keeps everything");
    assert_eq!(fx.errors(), Vec::<Value>::new());
    let remote = fx.git(&fx.repo, &["ls-remote", "--heads", "origin", "cpd/pay/api"]);
    assert!(!remote.is_empty(), "the remote branch is never touched");
}

#[test]
fn dirty_cancelled_lead_worktree_is_kept_with_one_event() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fs::write(api.lead.join("untracked.txt"), "wip\n").unwrap();
    cancel_in_db(&fx, "api");
    fx.github(&[], &[]);

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
}
