//! The migration `session-runner` (contract §4.4, §4.5): `sessions.runner` (`pi` or `durable`, default `pi`).

use super::*;
use crate::error::CpdError;
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

const BASE: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');";

/// A Lead row of Plan `pay`; `columns` and `values` add to the columns every Session has.
fn session(id: &str, columns: &str, values: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at{columns})
         VALUES ('{id}', 'lead', 'ctx', 'pay', 'Lead {id}', '/', 'O3', 'idle', 't', 't'{values});"
    )
}

fn runner_of(store: &mut Store, id: &str) -> Result<String, CpdError> {
    store.read(|c| Ok(c.query_row("SELECT runner FROM sessions WHERE id = ?1", [id], |r| r.get(0))?))
}

#[test]
fn session_runner_defaults_to_pi_for_a_row_inserted_without_it() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(&mut store, &format!("{BASE}{}", session("lead", "", ""))).expect("a Session that does not name `runner`");
    assert_eq!(runner_of(&mut store, "lead").expect("the column `runner` exists"), "pi");
    write(&mut store, &session("lead2", ", runner", ", 'durable'")).expect("a durable Session");
    assert_eq!(runner_of(&mut store, "lead2").unwrap(), "durable");
    assert!(NAMED.iter().any(|m| m.name.ends_with("-session-runner")), "the migration session-runner is declared");
}

#[test]
fn session_runner_refuses_a_value_other_than_pi_or_durable() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(&mut store, &format!("{BASE}{}", session("lead", "", ""))).unwrap();
    write(&mut store, &session("ok", ", runner", ", 'pi'")).expect("`pi` is a runner");
    write(&mut store, &session("ok2", ", runner", ", 'durable'")).expect("`durable` is a runner");
    for (n, bad) in [(1, "tmux"), (2, "PI"), (3, "")] {
        let failed = write(&mut store, &session(&format!("bad{n}"), ", runner", &format!(", '{bad}'")))
            .expect_err(bad)
            .details()["sqlite_message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(failed.contains("CHECK"), "{bad:?}: {failed}");
    }
}

#[test]
fn session_runner_migration_keeps_every_existing_row_as_pi() {
    let idx = NAMED.iter().position(|m| m.name.ends_with("-session-runner")).expect("the migration session-runner is declared");
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut old = crate::store::tests::open_named(&home, &NAMED[..idx]).unwrap();
    write(&mut old, &format!("{BASE}{}{}", session("lead", "", ""), session("other", "", ""))).unwrap();
    drop(old);
    let mut store = Store::open(&home, 5000).unwrap();
    assert_eq!(runner_of(&mut store, "lead").unwrap(), "pi");
    assert_eq!(runner_of(&mut store, "other").unwrap(), "pi");
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
}
