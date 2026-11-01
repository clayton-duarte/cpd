//! The fake's SQLite state: the schema, the id counters and the read view `State`.
//! A pane is a row; no process is ever started for it.

use rusqlite::{params, Connection, OpenFlags};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use parking_lot::ReentrantMutex;
use std::sync::Arc;

/// A `panes` row joined with its session: id, window, session, argv, cwd, dead, dead status.
type PaneRow = (i64, i64, i64, String, String, bool, Option<i32>);

/// One socket's connection, shared by the calls that run on it and the read views of it. Reentrant: a
/// `run-shell` child on the same thread calls the fake again while its parent waits (the parent has committed).
pub type Shared = Arc<ReentrantMutex<Connection>>;

/// `$TMUX_TMPDIR/fake-tmux-<socket>.db`.
pub fn db_path(tmux_tmpdir: &Path, socket: &str) -> PathBuf {
    tmux_tmpdir.join(format!("fake-tmux-{socket}.db"))
}

/// Created by the first `new-session`. Ids count from 0 (`counters`) and are never reused.
pub const SCHEMA: &str = "
PRAGMA journal_mode = DELETE;
CREATE TABLE IF NOT EXISTS counters (name TEXT PRIMARY KEY, next INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, cur_window INTEGER);
CREATE TABLE IF NOT EXISTS windows (
    id INTEGER PRIMARY KEY, session INTEGER NOT NULL, name TEXT NOT NULL, cur_pane INTEGER,
    width INTEGER NOT NULL DEFAULT 80, height INTEGER NOT NULL DEFAULT 24);
CREATE TABLE IF NOT EXISTS panes (
    id INTEGER PRIMARY KEY, window INTEGER NOT NULL, pos INTEGER NOT NULL, argv TEXT NOT NULL, cwd TEXT NOT NULL,
    dead INTEGER NOT NULL DEFAULT 0, dead_status INTEGER, pid INTEGER NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS options (
    scope TEXT NOT NULL, owner INTEGER NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (scope, owner, key));
CREATE TABLE IF NOT EXISTS env (
    scope TEXT NOT NULL, owner INTEGER NOT NULL, key TEXT NOT NULL, value TEXT, PRIMARY KEY (scope, owner, key));
CREATE TABLE IF NOT EXISTS hooks (name TEXT PRIMARY KEY, cmd TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS bindings (key TEXT PRIMARY KEY, tbl TEXT NOT NULL, cmd TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS buffers (name TEXT PRIMARY KEY, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS keys (seq INTEGER PRIMARY KEY AUTOINCREMENT, pane INTEGER NOT NULL, keys TEXT NOT NULL, literal INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS screens (pane INTEGER PRIMARY KEY, text TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS calls (seq INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, argv TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS clients (
    seq INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, session INTEGER NOT NULL,
    width INTEGER NOT NULL DEFAULT 80, height INTEGER NOT NULL DEFAULT 24);
";

/// Next id of a kind (`session`, `window`, `pane`).
pub fn next_id(db: &Connection, kind: &str) -> rusqlite::Result<i64> {
    db.execute("INSERT OR IGNORE INTO counters (name, next) VALUES (?1, 0)", [kind])?;
    let n: i64 = db.query_row("SELECT next FROM counters WHERE name = ?1", [kind], |r| r.get(0))?;
    db.execute("UPDATE counters SET next = next + 1 WHERE name = ?1", [kind])?;
    Ok(n)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowRow {
    pub id: String,
    /// The session's id (`$n`).
    pub session: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub id: String,
    pub window: String,
    /// The session's id (`$n`).
    pub session: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub dead: bool,
    pub dead_status: Option<i32>,
    pub options: Vec<(String, String)>,
}

/// One `send-keys` call: the pane, the key arguments, whether `-l` was given.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyLog {
    pub pane: String,
    pub keys: Vec<String>,
    pub literal: bool,
}

/// Read-only view of one socket's state.
pub struct State {
    db: Shared,
}

fn pairs(db: &Connection, sql: &str, args: impl rusqlite::Params) -> Vec<(String, String)> {
    let mut st = db.prepare(sql).expect("fake tmux state: prepare");
    st.query_map(args, |r| Ok((r.get(0)?, r.get(1)?))).expect("fake tmux state: query").map(|r| r.expect("row")).collect()
}

impl State {
    /// `None` while no server exists.
    pub fn open(tmux_tmpdir: &Path, socket: &str) -> Option<State> {
        let path = db_path(tmux_tmpdir, socket);
        if !path.exists() {
            return None;
        }
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok().map(|db| State { db: Arc::new(ReentrantMutex::new(db)) })
    }
    /// The view of a connection a [`FakeTmux`](super::FakeTmux) holds.
    pub(super) fn of(db: Shared) -> State {
        State { db }
    }
    fn db(&self) -> impl Deref<Target = Connection> + '_ {
        self.db.lock()
    }
    pub fn panes(&self) -> Vec<Pane> {
        let db = self.db();
        let mut st = db
            .prepare(
                "SELECT p.id, p.window, w.session, p.argv, p.cwd, p.dead, p.dead_status FROM panes p
                 JOIN windows w ON w.id = p.window ORDER BY p.id",
            )
            .expect("prepare");
        let rows: Vec<PaneRow> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        rows.into_iter()
            .map(|(id, window, session, argv, cwd, dead, dead_status)| Pane {
                id: format!("%{id}"),
                window: format!("@{window}"),
                session: format!("${session}"),
                argv: serde_json::from_str(&argv).expect("argv json"),
                cwd,
                dead,
                dead_status,
                options: pairs(
                    &db,
                    "SELECT key, value FROM options WHERE scope = 'pane' AND owner = ?1 ORDER BY rowid",
                    [id],
                ),
            })
            .collect()
    }
    /// The environment given to the pane with `-e`.
    pub fn pane_env(&self, pane_id: &str) -> Vec<(String, String)> {
        let db = self.db();
        let id: i64 = pane_id.trim_start_matches('%').parse().unwrap_or(-1);
        pairs(
            &db,
            "SELECT key, value FROM env WHERE scope = 'pane' AND owner = ?1 AND value IS NOT NULL ORDER BY rowid",
            params![id],
        )
    }
    /// The server (global) options.
    pub fn options(&self) -> Vec<(String, String)> {
        let db = self.db();
        pairs(&db, "SELECT key, value FROM options WHERE scope = 'server' ORDER BY rowid", [])
    }
    pub fn sessions(&self) -> Vec<SessionRow> {
        let db = self.db();
        pairs(&db, "SELECT '$' || id, name FROM sessions ORDER BY id", [])
            .into_iter()
            .map(|(id, name)| SessionRow { id, name })
            .collect()
    }
    pub fn windows(&self) -> Vec<WindowRow> {
        let db = self.db();
        let mut st = db.prepare("SELECT '@' || id, '$' || session, name FROM windows ORDER BY id").expect("prepare");
        st.query_map([], |r| Ok(WindowRow { id: r.get(0)?, session: r.get(1)?, name: r.get(2)? }))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    }
    /// The hooks set with `set-hook -g`: (name, command), in the order set.
    pub fn hooks(&self) -> Vec<(String, String)> {
        let db = self.db();
        pairs(&db, "SELECT name, cmd FROM hooks ORDER BY rowid", [])
    }
    /// The `send-keys` log, in call order.
    pub fn keys(&self) -> Vec<KeyLog> {
        let db = self.db();
        let mut st = db.prepare("SELECT pane, keys, literal FROM keys ORDER BY seq").expect("prepare");
        let rows: Vec<(i64, String, bool)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).expect("query").map(|r| r.expect("row")).collect();
        rows.into_iter()
            .map(|(pane, keys, literal)| KeyLog { pane: format!("%{pane}"), keys: serde_json::from_str(&keys).expect("keys json"), literal })
            .collect()
    }
    /// The names of the attached clients (`attach`).
    pub fn clients(&self) -> Vec<String> {
        let db = self.db();
        pairs(&db, "SELECT name, name FROM clients ORDER BY seq", []).into_iter().map(|p| p.0).collect()
    }
    /// The argv (subcommand name first) of each `display-popup` call.
    pub fn popups(&self) -> Vec<Vec<String>> {
        self.calls("popup")
    }
    /// The argv (subcommand name first) of each `display-menu` call.
    pub fn menus(&self) -> Vec<Vec<String>> {
        self.calls("menu")
    }
    fn calls(&self, kind: &str) -> Vec<Vec<String>> {
        let db = self.db();
        pairs(&db, "SELECT argv, argv FROM calls WHERE kind = ?1 ORDER BY seq", [kind])
            .into_iter()
            .map(|p| serde_json::from_str(&p.0).expect("argv json"))
            .collect()
    }
}
