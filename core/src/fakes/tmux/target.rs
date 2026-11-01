//! Target (`-t`) resolution: `=name`, `=name:`, `name`, `name:window`, `$s`, `@w`, `%p`, `=`.
//! A bare name is the session's exact name or else its unique prefix; `=name` is exact.

use rusqlite::{Connection, OptionalExtension};

/// What the command wants the target to name.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Session,
    Window,
    Pane,
}

/// The resolved ids (without the sigil). `window` and `pane` are set as far as `Kind` asks.
#[derive(Debug, Clone, Copy)]
pub struct Loc {
    pub session: i64,
    pub window: Option<i64>,
    pub pane: Option<i64>,
}

type R<T> = Result<T, String>;

fn q<T: rusqlite::types::FromSql>(
    db: &Connection,
    sql: &str,
    arg: impl rusqlite::ToSql,
) -> Option<T> {
    db.query_row(sql, [arg], |r| r.get(0))
        .optional()
        .expect("fake tmux state")
}

fn id_of(s: &str, sigil: char) -> Option<i64> {
    s.strip_prefix(sigil)?.parse().ok()
}

pub fn resolve(db: &Connection, target: Option<&str>, kind: Kind) -> R<Loc> {
    let t = target.unwrap_or("");
    if let Some(n) = id_of(t, '%') {
        let w: i64 = q(db, "SELECT window FROM panes WHERE id = ?1", n)
            .ok_or(format!("can't find pane: {t}"))?;
        let s = q(db, "SELECT session FROM windows WHERE id = ?1", w).expect("pane's window");
        return Ok(Loc {
            session: s,
            window: Some(w),
            pane: Some(n),
        });
    }
    if let Some(w) = id_of(t, '@') {
        let s: i64 = q(db, "SELECT session FROM windows WHERE id = ?1", w)
            .ok_or(format!("can't find window: {t}"))?;
        return finish(db, s, Some(w), kind);
    }
    let (sess, win) = match t.split_once(':') {
        Some((s, w)) => (s, Some(w)),
        None => (t, None),
    };
    let s = session(db, sess)?;
    let w = match win {
        None | Some("") => None,
        Some(name) => Some(window(db, s, name)?),
    };
    finish(db, s, w, kind)
}

fn finish(db: &Connection, session: i64, window: Option<i64>, kind: Kind) -> R<Loc> {
    if kind == Kind::Session {
        return Ok(Loc {
            session,
            window: None,
            pane: None,
        });
    }
    let window = match window {
        Some(w) => w,
        None => q::<i64>(db, "SELECT cur_window FROM sessions WHERE id = ?1", session)
            .ok_or("no current window")?,
    };
    let pane = if kind == Kind::Pane {
        Some(
            q::<i64>(db, "SELECT cur_pane FROM windows WHERE id = ?1", window)
                .ok_or("no current pane")?,
        )
    } else {
        None
    };
    Ok(Loc {
        session,
        window: Some(window),
        pane,
    })
}

fn session(db: &Connection, spec: &str) -> R<i64> {
    if spec.is_empty() || spec == "=" {
        let latest: Option<i64> = db
            .query_row("SELECT max(id) FROM sessions", [], |r| r.get(0))
            .expect("fake tmux state");
        return latest.ok_or_else(|| "no current session".to_string());
    }
    if let Some(id) = id_of(spec, '$') {
        return q(db, "SELECT id FROM sessions WHERE id = ?1", id)
            .ok_or(format!("can't find session: {spec}"));
    }
    let (exact, name) = match spec.strip_prefix('=') {
        Some(n) => (true, n),
        None => (false, spec),
    };
    if let Some(id) = q(db, "SELECT id FROM sessions WHERE name = ?1", name) {
        return Ok(id);
    }
    if !exact {
        let mut st = db
            .prepare("SELECT id FROM sessions WHERE substr(name, 1, length(?1)) = ?1")
            .expect("prepare");
        let ids: Vec<i64> = st
            .query_map([name], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        if let [id] = ids[..] {
            return Ok(id);
        }
    }
    Err(format!("can't find session: {name}"))
}

/// A window of `session` by `@id`, index or exact name.
fn window(db: &Connection, session: i64, spec: &str) -> R<i64> {
    let ids: Vec<i64> = {
        let mut st = db
            .prepare("SELECT id FROM windows WHERE session = ?1 ORDER BY id")
            .expect("prepare");
        st.query_map([session], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    };
    if let Some(w) = id_of(spec, '@') {
        return ids
            .contains(&w)
            .then_some(w)
            .ok_or(format!("can't find window: {spec}"));
    }
    if let Ok(i) = spec.parse::<usize>()
        && let Some(w) = ids.get(i)
    {
        return Ok(*w);
    }
    db.query_row(
        "SELECT id FROM windows WHERE session = ?1 AND name = ?2 ORDER BY id",
        (session, spec),
        |r| r.get(0),
    )
    .optional()
    .expect("fake tmux state")
    .ok_or(format!("can't find window: {spec}"))
}
