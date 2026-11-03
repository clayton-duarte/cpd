//! The header of a pane (contract §6.1; pro-d346): a Lead's pill is its dynamic title when it has one, else its key
//! (`sessions.name`); the Plan title leaves the pill.

use std::fs;

use super::row::load;
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::store::Store;

#[test]
fn row_title_is_the_key_of_a_lead_without_a_dynamic_title() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'cpd', 'plan-x', 'cpd:plan-x', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('plan-x', 'cpd', 'A Plan Title', 'grounding', 'lead1', 't');",
            )?)
        })
        .unwrap();
    let title = store
        .read(|conn| {
            let row = load(conn, &SessionId::parse("lead1").unwrap())?;
            assert!(row.dynamic_title.is_none() && row.plan_lead);
            row.title(conn)
        })
        .unwrap();
    assert_eq!(title, "cpd:plan-x");
}
