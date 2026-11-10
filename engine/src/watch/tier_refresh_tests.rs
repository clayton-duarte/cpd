//! Red test: E3 reads the role tiers (`[role_tiers]` keys) from the config file on every tick, not the ones loaded when
//! the engine started. Today `schedule::run` overlays only `concurrency` and `durable_workers` on the frozen
//! `engine.config`, so `role_tiers()` (and the worker range with it) stays stale. Same engine instance, fakes only.

use cpd_core::ports::Fakes;

use super::schedule;
use super::testkit::Fx;

/// A `research` Job `key` of Plan `pay` (no Workflow), queued, at `tier`.
fn add_research(fx: &mut Fx, tier: &str, key: &str) {
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
         VALUES ('pay', NULL, '{key}', 'Title of {key}', '{tier}', 'Find {key}.', 'Because.', '[\"{key}.md\"]', '[\"it is answered\"]', '[]', 'research', 'draft', 0, cpd_txn(), 't');"
    ));
    fx.lock_job(key, "");
}

fn attempts(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

/// The engine starts with the default role tiers (lead `O2`: the worker ceiling is `S5`, so a research Job at `S3` is in
/// range). The User then sets `lead_tier = "S2"` (worker ceiling below `S2`): the next tick must refuse the `S3` Job.
#[test]
fn e3_uses_role_tiers_changed_after_start() {
    let mut fx = Fx::with_plan("");
    add_research(&mut fx, "S3", "web");
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();

    let path = fx.engine.home.config();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("lead_tier = \"S2\"\n{text}")).unwrap();

    schedule::run(&mut fx.engine);

    assert_eq!(
        attempts(&mut fx),
        0,
        "S3 is above the worker ceiling the file now sets (lead_tier S2): no Attempt starts"
    );
}
