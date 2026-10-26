//! `--no-mcp` in every Session's pi argv (contract §6.6 argv table, §6.11; pro-d3, pro-d7): pi then loads no built-in MCP extension, so no `mcp.json` reaches a Session.
//! The one-shot runs (`--no-extensions`) have no MCP already and are unchanged.

use std::num::NonZeroU32;
use std::path::Path;

use super::*;
use crate::ids::JobKey;

const ID: &str = "596d007f-1111-4222-8333-444455556666";
const NO_MCP: &str = "--no-mcp";

fn home() -> CpdHome {
    CpdHome::resolve(Some("/cpd-home".into()), None).unwrap()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn lead_argv(plan: Option<&str>, start: LeadStart) -> Vec<String> {
    let home = home();
    let session = SessionId::parse(ID).unwrap();
    let plan = plan.map(|p| PlanId::parse(p).unwrap());
    let extensions = strs(&["/opt/cpd/pi", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"]);
    let plannotator = Plannotator { port: "19432-19456".into(), url_host: "192.168.3.3".into() };
    lead(&LeadLaunch { home: &home, session: &session, plan: plan.as_ref(), extensions: &extensions, plannotator: &plannotator, start }).argv
}

fn worker_argv(fork: Option<&Path>) -> Vec<String> {
    let home = home();
    let session = SessionId::parse(ID).unwrap();
    let plan = PlanId::parse("cpd2").unwrap();
    let attempt = AttemptId::new(JobKey::parse("rs-host").unwrap(), NonZeroU32::new(2).unwrap());
    let extensions = strs(&["/opt/cpd/pi"]);
    worker(&WorkerLaunch { home: &home, session: &session, plan: &plan, attempt: &attempt, model: "p/m:high", extensions: &extensions, fork }).argv
}

/// `--no-mcp` is in `argv` exactly once, and before the `--` that ends the options when there is one; the rest of `argv` is `rest`, in order: nothing else was added or removed.
fn assert_no_mcp_once(argv: &[String], rest: &[String]) {
    let at: Vec<usize> = argv.iter().enumerate().filter(|(_, a)| a.as_str() == NO_MCP).map(|(i, _)| i).collect();
    assert_eq!(at.len(), 1, "`--no-mcp` exactly once in {argv:?}");
    if let Some(end) = argv.iter().position(|a| a == "--") {
        assert!(at[0] < end, "`--no-mcp` before the `--` that ends the options in {argv:?}");
    }
    let without: Vec<String> = argv.iter().filter(|a| a.as_str() != NO_MCP).cloned().collect();
    assert_eq!(without, rest, "every other flag is as before");
}

fn fresh(first_message: Option<&str>) -> LeadStart {
    LeadStart::Fresh { model: "m/x:high".into(), name: "pay-4521".into(), first_message: first_message.map(Into::into) }
}

fn fresh_rest() -> Vec<String> {
    strs(&[
        "pi", "--model", "m/x:high", "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
        "--session-id", ID, "-n", "pay-4521", "--", "@/cpd-home/run/cpd2/lead-message.md",
    ])
}

fn resume() -> LeadStart {
    LeadStart::Resume { model: "p/m:high".into(), file: "/s/x.jsonl".into() }
}

fn resume_rest() -> Vec<String> {
    strs(&["pi", "--model", "p/m:high", "--session", "/s/x.jsonl", "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"])
}

#[test]
fn a_fresh_lead_has_no_mcp_once_before_the_double_dash() {
    assert_no_mcp_once(&lead_argv(Some("cpd2"), fresh(Some("/cpd-home/run/cpd2/lead-message.md"))), &fresh_rest());
}

#[test]
fn a_resumed_lead_has_no_mcp_once() {
    assert_no_mcp_once(&lead_argv(Some("cpd2"), resume()), &resume_rest());
}

#[test]
fn a_fresh_manager_has_no_mcp_once_before_the_double_dash() {
    assert_no_mcp_once(&lead_argv(None, fresh(Some("/cpd-home/run/cpd2/lead-message.md"))), &fresh_rest());
}

#[test]
fn a_resumed_manager_has_no_mcp_once() {
    assert_no_mcp_once(&lead_argv(None, resume()), &resume_rest());
}

#[test]
fn a_side_fork_has_no_mcp_once_with_and_without_a_first_message() {
    let fork = |first_message: Option<&str>| LeadStart::Fork {
        model: "m/x:high".into(),
        name: "pay-4521".into(),
        source_file: "/s/2026_6f1c.jsonl".into(),
        first_message: first_message.map(Into::into),
    };
    let base = ["pi", "--model", "m/x:high", "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8", "--fork", "/s/2026_6f1c.jsonl", "--session-id", ID, "-n", "pay-4521"];
    let with_message = [&base[..], &["--", "@/cpd-home/run/cpd2/messages/x.md"]].concat();
    assert_no_mcp_once(&lead_argv(Some("cpd2"), fork(Some("/cpd-home/run/cpd2/messages/x.md"))), &strs(&with_message));
    assert_no_mcp_once(&lead_argv(Some("cpd2"), fork(None)), &strs(&base));
}

fn worker_rest(fork: &[&str]) -> Vec<String> {
    let mut rest: Vec<String> = crate::host::priority::idle_prefix().iter().map(|s| (*s).to_owned()).collect();
    rest.extend(strs(&["pi", "--approve", "--no-skills", "--exclude-tools", WORKER_EXCLUDED_TOOLS, "-e", "/opt/cpd/pi", "--model", "p/m:high"]));
    rest.extend(strs(fork));
    rest.extend(strs(&[
        "--session-id", ID, "-n", "cpd2 rs-host a2", "--",
        "Read /cpd-home/run/cpd2/specs/rs-host-a2.md and follow it exactly. Your attempt id is rs-host-a2.",
    ]));
    rest
}

#[test]
fn a_worker_has_no_mcp_once_before_the_double_dash() {
    assert_no_mcp_once(&worker_argv(None), &worker_rest(&[]));
}

#[test]
fn a_review_worker_has_no_mcp_once_before_the_double_dash() {
    assert_no_mcp_once(&worker_argv(Some(Path::new("/s/lead.jsonl"))), &worker_rest(&["--fork", "/s/lead.jsonl"]));
}

#[test]
fn the_distiller_has_no_mcp_flag_and_is_unchanged() {
    let argv = super::distiller("p/sonnet:medium", &["prompt.md", "records.md"]).argv;
    assert!(!argv.iter().any(|a| a == NO_MCP), "a one-shot run with --no-extensions has no MCP already: {argv:?}");
    assert_eq!(
        argv,
        strs(&[
            "pi", "--mode", "json", "--no-session", "--no-extensions", "--no-skills", "--no-tools",
            "--model", "p/sonnet:medium", "--", "@prompt.md", "@records.md",
        ])
    );
}

/// The title writer's argv is built inline in `session/retitle.rs` (no seam), so this reads its source: its one `pi` call is `--no-extensions` and has no `--no-mcp`.
#[test]
fn the_title_writer_has_no_mcp_flag_and_keeps_no_extensions() {
    let source = include_str!("../../session/retitle.rs");
    let args = source.lines().find(|l| l.contains("args: [\"--print\"")).expect("the title writer's argv line");
    assert!(args.contains("\"--no-extensions\""), "{args}");
    assert!(!args.contains(NO_MCP), "{args}");
}
