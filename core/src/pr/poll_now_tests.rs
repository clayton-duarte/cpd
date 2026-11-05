//! `pr merge` and `pr rerun` ask E5 for a poll at once (decision pro-d394, assumption A6): each writes the run file
//! `run/pr-poll-now` after its successful GitHub write, and not after a failed one. In process on the janitor's `Rig`, with `gh`
//! scripted on the runner; no process, no sleep. (The other commands are in `cli/tests/workflow/pr_poll_now.rs`.)

use std::path::PathBuf;

use clap::Parser;

use super::merge::MergeArgs;
use super::{Args, merge, rerun};
use crate::args::{PlanArg, WorkflowArg};
use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::janitor_tests::Rig;
use crate::ports::Ports;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// The `gh` of both commands. `write_ok` is whether the GitHub write (`pr merge`, `run rerun`) succeeds; every read answers.
struct Gh {
    write_ok: bool,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        let write = |ok: bool| {
            if ok {
                Reply::ok("")
            } else {
                Reply::exit(1, "", "gh: the write was refused\n")
            }
        };
        match args[..] {
            ["pr", "view", _, "--repo", _, ..] => Reply::ok(&format!(
                r#"{{"state":"OPEN","mergeStateStatus":"CLEAN","headRefOid":"{SHA}","title":"T"}}"#
            )),
            ["pr", "view", _, "--json", "headRefOid"] => {
                Reply::ok(&format!(r#"{{"headRefOid":"{SHA}"}}"#))
            }
            ["pr", "merge", ..] => write(self.write_ok),
            ["repo", "view", _, "--json", "squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed"] => {
                Reply::ok(r#"{"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false}"#)
            }
            ["repo", "view", ..] => Reply::ok(r#"{"deleteBranchOnMerge":true}"#),
            ["run", "list", ..] => Reply::ok(
                r#"[{"databaseId":11,"name":"ci","status":"completed","conclusion":"failure"}]"#,
            ),
            ["run", "rerun", ..] => write(self.write_ok),
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A rig with the open Workflow `api` of Plan `pay` (pull request #801), `gh` as [`Gh`], `$CPD_HOME` the rig's; the run file's path.
fn rig(write_ok: bool) -> (Rig, PathBuf) {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", std::sync::Arc::new(Gh { write_ok }));
    // `Env::open_with` and `pr merge` read `$CPD_HOME`: the one variable this test sets, to its own temp dir (as `poll_threads_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let file = rig.home.run_dir().join("pr-poll-now");
    (rig, file)
}

/// `MergeArgs` as clap parses them: its pull request is private to `merge`.
#[derive(Parser)]
struct Merge {
    #[command(flatten)]
    args: MergeArgs,
}

fn merge_it(rig: &Rig) -> bool {
    let args = Merge::try_parse_from(["merge", "acme/pay#801"]).unwrap().args;
    Ports::scope(&rig.fakes.ports, || merge::merge(&rig.fakes.ports, args)).is_ok()
}

fn rerun_it(rig: &Rig) -> bool {
    let args = Args {
        workflow: WorkflowArg {
            workflow: Some(WorkflowKey::parse("api").unwrap()),
        },
        plan: PlanArg {
            plan: Some(PlanId::parse("pay").unwrap()),
        },
    };
    Ports::scope(&rig.fakes.ports, || rerun::rerun(&rig.fakes.ports, args)).is_ok()
}

#[test]
fn pr_commands_write_poll_now_file_after_github_write_merge() {
    let (rig, file) = rig(true);
    assert!(!file.exists());
    assert!(merge_it(&rig), "the merge succeeded");
    assert!(file.exists(), "a merge on GitHub asks for a poll at once");
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_merge() {
    let (rig, file) = rig(false);
    assert!(!merge_it(&rig), "the merge failed");
    assert!(!file.exists(), "nothing changed on GitHub");
}

#[test]
fn pr_commands_write_poll_now_file_after_github_write_rerun() {
    let (rig, file) = rig(true);
    assert!(rerun_it(&rig), "the rerun succeeded");
    assert!(file.exists(), "a rerun on GitHub asks for a poll at once");
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_rerun() {
    let (rig, file) = rig(false);
    assert!(!rerun_it(&rig), "the rerun failed");
    assert!(!file.exists(), "nothing changed on GitHub");
}
