//! Red test of the floor clamp (pro-d484): an open/update Job stored below the current worker `range.floor` starts at
//! `range.floor` instead of failing `tier_not_allowed`. No git, no tmux, no process (the fakes of `crate::ports`).

use super::{StartCtx, start_with};
use crate::attempt::explore_tests::{Rig, catalog, plan};
use crate::config::Tier;
use crate::ids::JobKey;
use crate::plan::{jobs, locked_fingerprint};

#[test]
fn open_job_below_floor_is_clamped_to_range_floor() {
    let mut rig = Rig::new();
    // The worker floor was raised to S1 after the open Job was stamped H3 (valid, below the floor).
    rig.env.config.worker_tier = Tier::parse("S1").unwrap();
    rig.sql(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at, description)
           VALUES ('pay', 'api', '/r/api', 'main', 'dispatch', 'cpd/pay/api', '/w/api', 'Api', 'config', cpd_txn(), 't', 't', 'D');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'work', 'T', 'S1', 'g', 'c', '[\"w.txt\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'open-api', 'T', 'H3', 'g', 'c', '[\"a.txt\"]', '[\"b\"]', '[]', 'open', 'draft', 0, cpd_txn(), 't');",
    );
    let all = rig.env.store.read(|c| jobs::load(c, &plan())).unwrap();
    let fp = locked_fingerprint(all.iter().find(|j| j.key == "open-api").unwrap());
    rig.sql(&format!(
        "UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = 'open-api';
         UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = 'work';
         UPDATE jobs SET status = 'running' WHERE key = 'work';
         UPDATE jobs SET status = 'merged', merged_sha = 'c0ffee' WHERE key = 'work';"
    ));
    rig.fakes
        .spawn
        .set_self_exe(std::env::current_exe().unwrap());
    let catalog = catalog();
    let ctx = StartCtx {
        home: &rig.env.home,
        config: &rig.env.config,
        catalog: &catalog,
        command: "test",
    };
    let result = start_with(
        &rig.fakes.ports,
        &mut rig.env.store,
        &ctx,
        &plan(),
        &JobKey::parse("open-api").unwrap(),
    );
    let started =
        result.unwrap_or_else(|e| panic!("the starter refused instead of clamping: {e:?}"));
    assert_eq!(started.tier, Tier::parse("S1").unwrap());
}
