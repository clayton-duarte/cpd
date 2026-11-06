//! E24 running the `jira` source (docs/architecture/swr-jira.md §8): a refusal parks the transition and raises one
//! plan-scoped `error` Event. The helper is the `node` route of a `FakeRunner`; the clock is a `ManualClock`; no process,
//! no Jira, no sleep.

use serde_json::{Value, json};

use super::swr;
use super::testkit::Fx;
use cpd_core::ports::Fakes;
use cpd_core::swr::outbox;
use cpd_core::swr::source::jira::{self, Jira};

const REFUSED: &str = r#"{"ok":false,"error":{"code":"jira_refused","message":"EEC-1 has no transition to \"In Review\" from \"In Progress\".","details":{"reason":"no_transition","tool":"getTransitionsForJiraIssue"}}}"#;

#[test]
fn jira_refused_parks_with_one_error_event() {
    let mut fx = Fx::with_plan("");
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    swr::set_sources_for_test(vec![&Jira]);
    fx.fixture("UPDATE workflows SET jira = 'EEC-1' WHERE plan = 'pay' AND key = 'repo';");
    let policy = fx.engine.home.root().join("policy");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(
        policy.join("host.toml"),
        "[mcp.servers.atlassian]\nurl = \"https://mcp.atlassian.com/v1/mcp/authv2\"\n",
    )
    .unwrap();
    (*fakes.runner).clone().on(
        "node",
        |_| true,
        cpd_core::host::runner::Reply::exit(1, REFUSED, ""),
    );

    let id = jira::enqueue_transition(
        &mut fx.engine.store,
        fakes.ports.clock.utc(),
        "EEC-1",
        "in_review",
        Some("In Review"),
        json!({"plan": "pay", "workflow": "repo"}),
    )
    .unwrap();

    swr::run(&mut fx.engine);
    swr::run(&mut fx.engine);

    let m = outbox::get(&mut fx.engine.store, id).unwrap().unwrap();
    assert_eq!(m.status, "failed", "a refusal is parked, never retried");
    assert!(m.error.as_deref().unwrap().contains("jira_refused"));
    let context: Value = serde_json::from_str(m.context_json.as_deref().unwrap()).unwrap();
    assert_eq!(context["refused_with"], "EEC-1|In Review");
    let errors = fx.events("error");
    assert_eq!(
        errors.len(),
        1,
        "one Event, and a second pass adds none: {errors:?}"
    );
    assert_eq!(errors[0].0.as_deref(), Some("pay"), "scoped to the Plan");
    assert_eq!(errors[0].3["code"], "jira_refused");
}
