//! On-demand Context retrieval: the first messages (header + brief + pointer), `context show --digest` gone, the
//! `context_recent_days` config key gone, and `context show --topic` kept (contract §4.6 "Consumers"; commands.md
//! §3.3, §4.1; design docs/research/context-retrieval-design.md "Digest removal", "First messages"; hs-d655). The
//! tests that start no pane run in this process (`Fx`), the others as the built binary. Curations are written through
//! `curation::propose`, as the distiller will.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::{SLEEP_SECONDS, TestEnv, WATCH_OK, refused};
use cpd_core::context::curation::{Curation, propose};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId};

pub(crate) const CTX: &str = "ctx";

/// Machine `m1`, Context `ctx` and a fake `cpd` for the panes (`plan new`).
fn fx() -> TestEnv {
    let env = TestEnv::new();
    env.write_tmux_config("");
    env.fake_cpd_panes(WATCH_OK);
    env.cpd(&["context", "new", CTX, "--title", "Payments", "--owner", "acme"]).ok();
    env
}

/// Machine `m1` and Context `ctx`, the command tree in this process: for the tests that start no pane.
pub(crate) fn fast() -> Fx {
    let fx = Fx::new();
    fx.env.write_tmux_config("");
    fx.cpd(&["context", "new", CTX, "--title", "Payments", "--owner", "acme"]).ok();
    fx
}

/// Writes a curation per `(record id, topic, short)` row, as the distiller will.
pub(crate) fn curate(env: &TestEnv, rows: &[(&str, &str, &str)]) {
    let curations: Vec<Curation> = rows
        .iter()
        .map(|(id, topic, short)| Curation { record: id.parse().unwrap(), topic: (*topic).into(), short: (*short).into(), doc_paths: None })
        .collect();
    propose(&mut env.store(), &ContextId::parse(CTX).unwrap(), &curations, &[]).unwrap();
}

fn lead_message(env: &TestEnv, plan: &str) -> String {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::read_to_string(home.lead_message(&PlanId::parse(plan).unwrap())).unwrap()
}

fn new_plan_with_brief(env: &TestEnv, id: &str, brief: &str) {
    let file = env.root().join(format!("{id}-brief.md"));
    fs::write(&file, brief).unwrap();
    env.cpd(&["plan", "new", id, "--context", CTX, "--title", &format!("Plan {id}"), "--brief-file", file.to_str().unwrap()]).ok();
}

/// A fake `pi` that creates its session file and sleeps; the cockpit is the one `plan new` made.
fn manager_cockpit(env: &TestEnv) {
    env.fake_cpd_views();
    env.fake_pi(&format!(
        "id=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = --session-id ] && id=$a; prev=$a; done\n\
         if [ -n \"$id\" ]; then mkdir -p \"$HOME/.pi/agent/sessions/g\"; \
         printf '{{\"type\":\"session\"}}\\n' > \"$HOME/.pi/agent/sessions/g/2026_$id.jsonl\"; fi\n\
         exec sleep {SLEEP_SECONDS}"
    ));
}

/// The pointer lines the design gives (docs/research/context-retrieval-design.md "First messages").
const LEAD_POINTER: &str = "Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>";
const MANAGER_POINTER: &str =
    "Context on demand: cpd context search <query> --context ctx · cpd context show --context ctx --since <seq> · cpd context show --context ctx --record <id>";

/// hs-d655: a Lead's first message is its header, its brief and the pointer line; no digest, no raw records.
#[test]
fn a_lead_first_message_is_header_brief_and_pointer() {
    let env = fx();
    env.cpd(&["context", "note", "FULL TEXT of the curated one.", "--context", CTX]).ok();
    curate(&env, &[("m1-n1", "ledger", "curated short form")]);
    new_plan_with_brief(&env, "pay", "Retry the failed payouts.\n");

    let message = lead_message(&env, "pay");
    assert_eq!(
        message,
        format!(
            "You are the Lead of Plan pay \"Plan pay\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\n\
             Retry the failed payouts.\n\n{}\n",
            LEAD_POINTER
        )
    );
    assert!(!message.contains("curated short form") && !message.contains("FULL TEXT"), "nothing from the store is pushed: {message}");
}

/// hs-d655: without a brief, a Lead's first message is its header and the pointer line alone.
#[test]
fn a_lead_without_a_brief_is_header_and_pointer() {
    let env = fx();
    env.cpd(&["plan", "new", "pay", "--context", CTX, "--title", "Pay"]).ok();

    let message = lead_message(&env, "pay");
    assert_eq!(
        message,
        format!("You are the Lead of Plan pay \"Pay\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\n{}\n", LEAD_POINTER)
    );
}

/// hs-d655: a Manager's first message is its header and the pointer line, named with the Context; it has no brief.
#[test]
fn a_manager_first_message_is_header_and_pointer() {
    let env = fx();
    env.cpd(&["context", "note", "Mind the ledger.", "--context", CTX]).ok();
    manager_cockpit(&env);
    env.cpd(&["context", "manager", "--context", CTX]).ok();

    let path = env.cpd_home().join("run/ctx/manager-message.md");
    let message = fs::read_to_string(&path).unwrap();
    assert_eq!(
        message,
        format!("You are the Manager of Context ctx \"Payments\". Follow the cpd-manager skill.\n\n{}\n", MANAGER_POINTER)
    );
    assert!(!message.contains("Mind the ledger."), "nothing from the store is pushed: {message}");
}

/// hs-d655: `context show --digest` is gone; the flag is an unknown argument (`usage`, exit 2).
#[test]
fn context_show_digest_is_usage() {
    let fx = fast();
    let _ = refused(&fx.cpd(&["context", "show", "--context", CTX, "--digest"]), "usage", 2);
}

/// hs-d655: `context_recent_days` is no longer a config key; a config that sets it is `config_invalid`.
#[test]
fn context_recent_days_is_not_a_config_key() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    let out = fx.cpd(&["config"]).ok();
    assert!(out["config"].get("context_recent_days").is_none(), "the key is gone from config: {out}");
    assert!(out["sources"].get("context_recent_days").is_none(), "and from sources: {out}");

    fx.env.write_config("version = 1\nmachine = \"m1\"\ncontext_recent_days = 7\n");
    let run = fx.cpd(&["config"]);
    let err = run.error();
    assert_eq!(run.exit, 1, "{err}");
    assert_eq!(err["code"], "config_invalid", "{err}");
    assert_eq!(err["details"]["key"], "context_recent_days", "{err}");
}
