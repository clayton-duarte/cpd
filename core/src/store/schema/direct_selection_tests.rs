//! Migration preserves old Job rows and schema while enforcing selection states.
use super::*;
use crate::store::Store;

#[test]
fn job_legacy_selection_and_invalid_states() {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-job-direct-selection"))
        .collect();
    let mut old = crate::store::tests::open_named(&home, &earlier).unwrap();
    old.write("fixture", |tx| {
        tx.execute_batch("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
          INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
          INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'plan', 'lead', 't');
          INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES ('pl', 'a', '/r', 'main', 'waiting', 'cpd/pl/a', cpd_txn(), 't');
          INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
          VALUES ('pl', 'a', 'legacy', 'Legacy', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, cpd_txn(), 't'),
                 ('pl', 'a', 'stub', 'Stub', NULL, NULL, NULL, NULL, NULL, NULL, 'draft', 0, cpd_txn(), 't');")?;
        Ok(())
    }).unwrap();
    let objects = |s: &mut Store| {
        s.read(|c| {
        let mut q = c.prepare("SELECT type, name FROM sqlite_schema WHERE (tbl_name = 'jobs' AND type IN ('index','trigger')) AND name NOT LIKE 'sqlite_%' ORDER BY type,name")?;
        Ok(q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }).unwrap()
    };
    let before = objects(&mut old);
    drop(old);
    assert!(
        earlier.len() < NAMED.len(),
        "job-direct-selection migration must exist"
    );
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none());
    let after = objects(&mut store);
    for obj in before {
        assert!(after.contains(&obj), "lost {obj:?}");
    }
    store.read(|c| {
        let count: i64 = c.query_row("SELECT count(*) FROM jobs WHERE (key='legacy' AND tier='S2') OR (key='stub' AND tier IS NULL)", [], |r| r.get(0))?;
        assert_eq!(count, 2);
        let fk: i64 = c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get(0))?;
        assert_eq!(fk, 0);
        Ok(())
    }).unwrap();
    store.write("valid selections", |tx| {
        tx.execute_batch("UPDATE jobs SET selected_model='provider/model', selected_effort=2 WHERE key='stub';
          INSERT INTO jobs (plan, workflow, key, title, status, appended, created_txn, created_at) VALUES ('pl', 'a', 'cancelled', 'Cancelled', 'cancelled', 0, cpd_txn(), 't');")?;
        Ok(())
    }).unwrap();
    store
        .read(|c| {
            let jobs = crate::plan::jobs::load(c, &crate::ids::PlanId::parse("pl").unwrap())?;
            assert_eq!(
                jobs.iter().find(|j| j.key == "legacy").unwrap().selection(),
                crate::plan::jobs::JobSelection::LegacyTier("S2".into())
            );
            assert_eq!(
                jobs.iter()
                    .find(|j| j.key == "cancelled")
                    .unwrap()
                    .selection(),
                crate::plan::jobs::JobSelection::Unselected
            );
            assert_eq!(
                jobs.iter().find(|j| j.key == "stub").unwrap().selection(),
                crate::plan::jobs::JobSelection::Direct(crate::policy::RoleSelection {
                    model: "provider/model".into(),
                    effort: 2
                })
            );
            Ok(())
        })
        .unwrap();
    for set in [
        "tier='S2', selected_model='provider/model', selected_effort=2",
        "tier=NULL, selected_model='provider/model', selected_effort=NULL",
        "tier=NULL, selected_model=NULL, selected_effort=2",
        "tier=NULL, selected_model='', selected_effort=2",
        "tier=NULL, selected_model='provider/model', selected_effort=-1",
    ] {
        let result = store.write("invalid", |tx| {
            tx.execute_batch(&format!("UPDATE jobs SET {set} WHERE key='stub'"))?;
            Ok(())
        });
        assert!(result.is_err(), "invalid selection accepted: {set}");
    }
    drop(store);
    drop(dir);
}
