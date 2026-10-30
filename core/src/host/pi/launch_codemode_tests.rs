//! `--tools +codemode` in every Session's pi argv (contract §6.6; hs-d736): right after `--no-mcp`, so before the first `-e`, exactly once, before the `--` that ends the options.
//! The literals are the contract; no constant from launch.rs is used.

use std::num::NonZeroU32;

use super::*;
use crate::ids::JobKey;

const ID: &str = "596d007f-1111-4222-8333-444455556666";

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

fn lead_argv(plan: Option<&str>, start: LeadStart) -> Vec<String> {
    let home = CpdHome::resolve(Some("/cpd-home".into()), None).unwrap();
    let session = SessionId::parse(ID).unwrap();
    let plan = plan.map(|p| PlanId::parse(p).unwrap());
    let extensions = strs(&["/opt/cpd/pi", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"]);
    let plannotator = Plannotator { port: "19432-19456".into(), url_host: "192.168.3.3".into() };
    lead(&LeadLaunch { home: &home, session: &session, plan: plan.as_ref(), extensions: &extensions, plannotator: &plannotator, start }).argv
}

fn worker_argv() -> Vec<String> {
    let home = CpdHome::resolve(Some("/cpd-home".into()), None).unwrap();
    let session = SessionId::parse(ID).unwrap();
    let plan = PlanId::parse("cpd2").unwrap();
    let attempt = AttemptId::new(JobKey::parse("rs-host").unwrap(), NonZeroU32::new(2).unwrap());
    let extensions = strs(&["/opt/cpd/pi"]);
    worker(&WorkerLaunch { home: &home, session: &session, plan: &plan, attempt: &attempt, model: "p/m:high", extensions: &extensions }).argv
}

/// `--tools`, `+codemode` are the two elements right after `--no-mcp`, exactly once, before the first `-e` and before any `--`.
fn assert_tools_codemode_after_no_mcp(argv: &[String]) {
    let tools: Vec<usize> = argv.iter().enumerate().filter(|(_, a)| a.as_str() == "--tools").map(|(i, _)| i).collect();
    assert_eq!(tools.len(), 1, "`--tools` exactly once in {argv:?}");
    let codemode = argv.iter().filter(|a| a.as_str() == "+codemode").count();
    assert_eq!(codemode, 1, "`+codemode` exactly once in {argv:?}");
    let no_mcp = argv.iter().position(|a| a == "--no-mcp").expect("`--no-mcp` in argv");
    assert_eq!(tools[0], no_mcp + 1, "`--tools` right after `--no-mcp` in {argv:?}");
    assert_eq!(argv.get(no_mcp + 2).map(String::as_str), Some("+codemode"), "`+codemode` right after `--tools` in {argv:?}");
    let first_e = argv.iter().position(|a| a == "-e").expect("`-e` in argv");
    assert!(no_mcp + 2 < first_e, "`--tools +codemode` before the first `-e` in {argv:?}");
    if let Some(end) = argv.iter().position(|a| a == "--") {
        assert!(no_mcp + 2 < end, "`--tools +codemode` before the `--` that ends the options in {argv:?}");
    }
}

fn fresh() -> LeadStart {
    LeadStart::Fresh { model: "m/x:high".into(), name: "pay-4521".into(), first_message: Some("/cpd-home/run/cpd2/lead-message.md".into()) }
}

fn resume() -> LeadStart {
    LeadStart::Resume { model: "p/m:high".into(), file: "/s/x.jsonl".into() }
}

#[test]
fn a_fresh_lead_has_tools_plus_codemode_right_after_no_mcp() {
    assert_tools_codemode_after_no_mcp(&lead_argv(Some("cpd2"), fresh()));
}

#[test]
fn a_resumed_lead_has_tools_plus_codemode_right_after_no_mcp() {
    assert_tools_codemode_after_no_mcp(&lead_argv(Some("cpd2"), resume()));
}

#[test]
fn a_fresh_manager_has_tools_plus_codemode_right_after_no_mcp() {
    assert_tools_codemode_after_no_mcp(&lead_argv(None, fresh()));
}

#[test]
fn a_resumed_manager_has_tools_plus_codemode_right_after_no_mcp() {
    assert_tools_codemode_after_no_mcp(&lead_argv(None, resume()));
}

#[test]
fn a_side_fork_has_tools_plus_codemode_right_after_no_mcp() {
    let fork = |first_message: Option<&str>| LeadStart::Fork {
        model: "m/x:high".into(),
        name: "pay-4521".into(),
        source_file: "/s/2026_6f1c.jsonl".into(),
        first_message: first_message.map(Into::into),
    };
    assert_tools_codemode_after_no_mcp(&lead_argv(Some("cpd2"), fork(Some("/cpd-home/run/cpd2/messages/x.md"))));
    assert_tools_codemode_after_no_mcp(&lead_argv(Some("cpd2"), fork(None)));
}

#[test]
fn a_worker_has_tools_plus_codemode_right_after_no_mcp_and_keeps_its_excluded_tools() {
    let argv = worker_argv();
    assert_tools_codemode_after_no_mcp(&argv);
    let at = argv.iter().position(|a| a == "--exclude-tools").expect("`--exclude-tools` in argv");
    assert_eq!(argv[at + 1], WORKER_EXCLUDED_TOOLS, "the Worker keeps its excluded tools");
}
