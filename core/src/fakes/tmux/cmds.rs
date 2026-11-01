//! Kill, respawn and pane-I/O commands of the fake tmux: `kill-pane kill-window kill-session respawn-pane
//! resize-pane resize-window select-layout send-keys capture-pane set-buffer show-buffer display-popup
//! display-menu attach list-clients`. Rendering is not modelled (docs/implementing.md, "The fake tmux").

use super::target::{resolve, Kind, Loc};
use super::{client_vars, hooks, ids, next_id, s, say, store_env, Cx, Opts, R};
use rusqlite::Connection;

fn fail(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Removes the window, its panes, and the session when it was the last window. Returns the window's name.
fn remove_window(db: &Connection, session: i64, window: i64) -> R<String> {
    let name: String = s(db, "SELECT name FROM windows WHERE id = ?1", [window]).expect("window row");
    for pane in ids(db, "SELECT id FROM panes WHERE window = ?1", [window]) {
        for table in ["env", "options"] {
            db.execute(&format!("DELETE FROM {table} WHERE scope = 'pane' AND owner = ?1"), [pane]).map_err(fail)?;
        }
    }
    db.execute("DELETE FROM panes WHERE window = ?1", [window]).map_err(fail)?;
    db.execute("DELETE FROM windows WHERE id = ?1", [window]).map_err(fail)?;
    db.execute("DELETE FROM options WHERE scope = 'window' AND owner = ?1", [window]).map_err(fail)?;
    let next: Option<i64> = s::<Option<i64>>(db, "SELECT max(id) FROM windows WHERE session = ?1", [session]).flatten();
    if next.is_some() {
        db.execute("UPDATE sessions SET cur_window = ?1 WHERE id = ?2 AND cur_window = ?3", (next, session, window)).map_err(fail)?;
    } else {
        db.execute("DELETE FROM sessions WHERE id = ?1", [session]).map_err(fail)?;
        for table in ["env", "options"] {
            db.execute(&format!("DELETE FROM {table} WHERE scope = 'session' AND owner = ?1"), [session]).map_err(fail)?;
        }
    }
    Ok(name)
}

/// Fires `window-unlinked` once the window is gone; its target is the session, when that is left.
fn unlinked(cx: &mut Cx, session: i64, name: String) {
    let alive = s::<i64>(cx.db, "SELECT id FROM sessions WHERE id = ?1", [session]).is_some();
    let loc = Loc { session: if alive { session } else { -1 }, window: None, pane: None };
    hooks::fire(cx, "window-unlinked", loc, vec![("hook_window_name".into(), name)]);
}

/// Removes the pane; when it was the last of its window, the window goes too (and fires `window-unlinked`).
/// Ids are not reused.
pub(super) fn kill_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("kill-pane", args, "at:")?;
    let loc = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let (session, window, pane) = (loc.session, loc.window.expect("window"), loc.pane.expect("pane"));
    let db = cx.db;
    db.execute("DELETE FROM panes WHERE id = ?1", [pane]).map_err(fail)?;
    db.execute("DELETE FROM env WHERE scope = 'pane' AND owner = ?1", [pane]).map_err(fail)?;
    db.execute("DELETE FROM options WHERE scope = 'pane' AND owner = ?1", [pane]).map_err(fail)?;
    let left: Option<i64> = s(db, "SELECT id FROM panes WHERE window = ?1 ORDER BY pos LIMIT 1", [window]);
    if left.is_some() {
        db.execute("UPDATE windows SET cur_pane = ?1 WHERE id = ?2 AND cur_pane = ?3", (left, window, pane)).map_err(fail)?;
        return Ok(());
    }
    let name = remove_window(db, session, window)?;
    unlinked(cx, session, name);
    Ok(())
}

pub(super) fn kill_window(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("kill-window", args, "t:")?;
    let loc = resolve(cx.db, o.val('t'), Kind::Window)?;
    let name = remove_window(cx.db, loc.session, loc.window.expect("window"))?;
    unlinked(cx, loc.session, name);
    Ok(())
}

pub(super) fn kill_session(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("kill-session", args, "aCt:")?;
    let session = resolve(cx.db, o.val('t'), Kind::Session)?.session;
    let mut names = Vec::new();
    for w in ids(cx.db, "SELECT id FROM windows WHERE session = ?1 ORDER BY id", [session]) {
        names.push(remove_window(cx.db, session, w)?);
    }
    for name in names {
        unlinked(cx, session, name);
    }
    Ok(())
}

/// `-k` is required when the pane is not dead. Without `-t`, in a hook, the pane is the hook's (as `pane-died`'s
/// `respawn-pane`), not the window's active pane.
pub(super) fn respawn_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("respawn-pane", args, "kc:e:t:")?;
    let loc = match (o.val('t'), cx.loc) {
        (None, Some(hook)) if hook.pane.is_some() => hook,
        (target, _) => resolve(cx.db, target, Kind::Pane)?,
    };
    let pane = loc.pane.expect("pane");
    let dead: i64 = s(cx.db, "SELECT dead FROM panes WHERE id = ?1", [pane]).expect("pane row");
    if dead == 0 && !o.has('k') {
        return Err(format!("respawn pane failed: pane %{pane} still active"));
    }
    if !o.rest.is_empty() {
        let argv = serde_json::to_string(&o.rest).expect("argv json");
        cx.db.execute("UPDATE panes SET argv = ?1 WHERE id = ?2", (argv, pane)).map_err(fail)?;
    }
    if let Some(cwd) = o.val('c') {
        cx.db.execute("UPDATE panes SET cwd = ?1 WHERE id = ?2", (cwd, pane)).map_err(fail)?;
    }
    // A respawned pane runs a new process: a new pid, as a real server gives it.
    let pid = 20000 + next_id(cx.db, "pid").map_err(fail)?;
    cx.db.execute("UPDATE panes SET dead = 0, dead_status = NULL, pid = ?1 WHERE id = ?2", (pid, pane)).map_err(fail)?;
    store_env(cx.db, "pane", pane, &o.all('e')).map_err(fail)
}

pub(super) fn resize_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("resize-pane", args, "DLRUZMTt:x:y:")?;
    let at = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let (window, pane) = (at.window.expect("window"), at.pane.expect("pane"));
    for (flag, col) in [('x', "width"), ('y', "height")] {
        if let Some(v) = o.val(flag) {
            // `N%` is a share of the window's size, as tmux takes it.
            let n: i64 = match v.strip_suffix('%') {
                Some(percent) => {
                    let share: i64 = percent.parse().map_err(|_| format!("size not a number: {v}"))?;
                    let whole: i64 = cx.db.query_row(&format!("SELECT {col} FROM windows WHERE id = ?1"), [window], |r| r.get(0)).map_err(fail)?;
                    whole * share / 100
                }
                None => v.parse().map_err(|_| format!("size not a number: {v}"))?,
            };
            cx.db.execute(&format!("UPDATE panes SET {col} = ?1 WHERE id = ?2"), (n, pane)).map_err(fail)?;
        }
    }
    Ok(())
}

/// Accepted, geometry is not modelled.
pub(super) fn resize_window(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("resize-window", args, "aADLRUt:x:y:")?;
    resolve(cx.db, o.val('t'), Kind::Window).map(|_| ())
}

/// Accepted, layouts are not modelled.
pub(super) fn select_layout(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("select-layout", args, "nNopEt:")?;
    resolve(cx.db, o.val('t'), Kind::Window).map(|_| ())
}

/// Appends to the key log; `-M` (a mouse event) logs nothing.
pub(super) fn send_keys(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("send-keys", args, "FHKlMRXc:N:t:")?;
    if o.has('M') {
        return Ok(());
    }
    let pane = resolve(cx.db, o.val('t'), Kind::Pane)?.pane.expect("pane");
    let keys = serde_json::to_string(&o.rest).expect("keys json");
    cx.db.execute("INSERT INTO keys (pane, keys, literal) VALUES (?1, ?2, ?3)", (pane, keys, o.has('l'))).map_err(fail)?;
    Ok(())
}

/// The screen a test seeded with `__ctl screen`; `-S`/`-E` are accepted and ignored. Without `-p` it goes
/// to the buffer.
pub(super) fn capture_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("capture-pane", args, "aepPqCJNS:E:b:t:")?;
    let pane = resolve(cx.db, o.val('t'), Kind::Pane)?.pane.expect("pane");
    let text: Option<String> = s(cx.db, "SELECT text FROM screens WHERE pane = ?1", [pane]);
    let Some(text) = text else { return Ok(()) };
    if o.has('p') {
        cx.out.push_str(&text);
        cx.out.push('\n');
    } else {
        store_buffer(cx.db, None, &text)?;
    }
    Ok(())
}

fn store_buffer(db: &Connection, name: Option<&str>, data: &str) -> R<()> {
    db.execute(
        "INSERT INTO buffers (name, data) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET data = excluded.data",
        (name.unwrap_or("buffer0"), data),
    )
    .map_err(fail)?;
    Ok(())
}

pub(super) fn set_buffer(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("set-buffer", args, "ab:n:t:w")?;
    let data = o.rest.first().ok_or("usage: set-buffer [-aw] [-b buffer-name] data")?;
    store_buffer(cx.db, o.val('b'), data)
}

pub(super) fn show_buffer(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("show-buffer", args, "b:")?;
    let name = o.val('b').unwrap_or("buffer0");
    let data: String = s(cx.db, "SELECT data FROM buffers WHERE name = ?1", [name]).ok_or(format!("unknown buffer: {name}"))?;
    cx.out.push_str(&data);
    Ok(())
}

/// `display-popup` and `display-menu`: record the argv, subcommand name first. A popup that [`FakeTmux::on_popup`]
/// answers writes the outcome file its command's `--out=` names, as the command it runs would.
///
/// [`FakeTmux::on_popup`]: super::FakeTmux::on_popup
pub(super) fn record_call(cx: &mut Cx, argv: &[String]) -> R<()> {
    let kind = if argv[0] == "display-popup" { "popup" } else { "menu" };
    let json = serde_json::to_string(argv).expect("argv json");
    cx.db.execute("INSERT INTO calls (kind, argv) VALUES (?1, ?2)", (kind, json)).map_err(fail)?;
    if let (true, Some(answer)) = (kind == "popup", cx.popup)
        && let (Some(text), Some(out)) = (answer(argv), out_path(argv)) {
            std::fs::write(&out, text).map_err(|e| format!("display-popup: cannot write {out}: {e}"))?;
        }
    Ok(())
}

/// The path of `--out=<path>` in a popup's command, `<path>` single-quoted (`sh_quote`) or a bare word.
fn out_path(argv: &[String]) -> Option<String> {
    let rest = argv.iter().find_map(|a| a.split_once("--out=").map(|(_, rest)| rest))?;
    match rest.strip_prefix('\'') {
        Some(quoted) => quoted.split('\'').next().map(str::to_owned),
        None => rest.split_whitespace().next().map(str::to_owned),
    }
}

pub(super) fn attach(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("attach", args, "dErxc:f:t:")?;
    let session = resolve(cx.db, o.val('t'), Kind::Session)?.session;
    let n: i64 = s(cx.db, "SELECT count(*) FROM clients", []).expect("count");
    cx.db.execute("INSERT INTO clients (name, session) VALUES (?1, ?2)", (format!("/dev/pts/{n}"), session)).map_err(fail)?;
    Ok(())
}

pub(super) fn list_clients(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("list-clients", args, "F:f:t:")?;
    let sessions = ids(cx.db, "SELECT session FROM clients ORDER BY seq", []);
    let clients = client_vars(cx.db, "SELECT name, width, height, seq FROM clients ORDER BY seq", []);
    for (session, vars) in sessions.into_iter().zip(clients) {
        let kept = cx.vars.len();
        cx.vars.extend(vars);
        say(cx, o.val('F').unwrap_or("#{client_name}"), Loc { session, window: None, pane: None });
        cx.vars.truncate(kept);
    }
    Ok(())
}
