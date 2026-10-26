//! The clean-up `pr poll` runs when it sees a PR cancelled (commands.md §9.2 "After a PR is merged or cancelled",
//! §9.6; hs-d517): the same fixtures as pr_cleanup.rs (in-process, on `run_with` over the fake ports), with the PR then
//! cancelled. A cancelled PR's row has no lead_worktree, so its paths come from contract §7.1's naming rule.

use crate::pr_cleanup::{Fx, fx, node, search_answer};

use std::fs;

use serde_json::{Value, json};

impl Fx {
    /// The pull requests GitHub reports: `open` ones in the search, `merged` ones in the lookup. The direct read names
    /// every stored open PR the search missed, in id order: a merged one answers MERGED, the others (a cancelled PR's
    /// row is still stored `open` and authored) answer OPEN.
    fn github_cancelled(&self, open: &[&str], merged: &[&str]) {
        let number = |key: &str| self.prs.iter().find(|p| p.key == key).unwrap().number;
        let nodes: Vec<String> = open.iter().map(|k| node(number(k), "OPEN")).collect();
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
        self.answer(&search_answer(&nodes), &format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")));
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
    fx.github_cancelled(&["web"], &[]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.gone("web"), (false, false), "another PR keeps everything");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn cancelled_open_pr_worktrees_are_removed() {
    let fx = fx(&["api", "web"]);
    cancel_in_db(&fx, "api");
    fx.github_cancelled(&["web"], &[]);

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
    fx.github_cancelled(&[], &[]);

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
