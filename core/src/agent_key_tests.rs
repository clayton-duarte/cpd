use crate::agent_key::AgentKey;
use crate::ids::{ContextId, JobKey, PlanId};

fn ctx(s: &str) -> ContextId {
    ContextId::parse(s).unwrap()
}
fn plan(s: &str) -> PlanId {
    PlanId::parse(s).unwrap()
}
fn job(s: &str) -> JobKey {
    JobKey::parse(s).unwrap()
}

#[test]
fn agent_key_type_manager_is_the_context_id() {
    assert_eq!(AgentKey::manager(ctx("cpd")).to_string(), "cpd");
}

#[test]
fn agent_key_type_lead_is_context_colon_plan() {
    let key = AgentKey::lead(ctx("cpd"), plan("agent-keys-core"));
    assert_eq!(key.to_string(), "cpd:agent-keys-core");
}

#[test]
fn agent_key_type_worker_is_context_colon_plan_colon_job() {
    let key = AgentKey::worker(ctx("cpd"), plan("agent-keys-core"), job("keys-tests"));
    assert_eq!(key.to_string(), "cpd:agent-keys-core:keys-tests");
}

#[test]
fn agent_key_type_longest_key_is_98_characters() {
    let c = format!("c{}", "a".repeat(39));
    let p = format!("p{}", "b".repeat(23));
    let j = format!("j{}", "c".repeat(31));
    let key = AgentKey::worker(ctx(&c), plan(&p), job(&j)).to_string();
    assert_eq!(key, format!("{c}:{p}:{j}"));
    assert_eq!(key.chars().count(), 98);
    assert_eq!(key.chars().count(), AgentKey::MAX_LEN);
    assert_eq!(AgentKey::MAX_LEN, 98);
}
