//! What every command opens: `$CPD_HOME`, its config and the store.
//!
//! The one place the open sequence is written: a change to it (the
//! `store_busy_ms` source, a new timeout) is made here.

use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ports::Ports;
use crate::store::{Persist, Store};

/// `$CPD_HOME`, its `config.toml`, the open store and the `Ports` the command runs on.
pub struct Env {
    pub home: CpdHome,
    pub config: Config,
    pub store: Store,
    pub ports: Ports,
    /// A fixed Catalog a test hands in; `None` reads pi's (what every command does).
    pub catalog: Option<crate::config::Catalog>,
}

impl Env {
    /// Opens `$CPD_HOME` from this process's environment (see [`CpdHome::from_env`]) on the real ports.
    pub fn open() -> Result<Env, CpdError> {
        Env::open_with(&Ports::current())
    }

    /// [`Env::open`] on `ports`: what a test hands in.
    pub fn open_with(ports: &Ports) -> Result<Env, CpdError> {
        Env::open_home_with(ports, CpdHome::from_env()?)
    }

    /// The config and the store of `home`, in that order: `config_missing` or
    /// `config_invalid` come before any store failure. On the real ports.
    pub fn open_home(home: CpdHome) -> Result<Env, CpdError> {
        Env::open_home_with(&Ports::current(), home)
    }

    /// [`Env::open_home`] on `ports`: the waits of the open (the setup lock another cpd holds while it migrates) sleep
    /// on `ports.clock`.
    pub fn open_home_with(ports: &Ports, home: CpdHome) -> Result<Env, CpdError> {
        let config = Config::load(&home)?;
        let store = Store::open_with(
            &*ports.clock,
            &home,
            u64::from(config.timeouts.store_busy_ms),
            None,
        )?;
        Ok(Env {
            home,
            config,
            store,
            ports: ports.clone(),
            catalog: None,
        })
    }

    /// [`Env::open_home`] whose store persists through transient errors, the
    /// open included (see [`Store::open_persistent`]).
    pub fn open_home_persistent(home: CpdHome, persist: Persist) -> Result<Env, CpdError> {
        let config = Config::load(&home)?;
        let store =
            Store::open_persistent(&home, u64::from(config.timeouts.store_busy_ms), persist)?;
        Ok(Env {
            home,
            config,
            store,
            ports: Ports::current(),
            catalog: None,
        })
    }

    /// Opens `$CPD_HOME` for `cpd host repair`: tolerates `migration_changed` without running
    /// migrations or writing to the store, so repair can reach divergence detection, confirmation,
    /// and reconciliation.
    pub fn open_for_repair() -> Result<Env, CpdError> {
        Env::open_for_repair_with(&Ports::current())
    }

    /// [`Env::open_for_repair`] on `ports`.
    pub fn open_for_repair_with(ports: &Ports) -> Result<Env, CpdError> {
        Env::open_home_for_repair_with(ports, CpdHome::from_env()?)
    }

    /// [`Env::open_for_repair`] for a specific `home` on `ports`.
    pub fn open_home_for_repair_with(ports: &Ports, home: CpdHome) -> Result<Env, CpdError> {
        let config = Config::load(&home)?;
        let store = Store::open_for_repair_with(
            &*ports.clock,
            &home,
            u64::from(config.timeouts.store_busy_ms),
        )?;
        Ok(Env {
            home,
            config,
            store,
            ports: ports.clone(),
            catalog: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn home(dir: &tempfile::TempDir) -> CpdHome {
        CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
    }

    #[test]
    fn open_home_loads_the_config_then_opens_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(
            home.config(),
            "version = 1\nmachine = \"test\"\n[timeouts]\nstore_busy_ms = 1234\n",
        )
        .unwrap();
        let mut env = Env::open_home(home.clone()).unwrap();
        assert_eq!(
            (env.home.clone(), env.config.timeouts.store_busy_ms),
            (home, 1234)
        );
        assert_eq!(
            env.store
                .read(
                    |c| Ok(c.query_row("SELECT count(*) FROM plans", [], |r| r.get::<_, i64>(0))?)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn open_home_without_a_config_is_config_missing_and_opens_no_store() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let err = Env::open_home(home.clone()).err().expect("no config.toml");
        assert_eq!(err.code(), ErrorCode::ConfigMissing);
        assert!(
            !home.root().join("cpd.db").exists(),
            "the store is opened after the config"
        );
    }

    #[test]
    fn open_for_repair_tolerates_diverged_migration_while_standard_open_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(
            home.config(),
            "version = 1\nmachine = \"test\"\n[timeouts]\nstore_busy_ms = 1234\n",
        )
        .unwrap();

        // First, create the store with standard open
        {
            let _env = Env::open_home(home.clone()).unwrap();
        }

        // Now introduce a diverged checksum into schema_migrations
        {
            let conn = rusqlite::Connection::open(home.db()).unwrap();
            let txn: i64 = conn
                .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
                .unwrap();
            conn.create_scalar_function(
                "cpd_txn",
                0,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8,
                move |_| Ok(txn),
            )
            .unwrap();

            let diverged_migration = "20261110T2200-draft-waiting-on";
            let fake_checksum = "d0e0badbbd9c0000000000000000000000000000000000000000000000000000";

            let row_exists: bool = conn
                .query_row(
                    "SELECT count(*) > 0 FROM schema_migrations WHERE name = ?1",
                    [diverged_migration],
                    |r| r.get(0),
                )
                .unwrap();

            if row_exists {
                conn.execute_batch(&format!(
                    "DROP TRIGGER IF EXISTS schema_migrations_no_update;
                     UPDATE schema_migrations SET checksum = '{fake_checksum}' WHERE name = '{diverged_migration}';
                     CREATE TRIGGER schema_migrations_no_update BEFORE UPDATE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;"
                )).unwrap();
            } else {
                conn.execute(
                    "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
                     VALUES (?1, 'additive', ?2, (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
                    rusqlite::params![diverged_migration, fake_checksum, txn],
                )
                .unwrap();
            }
        }

        // Standard Env::open_home must fail with MigrationChanged per contract §4.5a
        let err = Env::open_home(home.clone())
            .err()
            .expect("must fail on diverged migration");
        assert_eq!(err.code(), ErrorCode::MigrationChanged);

        // Standard Store::open must also fail with MigrationChanged
        let err = Store::open(&home, 1234)
            .err()
            .expect("must fail on diverged migration");
        assert_eq!(err.code(), ErrorCode::MigrationChanged);

        // Env::open_home_for_repair_with must succeed
        let env = Env::open_home_for_repair_with(&Ports::current(), home.clone()).unwrap();
        assert_eq!(env.home, home);

        // Store::open_for_repair must also succeed
        let mut store = Store::open_for_repair(&home, 1234).unwrap();
        assert!(
            store
                .read(
                    |c| Ok(c.query_row("SELECT count(*) FROM plans", [], |r| r.get::<_, i64>(0))?)
                )
                .is_ok()
        );
    }

    /// commands-G2: the open sequence is written once. Every other module of
    /// this crate goes through `Env`; the files below open the store by hand
    /// for a reason of their own.
    #[test]
    fn the_open_sequence_is_written_once() {
        const BY_HAND: &[(&str, &str)] = &[
            ("env.rs", "the one place"),
            ("config/file.rs", "defines the key"),
            ("store/mod.rs", "documents the parameter"),
            (
                "config/catalog.rs",
                "opens the cache's store from a Config it was handed",
            ),
            ("doctor/mod.rs", "must run when the config is invalid"),
            (
                "watch/engine.rs",
                "takes the engine lock between the config and the store",
            ),
            (
                "watch/ensure.rs",
                "checks the engine lock between the config and the store",
            ),
            (
                "pr/merge.rs",
                "needs no config: opens the store alone with the default busy timeout",
            ),
        ];
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut sites = Vec::new();
        let mut dirs = vec![src.clone()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let rel = path
                        .strip_prefix(&src)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    let text = std::fs::read_to_string(&path).unwrap();
                    if text.contains("timeouts.store_busy_ms")
                        && !BY_HAND.iter().any(|(f, _)| *f == rel)
                    {
                        sites.push(rel);
                    }
                }
            }
        }
        sites.sort();
        assert!(
            sites.is_empty(),
            "these files write the open sequence by hand; use Env::open: {sites:?}"
        );
    }
}
