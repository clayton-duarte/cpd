//! The start and the merge of an adopted PR (contract §7.2, §7.4; commands.md §9.7): the Lead Branch is cut from
//! `origin/<head_ref>`, and only an `open` or `update` Job's merge pushes, to the pull request's own head branch.
//! The built binary and the `cpd_core` merge API against a temporary repo with a bare `origin` whose branch `feat/x`
//! (the pull request's head, #7) has one commit beyond `main`, and a fake `gh`.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::{CPD, TestEnv};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_attempt_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, End};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/7";

struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    /// The tip of `feat/x` on `origin`.
    head: String,
}

/// The fake `gh`: `pr view` and `pr list` print files of the root, `api user` prints `alice`, the lookup prints `lookup.json`,
/// and every other graphql call (the poll's search) finds nothing.
const GH: &str = r#"
root="ROOT"
case "$1 $2" in
  "pr view") cat "$root/view.json" ;;
  "api user") echo alice ;;
  "pr list") if [ "$4" = "feat/x" ]; then cat "$root/list.json"; else echo '[]'; fi ;;
  "api graphql")
    case "$4" in
      *"p0: repository"*) cat "$root/lookup.json" ;;
      *) echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}' ;;
    esac ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

/// Repo `pay` with its bare `origin` holding `main` and `feat/x` (one commit writing `clash.txt` past `main`); Plan `pay` in
/// `plan`; the pull request #7 of `feat/x` adopted as PR `web`. When `conflicting`, `main` then moves on `origin`, writing
/// `clash.txt` differently, so that `feat/x` conflicts with it.
fn fx(conflicting: bool) -> Fx {
    let env = TestEnv::new();
    let config = format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env.fake("cpd", &format!("if [ \"$1 $2\" = 'watch --ensure' ]; then {}; fi\nexec '{CPD}' \"$@\"", common::WATCH_OK));
    env.fake_gh(&GH.replace("ROOT", &env.root().display().to_string()));
    fs::write(env.root().join("list.json"), "[]").unwrap();
    fs::write(env.root().join("lookup.json"), r#"{"data":{}}"#).unwrap();
    fs::write(
        env.root().join("view.json"),
        json!({"number": 7, "url": URL, "state": "OPEN", "isCrossRepository": false, "headRefName": "feat/x", "baseRefName": "main", "author": {"login": "alice"}}).to_string(),
    )
    .unwrap();

    let repo = env.keyed_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("origins/pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.local_origin(&repo, &origin);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    fs::write(repo.join("clash.txt"), "the head's\n").unwrap();
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "the head"]);
    let head = env.git(&repo, &["rev-parse", "HEAD"]);
    env.git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/feat/x"]);
    env.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    if conflicting {
        let other = env.root().join("other-clone");
        env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), other.to_str().unwrap()]);
        fs::write(other.join("clash.txt"), "main's\n").unwrap();
        env.git(&other, &["add", "-A"]);
        env.git(&other, &["commit", "-q", "-m", "main moves"]);
        env.git(&other, &["push", "-q", "origin", "main"]);
    }

    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    env.cpd(&["workflow", "add", "web", "--repo", repo.to_str().unwrap(), "--title", "Polish", "--shape", "config", "--adopt", "7", "--plan", "pay"]).ok();
    env.cpd(&["job", "cancel", "web-config", "--reason", "a stub of the template", "--plan", "pay"]).ok();
    env.sql("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';");
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Fx { env, home, repo, origin, head }
}

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &std::path::Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn lead(&self) -> PathBuf {
        self.repo.parent().unwrap().join("pay.pay.web")
    }

    fn start(&self) -> common::Run {
        self.env.cpd(&["workflow", "start", "web", "--plan", "pay"])
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/pay/web"])
    }

    fn remote_head(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/feat/x"])
    }

    /// The branches `origin` holds under `pay/`.
    fn remote_cpd_branches(&self) -> String {
        self.git(&self.origin, &["branch", "--list", "pay/*"])
    }

    /// PR `web` `open`, with its pull request, as `pr open` leaves it.
    fn open(&self) {
        self.env.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
               VALUES ('acme/pay', 7, '{URL}', 'T', 0, 'open', 'none', 'none', 't', 't');
             UPDATE workflows SET status = 'open', pr_number = 7, pr_url = '{URL}', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/pay') WHERE plan = 'pay' AND key = 'web';"
        ));
    }

    /// Job `m` of kind `kind` with Attempt `m-a1` in `merging`, as `attempt submit` leaves it: its worktree cut from the
    /// Lead Branch, with one commit writing `m.txt`.
    fn submitted(&self, kind: &str) -> AttemptId {
        let attempt = AttemptId::parse("m-a1").unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&self.repo, &self.lead(), &plan(), &attempt, &setup).unwrap();
        fs::write(made.path.join("m.txt"), "m\n").unwrap();
        self.git(&made.path, &["add", "-A"]);
        self.git(&made.path, &["commit", "-q", "-m", "m"]);
        let sha = self.git(&made.path, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, kind, created_txn, created_at)
                     VALUES ('pay', 'web', 'm', 'M', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, ?3, 't')",
                    params!["f".repeat(64), kind, tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at)
                     VALUES ('pay', 'm-a1', 'm', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                    params![made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
                )?;
                Ok(())
            })
            .unwrap();
        attempt
    }

    /// Driver step A8 for `attempt`, in this process, as `attempt run` runs it.
    fn merge(&self, attempt: &AttemptId) -> Result<End, CpdError> {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
        let catalog = Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } };
        let ctx = Ctx { home: &self.home, config: &config, catalog: &catalog, source: "attempt run", metrics: &|_| None };
        merge::merge(&mut self.store(), &ctx, &plan(), attempt).map(|m| m.end)
    }

    fn errors(&self) -> Vec<Value> {
        self.env.cpd(&["events", "list", "--kind", "error"]).ok()["events"].as_array().unwrap().clone()
    }
}

const CHILD_ATTEMPT: &str = "CPD_ADOPT_LIFE_ATTEMPT";

/// Step A8 for the Attempt named by `CPD_ADOPT_LIFE_ATTEMPT`, run by [`merge_in_child`] with the env's `PATH` (the fake `gh` first)
/// and `CPD_HOME`: the test process's own environment is not changed (`set_var` races the other tests). Prints `END <debug of the end>`.
#[test]
#[ignore = "run by merge_in_child, in a child process"]
fn child_merges_the_attempt() {
    let home = CpdHome::resolve(Some(std::env::var_os("CPD_HOME").expect("CPD_HOME").into()), None).unwrap();
    let attempt = AttemptId::parse(&std::env::var(CHILD_ATTEMPT).expect(CHILD_ATTEMPT)).unwrap();
    let config = Config::load(&home).unwrap();
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
    let catalog = Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } };
    let ctx = Ctx { home: &home, config: &config, catalog: &catalog, source: "attempt run", metrics: &|_| None };
    let end = merge::merge(&mut Store::open(&home, 5000).unwrap(), &ctx, &plan(), &attempt).map(|m| m.end);
    println!("END {end:?}");
}

/// Step A8 for `attempt`, run by this test binary again as a child: the `Debug` of its end.
fn merge_in_child(fx: &Fx, attempt: &AttemptId) -> String {
    let exe = std::env::current_exe().unwrap();
    let out = fx
        .env
        .command(exe)
        .args(["workflow_adopt_life::child_merges_the_attempt", "--exact", "--ignored", "--nocapture", "--test-threads", "1"])
        .env(CHILD_ATTEMPT, attempt.to_string())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the child merge failed: {stdout}{}", String::from_utf8_lossy(&out.stderr));
    stdout.split_once("END ").unwrap_or_else(|| panic!("no END: {stdout}")).1.lines().next().unwrap().to_owned()
}

#[test]
fn start_cuts_the_lead_branch_from_the_remote_head_and_pushes_nothing() {
    let fx = fx(false);

    let run = fx.start();

    assert_eq!(fx.remote_cpd_branches(), "", "no pay/* branch ever appears on origin");
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", "feat/x"]), "", "the User's own local branch is never made");
    run.ok();
    assert_eq!(fx.lead_sha(), fx.head, "the Lead Branch is origin/feat/x's commit");
    assert_eq!(fx.git(&fx.lead(), &["symbolic-ref", "--short", "HEAD"]), "pay/web");
    assert_eq!(fx.git(&fx.lead(), &["rev-parse", "HEAD"]), fx.head);
}

#[test]
fn start_does_not_refuse_a_head_that_conflicts_with_the_base() {
    let fx = fx(true);

    let run = fx.start();

    assert_eq!(run.exit, 0, "{}", run.error());
    assert_eq!(fx.lead_sha(), fx.head);
    assert_eq!(fx.remote_cpd_branches(), "");
}

#[test]
fn a_work_job_merge_pushes_nothing_for_an_adopted_pr() {
    let fx = fx(false);
    fx.start().ok();
    let attempt = fx.submitted("work");

    let end = fx.merge(&attempt);

    let Ok(End::Merged { sha }) = end else { panic!("{end:?}") };
    assert_ne!(fx.lead_sha(), fx.head, "the Lead Branch moved");
    assert_eq!(fx.lead_sha(), sha);
    assert_eq!(fx.remote_head(), fx.head, "origin/feat/x did not move");
    assert_eq!(fx.remote_cpd_branches(), "");
    assert!(fx.errors().iter().all(|e| e["data"]["code"] != "push_failed"), "{:?}", fx.errors());
}

#[test]
fn an_update_jobs_merge_pushes_to_the_head_branch() {
    let fx = fx(false);
    fx.start().ok();
    fx.open();
    let attempt = fx.submitted("update");

    let end = fx.merge(&attempt);

    let Ok(End::Merged { sha }) = end else { panic!("{end:?}") };
    assert_eq!(fx.lead_sha(), sha, "the Lead Branch fast-forwarded");
    assert_ne!(sha, fx.head);
    assert_eq!(fx.remote_head(), sha, "origin/feat/x is the new tip");
    assert_eq!(fx.remote_cpd_branches(), "", "origin still has no pay/* branch");
    assert!(fx.errors().iter().all(|e| e["data"]["code"] != "push_failed"), "{:?}", fx.errors());
}

#[test]
fn a_pull_request_merged_on_github_is_found_by_the_head_branch() {
    let fx = fx(false);
    fx.start().ok();
    fs::write(fx.env.root().join("list.json"), json!([{"number": 7, "url": URL, "state": "MERGED", "createdAt": "2099-01-01T00:00:00Z"}]).to_string()).unwrap();
    let node = json!({"repository": {"nameWithOwner": "acme/pay"}, "number": 7, "url": URL, "title": "Polish", "isDraft": false, "state": "MERGED",
        "createdAt": "opened","updatedAt":"U1", "mergedAt": "2026-10-14T10:00:00Z", "reviewDecision": null, "mergeable": "UNKNOWN", "mergeStateStatus": "UNKNOWN",
        "viewerDidAuthor": true, "viewerLatestReviewRequest": null, "commits": {"nodes": [{"commit": {"statusCheckRollup": null}}]}});
    fs::write(fx.env.root().join("lookup.json"), json!({"data": {"p0": {"pullRequest": node}}}).to_string()).unwrap();
    let attempt = fx.submitted("open");
    let end = merge_in_child(&fx, &attempt);

    let lists: Vec<Vec<String>> = fx.env.calls("gh").into_iter().filter(|c| c[..2] == ["pr", "list"]).collect();
    assert!(!lists.is_empty() && lists.iter().all(|c| c[3] == "feat/x"), "the lookup names the head branch: {lists:?}");
    assert!(end.starts_with(&format!("Ok(Merged {{ sha: \"{}\"", fx.head)), "adopted as merged, nothing pushed: {end}");
    assert_eq!(fx.remote_cpd_branches(), "");
}
