//! `pr poll` cleans nothing (commands.md §9.2 "After a PR is merged or cancelled", contract §7.2): the clean-up
//! of merged and cancelled PRs is `cpd janitor`'s (`janitor_tests.rs`). In process on the janitor's `Rig`: the git is
//! the in-process `Universe`, `gh` and tmux are scripted on the runner. No test starts a process or sleeps.

use std::sync::Arc;

use super::PollArgs;
use super::poll::poll;
use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;

/// The in-process `gh` of the poll: no open pull request in the search, `merged` pull requests in the lookup.
struct Gh {
    merged: Vec<i64>,
}

fn node(number: i64) -> String {
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"PR {number}",
        "isDraft":false,"state":"MERGED","createdAt":"opened","updatedAt":"U1","mergedAt":"2026-10-14T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => Reply::ok(
                r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#,
            ),
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => {
                let aliases: Vec<String> = self.merged.iter().enumerate().map(|(i, n)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(*n))).collect();
                Reply::ok(&format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

#[test]
fn pr_poll_removes_and_reports_nothing_of_a_merged_or_cancelled_pr() {
    let mut rig = Rig::new();
    // `api` is open and GitHub says it merged; `web` is cancelled already.
    rig.pr("api", "open");
    rig.pr("web", "cancelled");
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(Gh { merged: vec![801] }));

    // `Env::open_with` reads `$CPD_HOME`: the one variable this test sets, to its own temp dir (as `inbox/snooze_time_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || poll(&rig.fakes.ports, PollArgs { plan: PlanArg { plan: None } })).unwrap();

    let status: String = {
        let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
        store.read(|c| Ok(c.query_row("SELECT status FROM prs WHERE key = 'api'", [], |r| r.get(0))?)).unwrap()
    };
    assert_eq!(status, "merged", "the poll saw the merge: {out:?}");
    for pr in &rig.prs {
        assert_eq!(rig.left(pr), [true; 6], "{} PR {}: the poll leaves every worktree, registration and branch", pr.status, pr.key);
    }
    let kills: Vec<Call> = rig.fakes.runner.calls().into_iter().filter(|c| c.program.ends_with("tmux") && c.args.iter().any(|a| a.starts_with("kill-"))).collect();
    assert!(kills.is_empty(), "the poll kills no window: {kills:?}");
    let cleanup: Vec<_> = rig.errors().into_iter().filter(|(_, data)| data["code"] == "cleanup_failed").collect();
    assert_eq!(cleanup, Vec::new(), "the poll appends no cleanup_failed Event");
}
