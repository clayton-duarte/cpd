//! The catch-up of an adopted PR (commands.md §9.3, contract §7.6): `pr rebase` never rebases onto the base; it
//! rebases the Lead Branch's commits that `origin/<head_ref>` lacks onto the moved remote head, then pushes plain
//! (no lease, no force). The built binary against a temporary repo with a bare `origin` whose `feat/x` is the pull
//! request's head (#7) and which refuses non-fast-forward pushes, so a forced push would fail; a fake `gh`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Run, TestEnv};
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/7";

/// The fake `gh` of `pr add --adopt`: `pr view` prints `view.json`, `api user` prints `alice`.
const GH: &str = r#"
root="ROOT"
case "$1 $2" in
  "pr view") cat "$root/view.json" ;;
  "api user") echo alice ;;
  "pr list") echo '[]' ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
}

/// Repo `pay` with its bare `origin` (`denyNonFastForwards`) holding `main` and `feat/x` (one commit past `main`); PR `web`
/// of Plan `pay` adopts #7 and is in `status`, its Lead worktree on `pay/web` cut from `origin/feat/x`.
fn fx(status: &str) -> Fx {
    let env = TestEnv::new();
    let config = format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    env.fake_gh(&GH.replace("ROOT", &env.root().display().to_string()));
    fs::write(
        env.root().join("view.json"),
        json!({"number": 7, "url": URL, "state": "OPEN", "isCrossRepository": false, "headRefName": "feat/x", "baseRefName": "main", "author": {"login": "alice"}}).to_string(),
    )
    .unwrap();

    let repo = env.keyed_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("origins/pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(&origin, &["config", "receive.denyNonFastForwards", "true"]);
    env.local_origin(&repo, &origin);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    fs::write(repo.join("head.txt"), "the head\n").unwrap();
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "the head"]);
    env.git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/feat/x"]);
    env.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);

    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    env.cpd(&["pr", "add", "web", "--repo", repo.to_str().unwrap(), "--title", "Polish", "--shape", "config", "--adopt", "7", "--plan", "pay"]).ok();
    env.cpd(&["job", "cancel", "web-config", "--reason", "a stub of the template", "--plan", "pay"]).ok();

    // The start of the PR, as `pr start` leaves it: the Lead worktree on the local branch, cut from the remote head.
    let lead = repo.parent().unwrap().join("pay.pay.web");
    env.git(&repo, &["worktree", "add", "-q", "--no-track", "-b", "pay/web", lead.to_str().unwrap(), "origin/feat/x"]);
    env.git(&lead, &["config", "user.name", "cpd test"]);
    env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    let opened = if status == "open" {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
               VALUES ('acme/pay', 7, '{URL}', 'T', 0, 'open', 'none', 'none', 't', 't');
             UPDATE prs SET status = 'open', pr_number = 7, pr_url = '{URL}', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/pay') WHERE plan = 'pay' AND key = 'web';"
        )
    } else {
        String::new()
    };
    env.sql(&format!(
        "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE prs SET status = 'dispatch', lead_worktree = '{}', started_at = 't' WHERE plan = 'pay' AND key = 'web';
         {opened}",
        lead.display()
    ));
    Fx { env, repo, origin, lead }
}

impl Fx {
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn rebase(&self, flag: &[&str]) -> Run {
        let mut args = vec!["pr", "rebase", "--pr", "web", "--plan", "pay"];
        args.extend_from_slice(flag);
        self.env.cpd(&args)
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/pay/web"])
    }

    fn remote_head(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/feat/x"])
    }

    fn remote_main(&self) -> String {
        self.git(&self.origin, &["rev-parse", "refs/heads/main"])
    }

    /// Commits `name` (with `content`) on the Lead Branch; returns the new tip.
    fn commit(&self, name: &str, content: &str) -> String {
        fs::write(self.lead.join(name), content).unwrap();
        self.git(&self.lead, &["add", "-A"]);
        self.git(&self.lead, &["commit", "-q", "-m", name]);
        self.lead_sha()
    }

    /// Someone else commits `name` (with `content`) to `branch` of `origin`, through a second clone the repo has not fetched
    /// from. Returns the new tip.
    fn push_by_someone(&self, branch: &str, name: &str, content: &str) -> String {
        let clone = self.env.root().join(format!("clone-{}", branch.replace('/', "-")));
        self.git(self.env.root(), &["clone", "-q", self.origin.to_str().unwrap(), clone.to_str().unwrap()]);
        self.git(&clone, &["config", "user.name", "someone"]);
        self.git(&clone, &["config", "user.email", "someone@cpd.invalid"]);
        self.git(&clone, &["checkout", "-q", branch]);
        fs::write(clone.join(name), content).unwrap();
        self.git(&clone, &["add", "-A"]);
        self.git(&clone, &["commit", "-q", "-m", name]);
        self.git(&clone, &["push", "-q", "origin", branch]);
        self.git(&clone, &["rev-parse", "HEAD"])
    }

    fn is_ancestor(&self, ancestor: &str, of: &str) -> bool {
        self.git(&self.repo, &["merge-base", ancestor, of]) == self.git(&self.repo, &["rev-parse", ancestor])
    }

    fn rebasing(&self) -> bool {
        ["rebase-merge", "rebase-apply"].iter().any(|name| {
            let path = self.git(&self.lead, &["rev-parse", "--path-format=absolute", "--git-path", name]);
            Path::new(&path).exists()
        })
    }
}

fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

#[test]
fn rebase_puts_the_unpushed_commits_on_the_moved_remote_head_and_pushes_plain() {
    let fx = fx("open");
    let (a, b) = (fx.commit("a.txt", "a\n"), fx.commit("b.txt", "b\n"));
    let moved = fx.push_by_someone("feat/x", "remote.txt", "remote\n");

    let out: Value = fx.rebase(&[]).ok();

    let new = fx.lead_sha();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "web", "rebased": true, "old": b, "new": new, "pushed": true}));
    assert!(fx.is_ancestor(&moved, &new), "the Lead Branch holds the moved remote head");
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--count", &format!("{moved}..{new}")]), "2", "plus the PR's own two commits");
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--merges", &new]), "", "linear");
    assert!(!fx.is_ancestor(&a, &new) && !fx.is_ancestor(&b, &new), "the old commits are gone");
    assert_eq!(fx.git(&fx.repo, &["show", &format!("{new}:a.txt")]), "a");
    assert_eq!(fx.git(&fx.repo, &["show", &format!("{new}:b.txt")]), "b");
    assert_eq!(fx.git(&fx.repo, &["show", &format!("{new}:remote.txt")]), "remote");
    assert_eq!(fx.remote_head(), new, "pushed plain, under denyNonFastForwards");
    assert_eq!(fx.git(&fx.origin, &["branch", "--list", "pay/*"]), "", "no pay/* branch on origin");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
}

#[test]
fn rebase_never_rebases_onto_the_base() {
    let fx = fx("open");
    fx.commit("a.txt", "a\n");
    let moved = fx.push_by_someone("feat/x", "remote.txt", "remote\n");
    let base = fx.push_by_someone("main", "base.txt", "base\n");
    assert_ne!(fx.remote_main(), "");

    fx.rebase(&[]).ok();

    let new = fx.lead_sha();
    assert!(fx.is_ancestor(&moved, &new), "it caught up with the head");
    assert!(!fx.is_ancestor(&base, &new), "the new main commit is not in the Lead Branch");
    assert_eq!(fx.remote_head(), new);
    assert_eq!(fx.remote_main(), base, "main untouched");
}

#[test]
fn rebase_fast_forwards_when_nothing_is_unpushed() {
    let fx = fx("open");
    let old = fx.lead_sha();
    let moved = fx.push_by_someone("feat/x", "remote.txt", "remote\n");

    let out = fx.rebase(&[]).ok();

    assert_eq!(fx.lead_sha(), moved, "the Lead Branch fast-forwarded to the remote tip");
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "web", "rebased": true, "old": old, "new": moved, "pushed": false}));
    assert_eq!(fx.remote_head(), moved, "nothing to push");
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), moved, "the Lead worktree followed");
}

#[test]
fn rebase_is_already_when_the_remote_head_is_in_the_lead_branch() {
    let fx = fx("open");
    let tip = fx.commit("a.txt", "a\n");
    let remote = fx.remote_head();
    assert_ne!(remote, tip);

    let out = fx.rebase(&[]).ok();

    assert_eq!(out["already"], true, "{out}");
    assert_eq!(fx.lead_sha(), tip, "nothing moves");
    assert_eq!(fx.remote_head(), remote, "nothing pushed");
    assert_eq!(fx.git(&fx.origin, &["branch", "--list", "pay/*"]), "");
}

#[test]
fn a_dispatch_pr_rebases_only_when_its_jobs_are_done() {
    let fx = fx("dispatch");
    let tip = fx.commit("a.txt", "a\n");
    let moved = fx.push_by_someone("feat/x", "remote.txt", "remote\n");
    fx.env.sql(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, kind, created_txn, created_at)
           VALUES ('pay', 'web', 'w', 'W', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'queued', 0, 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', 'work', cpd_txn(), 't');",
    );

    let run = fx.rebase(&[]);

    assert_eq!(code(&run), ("jobs_not_done".into(), 3));
    assert_eq!(fx.lead_sha(), tip, "nothing moved");
    assert_eq!(fx.remote_head(), moved, "nothing pushed");

    fx.env.sql("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'w'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND key = 'w';");
    let out = fx.rebase(&[]).ok();

    let new = fx.lead_sha();
    assert_eq!(out["rebased"], true, "{out}");
    assert!(fx.is_ancestor(&moved, &new));
    assert_eq!(fx.remote_head(), new);
}

#[test]
fn a_conflict_stops_and_continue_finishes_with_a_plain_push() {
    let fx = fx("open");
    let old = fx.commit("clash.txt", "mine\n");
    let moved = fx.push_by_someone("feat/x", "clash.txt", "theirs\n");

    let run = fx.rebase(&[]);

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["clash.txt"]));
    assert_eq!(run.error()["details"]["worktree"], fx.lead.to_str().unwrap());
    assert!(fx.rebasing(), "the rebase waits in the Lead worktree");
    assert_eq!(fx.lead_sha(), old);
    assert_eq!(fx.remote_head(), moved, "nothing pushed");

    fs::write(fx.lead.join("clash.txt"), "resolved\n").unwrap();
    fx.git(&fx.lead, &["add", "clash.txt"]);
    let out = fx.rebase(&["--continue"]).ok();

    let new = fx.lead_sha();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "web", "rebased": true, "old": old, "new": new, "pushed": true}));
    assert!(!fx.rebasing());
    assert!(fx.is_ancestor(&moved, &new));
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--merges", &new]), "");
    assert_eq!(fx.git(&fx.repo, &["show", &format!("{new}:clash.txt")]), "resolved");
    assert_eq!(fx.remote_head(), new, "pushed plain, under denyNonFastForwards");
}
