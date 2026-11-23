//! Red tests of the Permanent Contexts (commands.md §3.8; decisions pro-d338..pro-d343): their ids, the ensure step
//! and the home cwd of Managers and Leads.

use std::path::{Path, PathBuf};

use rusqlite::params;

use super::permanent::{ensure, ensure_all};
use crate::config::Config;
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{ContextId, PermanentContext, SessionId};
use crate::session::{agent_cwd, agent_cwd_in};
use crate::store::Store;

const T: &str = "2026-01-01T05:00:00.000Z";

struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    config: Config,
    store: Store,
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let (config, _) =
        Config::parse("version = 1\nmachine = \"m1\"\n", Path::new("config.toml")).unwrap();
    Fx {
        _dir: dir,
        home,
        config,
        store,
    }
}

impl Fx {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn text(&mut self, sql: &str) -> Option<String> {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn ensure(&mut self, context: PermanentContext) -> SessionId {
        ensure(&self.home, &self.config, &mut self.store, context).unwrap()
    }

    /// A Manager Session `id` of Context `context`, in `state`, archived or asleep as asked.
    fn manager(&mut self, id: &str, context: &str, state: &str, archived: bool, asleep: bool) {
        let archived = if archived {
            format!("'{T}'")
        } else {
            "NULL".into()
        };
        let asleep = if asleep {
            format!("'{T}'")
        } else {
            "NULL".into()
        };
        self.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at, asleep_at)
             VALUES ('{id}', 'manager', '{context}', NULL, '{context}-manager', '/', 'O4', '{state}', '{T}', '{T}', {archived}, {asleep});"
        ));
    }

    fn context_row(&mut self, id: &str, title: &str, archived: bool, deleted: bool) {
        let archived = if archived {
            format!("'{T}'")
        } else {
            "NULL".into()
        };
        let deleted = if deleted {
            format!("'{T}'")
        } else {
            "NULL".into()
        };
        self.sql(&format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on, archived_at, deleted_at)
             VALUES ('{id}', '{title}', 'clayton-duarte', '{T}', 'm1', {archived}, {deleted});"
        ));
    }
}

fn home_dir() -> String {
    std::env::home_dir()
        .expect("the test process has a HOME")
        .canonicalize()
        .unwrap()
        .display()
        .to_string()
}

#[test]
fn the_permanent_ids_are_cpd_host_and_codereview_in_that_order() {
    let ids: Vec<_> = PermanentContext::ALL.iter().map(|c| c.id()).collect();
    assert_eq!(ids, ["cpd", "host", "codereview"]);
    let titles: Vec<_> = PermanentContext::ALL.iter().map(|c| c.title()).collect();
    assert_eq!(
        titles,
        [
            "cpd",
            "Host: local maintenance of this machine",
            "codereview"
        ]
    );
}

#[test]
fn a_context_id_is_permanent_only_when_it_is_one_of_the_three() {
    for other in ["eec-codereview", "host-2", "cpd-2"] {
        assert_eq!(
            ContextId::parse(other).unwrap().permanent(),
            None,
            "{other}"
        );
    }
    let found: Vec<_> = ["cpd", "host", "codereview"]
        .iter()
        .map(|id| ContextId::parse(id).unwrap().permanent())
        .collect();
    assert_eq!(
        found,
        [
            Some(PermanentContext::Cpd),
            Some(PermanentContext::Host),
            Some(PermanentContext::Codereview)
        ]
    );
}

#[test]
fn ensure_creates_a_missing_context_owned_by_the_fallback_owner_with_a_stopped_manager() {
    let mut fx = fixture();
    let id = fx.ensure(PermanentContext::Host);

    assert_eq!(
        fx.text("SELECT owner FROM contexts WHERE id = 'host'")
            .as_deref(),
        Some("clayton-duarte")
    );
    assert_eq!(
        fx.text("SELECT title FROM contexts WHERE id = 'host'")
            .as_deref(),
        Some("Host: local maintenance of this machine")
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'context-created' AND context = 'host'"),
        1
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM sessions WHERE context = 'host'"),
        1
    );
    let row = fx
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT role, state, by_user, tier, runner, archived_at FROM sessions WHERE id = ?1",
                params![id.as_str()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                    ))
                },
            )?)
        })
        .unwrap();
    assert_eq!(
        row,
        (
            "manager".to_owned(),
            "stopped".to_owned(),
            false,
            fx.config.manager_tier.to_string(),
            fx.config.manager_runner.as_str().to_owned(),
            None
        )
    );
    // `by_user = false`: the first message file is written.
    assert!(
        fx.home
            .manager_message(&ContextId::parse("host").unwrap())
            .exists(),
        "the Manager's first message"
    );
}

#[test]
fn ensure_is_idempotent_and_writes_nothing_the_second_time() {
    let mut fx = fixture();
    let first = fx.ensure(PermanentContext::Cpd);
    let txns = fx.count("SELECT count(*) FROM txns");
    let events = fx.count("SELECT count(*) FROM events");
    let second = fx.ensure(PermanentContext::Cpd);
    assert_eq!(first, second);
    assert_eq!(
        fx.count("SELECT count(*) FROM contexts WHERE id = 'cpd'"),
        1
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM sessions WHERE context = 'cpd'"),
        1
    );
    assert_eq!(fx.count("SELECT count(*) FROM events"), events);
    assert_eq!(fx.count("SELECT count(*) FROM txns"), txns);
}

#[test]
fn ensure_registers_a_new_manager_when_the_only_one_is_archived() {
    let mut fx = fixture();
    fx.context_row("codereview", "codereview", false, false);
    fx.manager("m-old", "codereview", "stopped", true, false);
    let id = fx.ensure(PermanentContext::Codereview);
    assert_ne!(id.as_str(), "m-old");
    assert_eq!(
        fx.count("SELECT count(*) FROM sessions WHERE context = 'codereview' AND role = 'manager'"),
        2
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM sessions WHERE id = 'm-old' AND archived_at IS NOT NULL"),
        1
    );
    assert_eq!(
        fx.count(
            "SELECT count(*) FROM sessions WHERE context = 'codereview' AND role = 'manager' AND archived_at IS NULL"
        ),
        1
    );
}

#[test]
fn ensure_leaves_a_live_manager_alone_asleep_or_stopped() {
    for (asleep, state) in [(false, "stopped"), (true, "idle"), (true, "stopped")] {
        let mut fx = fixture();
        fx.context_row("cpd", "cpd", false, false);
        fx.manager("m-live", "cpd", state, false, asleep);
        let txns = fx.count("SELECT count(*) FROM txns");
        let id = fx.ensure(PermanentContext::Cpd);
        assert_eq!(id.as_str(), "m-live", "{state} asleep={asleep}");
        assert_eq!(fx.count("SELECT count(*) FROM sessions"), 1);
        assert_eq!(
            fx.count("SELECT count(*) FROM txns"),
            txns,
            "nothing written"
        );
    }
}

#[test]
fn ensure_clears_archived_at_of_the_context_row_only() {
    let mut fx = fixture();
    fx.context_row(
        "host",
        "Host: local maintenance of this machine",
        true,
        false,
    );
    fx.manager("m-host", "host", "stopped", false, false);
    fx.sql(&format!(
        "UPDATE sessions SET archived_at = '{T}' WHERE id = 'm-host';
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('l-old', 'lead', 'host', 'p-old', 'Lead', '/', 'O3', 'stopped', '{T}', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, archived_at)
           VALUES ('p-old', 'host', 'Old', 'grounding', 'l-old', '{T}', '{T}');"
    ));
    fx.ensure(PermanentContext::Host);
    assert_eq!(
        fx.count("SELECT count(*) FROM contexts WHERE id = 'host' AND archived_at IS NULL"),
        1
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM plans WHERE id = 'p-old' AND archived_at IS NOT NULL"),
        1
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM sessions WHERE id = 'm-host' AND archived_at IS NOT NULL"),
        1,
        "the archived Session stays archived"
    );
}

#[test]
fn ensure_refuses_a_deleted_context_row() {
    let mut fx = fixture();
    fx.context_row("cpd", "cpd", false, true);
    let err = ensure(&fx.home, &fx.config, &mut fx.store, PermanentContext::Cpd).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ContextExists);
    assert_eq!(fx.count("SELECT count(*) FROM sessions"), 0);
}

#[test]
fn ensure_all_covers_the_three_in_order() {
    let mut fx = fixture();
    let ids = ensure_all(&fx.home, &fx.config, &mut fx.store).unwrap();
    assert_eq!(ids.len(), 3);
    for (id, context) in ids.iter().zip(PermanentContext::ALL) {
        let found = fx.text(&format!(
            "SELECT context FROM sessions WHERE id = '{id}' AND role = 'manager'"
        ));
        assert_eq!(found.as_deref(), Some(context.id()));
    }
}

#[test]
fn the_ensured_manager_runs_in_the_users_home() {
    let mut fx = fixture();
    let id = fx.ensure(PermanentContext::Cpd);
    assert_eq!(
        fx.text(&format!("SELECT cwd FROM sessions WHERE id = '{id}'")),
        Some(home_dir())
    );
}

#[test]
fn agent_cwd_is_the_users_home_and_unknown_home_is_usage() {
    assert_eq!(agent_cwd().unwrap(), home_dir());
    assert_eq!(
        agent_cwd().unwrap(),
        agent_cwd_in(std::env::home_dir()).unwrap()
    );
    let tmp = tempfile::tempdir().unwrap();
    let link: PathBuf = tmp.path().canonicalize().unwrap();
    assert_eq!(
        agent_cwd_in(Some(link.clone())).unwrap(),
        link.display().to_string()
    );
    let err = agent_cwd_in(None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::Usage);
    assert_eq!(err.exit().code(), 2);
}
