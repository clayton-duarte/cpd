// The Rust of `20261103T0600-permanent-contexts` (contract §4.5; pro-d338, pro-d340, pro-d343). The migration
// moves data only: `eec-codereview` becomes `codereview`, `host-2` becomes `host` (a rename, or a merge when the
// target exists), `cpd`, `host` and `codereview` are unarchived, the Manager and Lead `cwd` of the CPD_HOME moves
// to the user's home, and the engine index files and the digest directory follow. It creates no Context and no
// Event. A frozen migration: it calls nothing that later changes.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{CpdError, ErrorCode};

/// Every table that holds a Context id in a `context` column (the Context row aside).
const CONTEXT_TABLES: [&str; 9] = [
    "events",
    "context_records",
    "plans",
    "jobs",
    "sessions",
    "inbox_items",
    "board_posts",
    "record_retirements",
    "record_curations",
];

/// The triggers that block an UPDATE of a Context id, a record id or a session name.
const BLOCKING_TRIGGERS: [&str; 3] = ["events_no_update", "context_records_immutable", "jobs_locked"];

pub fn run(conn: &Connection, home: &crate::home::CpdHome) -> Result<(), CpdError> {
    move_context(conn, "eec-codereview", "codereview")?;
    if move_context(conn, "host-2", "host")? {
        conn.execute(
            "UPDATE contexts SET owner = 'clayton-duarte' WHERE id = 'host'",
            [],
        )?;
    }
    for id in ["cpd", "host", "codereview"] {
        conn.execute(
            "UPDATE contexts SET archived_at = NULL WHERE id = ?1 AND deleted_at IS NULL",
            [id],
        )?;
    }
    move_cwd(conn, home)?;
    clean_up(home.root());
    Ok(())
}

fn live(conn: &Connection, id: &str) -> Result<bool, CpdError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM contexts WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn exists(conn: &Connection, id: &str) -> Result<bool, CpdError> {
    Ok(conn
        .query_row("SELECT 1 FROM contexts WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Move the Context `from` to `to`: a rename in place, or a merge when `to` exists. A no-op without a live `from`.
fn move_context(conn: &Connection, from: &str, to: &str) -> Result<bool, CpdError> {
    if !live(conn, from)? {
        return Ok(false);
    }
    // Capture the triggers that block the UPDATEs, drop them, and recreate them from the captured text.
    let mut captured: Vec<String> = Vec::new();
    for name in BLOCKING_TRIGGERS {
        let sql: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(sql) = sql {
            conn.execute_batch(&format!("DROP TRIGGER {name}"))?;
            captured.push(sql);
        }
    }
    if exists(conn, to)? {
        merge(conn, from, to)?;
    } else {
        conn.execute(
            "UPDATE contexts SET id = ?2, title = CASE WHEN title = ?1 THEN ?2 ELSE title END WHERE id = ?1",
            params![from, to],
        )?;
        repoint(conn, from, to)?;
    }
    for sql in captured {
        conn.execute_batch(&sql)?;
    }
    Ok(true)
}

/// Every Context-id column of the child tables, `from` to `to`.
fn repoint(conn: &Connection, from: &str, to: &str) -> Result<(), CpdError> {
    for table in CONTEXT_TABLES {
        conn.execute(
            &format!("UPDATE {table} SET context = ?2 WHERE context = ?1"),
            params![from, to],
        )?;
    }
    Ok(())
}

/// A record id `<machine>-<letter><n>` as (machine, letter, n).
fn parse_record_id(id: &str) -> Option<(&str, char, u64)> {
    let (machine, rest) = id.split_once('-')?;
    let mut chars = rest.chars();
    let letter = chars.next()?;
    let n = chars.as_str().parse::<u64>().ok()?;
    Some((machine, letter, n))
}

fn ids_of(conn: &Connection, context: &str) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare("SELECT id FROM context_records WHERE context = ?1 ORDER BY seq")?;
    let ids = stmt
        .query_map([context], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

fn merge(conn: &Connection, from: &str, to: &str) -> Result<(), CpdError> {
    // (a) the colliding record ids take the next free id in `to`, in seq order
    let theirs = ids_of(conn, to)?;
    let ours = ids_of(conn, from)?;
    let mut max: HashMap<(String, char), u64> = HashMap::new();
    for id in theirs.iter().chain(ours.iter()) {
        if let Some((machine, letter, n)) = parse_record_id(id) {
            let slot = max.entry((machine.to_owned(), letter)).or_insert(0);
            *slot = (*slot).max(n);
        }
    }
    for id in &ours {
        if !theirs.contains(id) {
            continue;
        }
        let (machine, letter, _) = parse_record_id(id).ok_or_else(|| {
            CpdError::new(ErrorCode::StoreFailed, "A Context record id has an unknown shape.")
                .with("record", id.as_str())
        })?;
        let slot = max.entry((machine.to_owned(), letter)).or_insert(0);
        *slot += 1;
        let new = format!("{machine}-{letter}{slot}");
        conn.execute(
            "UPDATE context_records SET id = ?3 WHERE context = ?1 AND id = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE context_records SET supersedes = ?3 WHERE context = ?1 AND supersedes = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE context_records SET question = ?3 WHERE context = ?1 AND question = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE record_retirements SET record = ?3 WHERE context = ?1 AND record = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE record_retirements SET target = ?3 WHERE context = ?1 AND target = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE record_curations SET record = ?3 WHERE context = ?1 AND record = ?2",
            params![from, id, new],
        )?;
        conn.execute(
            "UPDATE inbox_items SET record = ?3 WHERE context = ?1 AND record = ?2",
            params![from, id, new],
        )?;
    }
    // (c) the Manager seat: the incoming live Manager replaces the live one of `to`
    let incoming: bool = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL",
            [from],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if incoming {
        conn.execute(
            "UPDATE sessions SET archived_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL",
            [to],
        )?;
    }
    // a name that carries the id follows it
    conn.execute(
        "UPDATE sessions SET name = CASE
            WHEN name = ?1 || '-manager' THEN ?2 || '-manager'
            WHEN name = ?1 THEN ?2
            WHEN name = 'Manager ' || ?1 THEN 'Manager ' || ?2
            WHEN substr(name, 1, length(?1) + 1) = ?1 || ':' THEN ?2 || ':' || substr(name, length(?1) + 2)
            ELSE name END
          WHERE context = ?1",
        params![from, to],
    )?;
    // (b) every Context-id column
    repoint(conn, from, to)?;
    // (e) the tombstone: its id is never reused
    conn.execute(
        "UPDATE contexts SET deleted_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1",
        [from],
    )?;
    Ok(())
}

/// Manager and Lead sessions whose `cwd` is the CPD_HOME move to the user's home; `sessions.file` stays.
fn move_cwd(conn: &Connection, home: &crate::home::CpdHome) -> Result<(), CpdError> {
    #[allow(deprecated)]
    let user = std::env::home_dir().ok_or_else(|| {
        CpdError::new(
            ErrorCode::Usage,
            "HOME is not set: the permanent-contexts migration cannot move the Manager and Lead cwd.",
        )
    })?;
    let user = user.canonicalize().unwrap_or(user);
    conn.execute(
        "UPDATE sessions SET cwd = ?1 WHERE role IN ('manager', 'lead') AND cwd = ?2",
        params![user.to_string_lossy(), home.root().to_string_lossy()],
    )?;
    Ok(())
}

/// The file clean-up, last: each step ignores a missing file and never fails the migration.
fn clean_up(root: &Path) {
    for name in ["eec-codereview", "codereview", "host", "host-2"] {
        let _ = std::fs::remove_file(root.join("engines/holographic").join(format!("{name}.db")));
    }
    let digests = root.join("run/context-digests");
    let target = digests.join("codereview");
    if !target.exists() {
        let _ = std::fs::rename(digests.join("eec-codereview"), target);
    }
}
