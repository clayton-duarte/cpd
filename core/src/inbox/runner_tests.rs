//! `inbox::team::github_lookup_with` on a [`FakeRunner`]: the `gh` call it makes and how it maps the answer.

use std::io::ErrorKind;
use std::time::Duration;

use serde_json::json;

use super::team::{GhState, github_lookup_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};

fn home() -> CpdHome {
    CpdHome::resolve(Some("/cpd-home".into()), None).unwrap()
}

fn refs() -> Vec<(String, u64)> {
    vec![("acme/api".to_owned(), 7)]
}

fn fake(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on("gh", |_| true, reply)
}

fn answer() -> String {
    json!({"data": {
        "viewer": {"login": "me"},
        "p0": {"pullRequest": {
            "title": "Add retries", "isDraft": false, "state": "OPEN", "reviewDecision": "APPROVED", "author": {"login": "bob"},
            "latestReviews": {"nodes": [{"state": "APPROVED", "author": {"login": "amy"}}]}
        }}
    }})
    .to_string()
}

#[test]
fn github_lookup_with_asks_the_runner_for_gh_api_graphql_with_the_query() {
    let runner = fake(Reply::ok(&answer()));
    github_lookup_with(&runner, &home(), &refs()).unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, "gh");
    assert_eq!(calls[0].program, "gh");
    assert_eq!(&calls[0].args[..3], ["api", "graphql", "-f"]);
    assert_eq!(calls[0].args.len(), 4);
    assert!(calls[0].args[3].starts_with("query=query { viewer { login }"), "{}", calls[0].args[3]);
    assert!(calls[0].args[3].contains("p0: repository(owner: \"acme\", name: \"api\") { pullRequest(number: 7)"), "{}", calls[0].args[3]);
}

#[test]
fn github_lookup_with_gives_the_call_fifteen_seconds_and_the_homes_root_as_its_directory() {
    let runner = fake(Reply::ok(&answer()));
    github_lookup_with(&runner, &home(), &refs()).unwrap();
    let call = &runner.calls()[0];
    assert_eq!(call.deadline, Duration::from_secs(15));
    assert_eq!(call.cwd.as_deref(), Some(home().root()));
}

#[test]
fn github_lookup_with_turns_off_gh_prompts_and_colour() {
    let runner = fake(Reply::ok(&answer()));
    github_lookup_with(&runner, &home(), &refs()).unwrap();
    let envs = runner.calls()[0].envs.clone();
    assert!(envs.contains(&("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned()))), "{envs:?}");
    assert!(envs.contains(&("NO_COLOR".to_owned(), Some("1".to_owned()))), "{envs:?}");
}

#[test]
fn github_lookup_with_parses_a_scripted_success() {
    let runner = fake(Reply::ok(&answer()));
    let lookup = github_lookup_with(&runner, &home(), &refs()).unwrap();
    assert_eq!(lookup.me, "me");
    let pr = lookup.prs[&("acme/api".to_owned(), 7)].clone().unwrap();
    assert_eq!(pr.state, GhState::Open);
    assert_eq!(pr.author, "bob");
    assert_eq!(pr.decision.as_deref(), Some("APPROVED"));
    assert_eq!(pr.reviews.len(), 1);
}

#[test]
fn github_lookup_with_lets_stdout_decide_when_gh_exits_one_with_data() {
    let runner = fake(Reply::exit(1, &answer(), "GraphQL: Could not resolve"));
    let lookup = github_lookup_with(&runner, &home(), &refs()).unwrap();
    assert_eq!(lookup.me, "me");
}

#[test]
fn github_lookup_with_maps_stdout_that_is_not_json_to_gh_failed_with_the_stderr() {
    let runner = fake(Reply::exit(4, "not json", "gh: To get started with GitHub CLI, please run: gh auth login"));
    let err = github_lookup_with(&runner, &home(), &refs()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GhFailed);
    assert!(err.message().starts_with("gh api graphql printed unexpected JSON"), "{}", err.message());
    assert!(err.details()["stderr"].as_str().unwrap().contains("gh auth login"));
}

#[test]
fn github_lookup_with_maps_a_gh_that_cannot_start_to_gh_failed() {
    let runner = fake(Reply::io(ErrorKind::NotFound));
    let err = github_lookup_with(&runner, &home(), &refs()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GhFailed);
    assert!(err.message().starts_with("Cannot run gh: "), "{}", err.message());
    assert!(!err.details()["stderr"].as_str().unwrap().is_empty());
}

#[test]
fn github_lookup_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("gh", |_| true, Reply::ok(&answer()).takes(Duration::from_secs(20)));
    let err = github_lookup_with(&runner, &home(), &refs()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], "gh");
    assert_eq!(err.details()["deadline_seconds"], 15);
    assert_eq!(clock.elapsed(), Duration::from_secs(15));
}
