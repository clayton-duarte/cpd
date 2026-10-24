//! The fake tmux (docs/implementing.md, Tests, "The fake tmux"): `run` parses the global flags, splits the
//! argv on `;` and runs each command inside one `BEGIN IMMEDIATE` transaction. This Job: state, targets,
//! options and formats; the other subcommands answer `unknown command: <name>`.

pub mod format;
pub mod state;
pub mod target;

pub use state::{Pane, SessionRow, State, WindowRow};

use rusqlite::Connection;
use state::{db_path, next_id};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use target::{resolve, Kind, Loc};

type R<T> = Result<T, String>;

/// One client call: everything after `tmux`, including the global flags.
pub struct Invocation {
    pub argv: Vec<String>,
    pub tmux_tmpdir: PathBuf,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The one entry the bin and the unit tests share.
pub fn run(inv: &Invocation) -> Outcome {
    match run_commands(inv) {
        Ok(stdout) => Outcome { code: 0, stdout, stderr: String::new() },
        Err(e) => Outcome { code: 1, stdout: String::new(), stderr: format!("{e}\n") },
    }
}

/// The commands of this Job. Others (kill-pane, send-keys, ...) belong to the next Jobs.
const KNOWN: &[&str] = &[
    "new-session", "has-session", "kill-server", "list-sessions", "new-window", "split-window", "list-windows",
    "list-panes", "rename-window", "swap-pane", "set", "show", "set-environment", "show-environment", "display",
    "display-message", "select-pane", "kill-pane",
];

struct Cx<'a> {
    db: &'a Connection,
    inv: &'a Invocation,
    out: String,
    kill: bool,
}

fn run_commands(inv: &Invocation) -> R<String> {
    let mut socket = "default".to_string();
    let mut i = 0;
    while i < inv.argv.len() {
        match inv.argv[i].as_str() {
            "-u" => i += 1,
            "-L" | "-f" | "-S" => {
                let v = inv.argv.get(i + 1).ok_or("option requires an argument -- L")?;
                if inv.argv[i] == "-L" {
                    socket = v.clone();
                }
                i += 2;
            }
            _ => break,
        }
    }
    let commands: Vec<Vec<String>> =
        inv.argv[i..].split(|a| a == ";").filter(|c| !c.is_empty()).map(<[String]>::to_vec).collect();
    if let Some(c) = commands.iter().find(|c| !KNOWN.contains(&c[0].as_str())) {
        return Err(format!("unknown command: {}", c[0]));
    }
    let Some(first) = commands.first() else { return Ok(String::new()) };
    let _ = first;
    let meta = std::fs::metadata(&inv.tmux_tmpdir)
        .map_err(|e| format!("error creating {} ({e})", inv.tmux_tmpdir.display()))?;
    let path = db_path(&inv.tmux_tmpdir, &socket);
    let creating = !path.exists() && commands.iter().any(|c| c[0] == "new-session");
    if !path.exists() && !creating {
        return Err(format!("no server running on {}/tmux-{}/{socket}", inv.tmux_tmpdir.display(), meta.uid()));
    }
    let db = Connection::open(&path).map_err(|e| e.to_string())?;
    let fail = |e: rusqlite::Error| e.to_string();
    if creating {
        db.execute_batch(state::SCHEMA).map_err(fail)?;
    }
    db.execute_batch("BEGIN IMMEDIATE").map_err(fail)?;
    let mut cx = Cx { db: &db, inv, out: String::new(), kill: false };
    let result = commands.iter().try_for_each(|c| dispatch(&mut cx, c));
    let (out, kill) = (std::mem::take(&mut cx.out), cx.kill);
    match result {
        Ok(()) => db.execute_batch("COMMIT").map_err(fail)?,
        Err(e) => {
            drop(db);
            if creating {
                let _ = std::fs::remove_file(&path);
            }
            return Err(e);
        }
    }
    drop(db);
    if kill {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn dispatch(cx: &mut Cx, c: &[String]) -> R<()> {
    let args = &c[1..];
    match c[0].as_str() {
        "new-session" => new_session(cx, args),
        "has-session" => has_session(cx, args),
        "kill-server" => {
            cx.kill = true;
            Ok(())
        }
        "list-sessions" => list_sessions(cx, args),
        "new-window" => new_window(cx, args),
        "split-window" => split_window(cx, args),
        "list-windows" => list_windows(cx, args),
        "list-panes" => list_panes(cx, args),
        "rename-window" => rename_window(cx, args),
        "swap-pane" => swap_pane(cx, args),
        "set" => set(cx, args),
        "show" => show(cx, args),
        "set-environment" => set_environment(cx, args),
        "show-environment" => show_environment(cx, args),
        "display" | "display-message" => display_message(cx, args),
        "select-pane" => select_pane(cx, args),
        "kill-pane" => kill_pane(cx, args),
        other => Err(format!("unknown command: {other}")),
    }
}

// ---- arguments ----

/// getopt-style: `spec` lists the flags, a `:` after one means it takes a value; stops at `--` or the
/// first non-option.
struct Opts {
    flags: Vec<(char, Option<String>)>,
    rest: Vec<String>,
}

impl Opts {
    fn parse(cmd: &str, args: &[String], spec: &str) -> R<Opts> {
        let (mut flags, mut i) = (Vec::new(), 0);
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                break;
            }
            let Some(cluster) = a.strip_prefix('-').filter(|c| !c.is_empty()) else { break };
            i += 1;
            for (at, ch) in cluster.char_indices() {
                let at_spec = spec.find(ch).ok_or(format!("command {cmd}: unknown flag -{ch}"))?;
                if spec[at_spec + 1..].starts_with(':') {
                    let attached = &cluster[at + ch.len_utf8()..];
                    let v = if attached.is_empty() {
                        i += 1;
                        args.get(i - 1).cloned().ok_or(format!("command {cmd}: -{ch} expects an argument"))?
                    } else {
                        attached.to_string()
                    };
                    flags.push((ch, Some(v)));
                    break;
                }
                flags.push((ch, None));
            }
        }
        Ok(Opts { flags, rest: args[i..].to_vec() })
    }
    fn has(&self, c: char) -> bool {
        self.flags.iter().any(|f| f.0 == c)
    }
    fn val(&self, c: char) -> Option<&str> {
        self.flags.iter().rev().find(|f| f.0 == c).and_then(|f| f.1.as_deref())
    }
    fn all(&self, c: char) -> Vec<&str> {
        self.flags.iter().filter(|f| f.0 == c).filter_map(|f| f.1.as_deref()).collect()
    }
}

// ---- variables and output ----

fn s<T: rusqlite::types::FromSql>(db: &Connection, sql: &str, p: impl rusqlite::Params) -> Option<T> {
    use rusqlite::OptionalExtension;
    db.query_row(sql, p, |r| r.get(0)).optional().expect("fake tmux state")
}

fn option_in(db: &Connection, scope: &str, owner: i64, key: &str) -> Option<String> {
    s(db, "SELECT value FROM options WHERE scope = ?1 AND owner = ?2 AND key = ?3", (scope, owner, key))
}

/// The value of a format variable at `loc`; `None` when unknown.
fn variable(cx: &Cx, loc: Loc, name: &str) -> Option<String> {
    let db = cx.db;
    if name.starts_with('@') {
        let pane = loc.pane.and_then(|p| option_in(db, "pane", p, name));
        let window = loc.window.and_then(|w| option_in(db, "window", w, name));
        return pane.or(window).or_else(|| option_in(db, "session", loc.session, name)).or_else(|| option_in(db, "server", 0, name));
    }
    let pane_row = |col: &str| -> Option<i64> {
        s::<Option<i64>>(db, &format!("SELECT {col} FROM panes WHERE id = ?1"), [loc.pane?])?
    };
    Some(match name {
        "session_id" => format!("${}", loc.session),
        "session_name" => s(db, "SELECT name FROM sessions WHERE id = ?1", [loc.session])?,
        "window_id" => format!("@{}", loc.window?),
        "window_name" => s(db, "SELECT name FROM windows WHERE id = ?1", [loc.window?])?,
        "window_index" => s::<i64>(
            db,
            "SELECT count(*) FROM windows WHERE session = ?1 AND id < ?2",
            (loc.session, loc.window?),
        )?
        .to_string(),
        "pane_id" => format!("%{}", loc.pane?),
        "pane_index" => s::<i64>(
            db,
            "SELECT count(*) FROM panes WHERE window = ?1 AND pos < (SELECT pos FROM panes WHERE id = ?2)",
            (loc.window?, loc.pane?),
        )?
        .to_string(),
        "pane_pid" => pane_row("pid")?.to_string(),
        "pane_dead" => pane_row("dead")?.to_string(),
        "pane_dead_status" => pane_row("dead_status").map(|v| v.to_string()).unwrap_or_default(),
        "pane_width" => pane_row("width")?.to_string(),
        "pane_height" => pane_row("height")?.to_string(),
        "client_name" => "/dev/pts/0".into(),
        "client_width" => "80".into(),
        "client_height" => "24".into(),
        "client_activity" => "0".into(),
        "version" => "3.4".into(),
        "pid" => server_pid(cx.inv),
        "hook_window_name" => String::new(),
        _ => return None,
    })
}

fn server_pid(inv: &Invocation) -> String {
    inv.env
        .iter()
        .find(|(k, _)| k == "FAKE_TMUX_SERVER_PID")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::process::id().to_string())
}

fn say(cx: &mut Cx, fmt: &str, loc: Loc) {
    let line = format::expand(fmt, &|n| variable(cx, loc, n));
    cx.out.push_str(&line);
    cx.out.push('\n');
}

fn loc_of_window(db: &Connection, window: i64) -> Loc {
    let session = s(db, "SELECT session FROM windows WHERE id = ?1", [window]).expect("window row");
    Loc { session, window: Some(window), pane: s(db, "SELECT cur_pane FROM windows WHERE id = ?1", [window]) }
}

fn loc_of_pane(db: &Connection, pane: i64) -> Loc {
    let window: i64 = s(db, "SELECT window FROM panes WHERE id = ?1", [pane]).expect("pane row");
    Loc { pane: Some(pane), ..loc_of_window(db, window) }
}

fn ids(db: &Connection, sql: &str, p: impl rusqlite::Params) -> Vec<i64> {
    let mut st = db.prepare(sql).expect("prepare");
    st.query_map(p, |r| r.get(0)).expect("query").map(|r| r.expect("row")).collect()
}

// ---- sessions, windows, panes ----

fn default_window_name(argv: &[String]) -> String {
    let base = argv.first().and_then(|a| a.rsplit('/').next()).filter(|b| !b.is_empty());
    base.unwrap_or("sh").to_string()
}

fn int(v: Option<&str>, default: i64) -> R<i64> {
    v.map_or(Ok(default), |v| v.parse().map_err(|_| format!("size not a number: {v}")))
}

/// Adds a window with one pane to `session`; selects it when `select`.
fn add_window(cx: &Cx, session: i64, name: Option<&str>, o: &Opts, select: bool) -> R<Loc> {
    let db = cx.db;
    let fail = |e: rusqlite::Error| e.to_string();
    let (window, pane) = (next_id(db, "window").map_err(fail)?, next_id(db, "pane").map_err(fail)?);
    let name = name.map_or_else(|| default_window_name(&o.rest), str::to_string);
    let cwd = o.val('c').map_or_else(|| cx.inv.cwd.display().to_string(), str::to_string);
    let (w, h) = (int(o.val('x'), 80)?, int(o.val('y'), 24)?);
    let argv = serde_json::to_string(&o.rest).expect("argv json");
    db.execute("INSERT INTO windows VALUES (?1, ?2, ?3, ?4)", (window, session, name, pane)).map_err(fail)?;
    db.execute("INSERT INTO panes VALUES (?1, ?2, 0, ?3, ?4, 0, NULL, ?5, ?6, ?7)", (pane, window, argv, cwd, 10000 + pane, w, h))
        .map_err(fail)?;
    store_env(db, "pane", pane, &o.all('e')).map_err(fail)?;
    if select {
        db.execute("UPDATE sessions SET cur_window = ?1 WHERE id = ?2", (window, session)).map_err(fail)?;
    }
    Ok(Loc { session, window: Some(window), pane: Some(pane) })
}

fn store_env(db: &Connection, scope: &str, owner: i64, pairs: &[&str]) -> rusqlite::Result<()> {
    for p in pairs {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        db.execute("INSERT OR REPLACE INTO env VALUES (?1, ?2, ?3, ?4)", (scope, owner, k, v))?;
    }
    Ok(())
}

fn new_session(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("new-session", args, "dDAEPXs:n:c:e:x:y:F:t:f:")?;
    if let Some(name) = o.val('s') {
        if s::<i64>(cx.db, "SELECT id FROM sessions WHERE name = ?1", [name]).is_some() {
            return Err(format!("duplicate session: {name}"));
        }
    }
    let session = next_id(cx.db, "session").map_err(|e| e.to_string())?;
    let name = o.val('s').map_or_else(|| session.to_string(), str::to_string);
    cx.db.execute("INSERT INTO sessions VALUES (?1, ?2, NULL)", (session, name)).map_err(|e| e.to_string())?;
    let loc = add_window(cx, session, o.val('n'), &o, true)?;
    if o.has('P') {
        say(cx, o.val('F').unwrap_or("#{session_name}:"), loc);
    }
    Ok(())
}

fn has_session(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("has-session", args, "t:")?;
    resolve(cx.db, o.val('t'), Kind::Session).map(|_| ())
}

fn list_sessions(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("list-sessions", args, "F:f:")?;
    for id in ids(cx.db, "SELECT id FROM sessions ORDER BY id", []) {
        let loc = Loc { session: id, window: None, pane: None };
        say(cx, o.val('F').unwrap_or("#{session_name}: #{session_id}"), loc);
    }
    Ok(())
}

fn new_window(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("new-window", args, "adkPSn:c:e:F:t:")?;
    let session = resolve(cx.db, o.val('t'), Kind::Session)?.session;
    let loc = add_window(cx, session, o.val('n'), &o, !o.has('d'))?;
    if o.has('P') {
        say(cx, o.val('F').unwrap_or("#{session_name}:#{window_index}.#{pane_index}"), loc);
    }
    Ok(())
}

fn split_window(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("split-window", args, "bdfhvPIZc:e:F:l:p:t:")?;
    let target = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let (window, at) = (target.window.expect("window"), target.pane.expect("pane"));
    let db = cx.db;
    let fail = |e: rusqlite::Error| e.to_string();
    let (pos, width, height, cwd): (i64, i64, i64, String) = db
        .query_row("SELECT pos, width, height, cwd FROM panes WHERE id = ?1", [at], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(fail)?;
    let pane = next_id(db, "pane").map_err(fail)?;
    let pos = if o.has('b') { pos } else { pos + 1 };
    db.execute("UPDATE panes SET pos = pos + 1 WHERE window = ?1 AND pos >= ?2", (window, pos)).map_err(fail)?;
    let cwd = o.val('c').map_or(cwd, str::to_string);
    let argv = serde_json::to_string(&o.rest).expect("argv json");
    db.execute("INSERT INTO panes VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, ?6, ?7, ?8)", (pane, window, pos, argv, cwd, 10000 + pane, width, height))
        .map_err(fail)?;
    store_env(db, "pane", pane, &o.all('e')).map_err(fail)?;
    if !o.has('d') {
        db.execute("UPDATE windows SET cur_pane = ?1 WHERE id = ?2", (pane, window)).map_err(fail)?;
    }
    if o.has('P') {
        say(cx, o.val('F').unwrap_or("#{session_name}:#{window_index}.#{pane_index}"), Loc { pane: Some(pane), ..target });
    }
    Ok(())
}

fn list_windows(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("list-windows", args, "aF:f:t:")?;
    let windows = if o.has('a') {
        ids(cx.db, "SELECT id FROM windows ORDER BY id", [])
    } else {
        let session = resolve(cx.db, o.val('t'), Kind::Session)?.session;
        ids(cx.db, "SELECT id FROM windows WHERE session = ?1 ORDER BY id", [session])
    };
    for w in windows {
        say(cx, o.val('F').unwrap_or("#{window_index}: #{window_name}"), loc_of_window(cx.db, w));
    }
    Ok(())
}

fn list_panes(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("list-panes", args, "asF:f:t:")?;
    let order = "ORDER BY w.id, p.pos";
    let from = "SELECT p.id FROM panes p JOIN windows w ON w.id = p.window";
    let panes = if o.has('a') {
        ids(cx.db, &format!("{from} {order}"), [])
    } else if o.has('s') {
        let session = resolve(cx.db, o.val('t'), Kind::Session)?.session;
        ids(cx.db, &format!("{from} WHERE w.session = ?1 {order}"), [session])
    } else {
        let window = resolve(cx.db, o.val('t'), Kind::Window)?.window.expect("window");
        ids(cx.db, &format!("{from} WHERE w.id = ?1 {order}"), [window])
    };
    for p in panes {
        say(cx, o.val('F').unwrap_or("#{pane_index}: #{pane_id}"), loc_of_pane(cx.db, p));
    }
    Ok(())
}

fn rename_window(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("rename-window", args, "t:")?;
    let name = o.rest.first().ok_or("usage: rename-window [-t target-window] new-name")?;
    let window = resolve(cx.db, o.val('t'), Kind::Window)?.window.expect("window");
    cx.db.execute("UPDATE windows SET name = ?1 WHERE id = ?2", (name, window)).map_err(|e| e.to_string())?;
    Ok(())
}

fn swap_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("swap-pane", args, "dDUZs:t:")?;
    let a = resolve(cx.db, o.val('s'), Kind::Pane)?;
    let b = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let (a, b) = (a.pane.expect("pane"), b.pane.expect("pane"));
    let place = |p: i64| -> (i64, i64) {
        cx.db.query_row("SELECT window, pos FROM panes WHERE id = ?1", [p], |r| Ok((r.get(0)?, r.get(1)?))).expect("pane row")
    };
    let (pa, pb) = (place(a), place(b));
    let fail = |e: rusqlite::Error| e.to_string();
    for (pane, (window, pos)) in [(a, pb), (b, pa)] {
        cx.db.execute("UPDATE panes SET window = ?1, pos = ?2 WHERE id = ?3", (window, pos, pane)).map_err(fail)?;
    }
    if pa.0 != pb.0 {
        cx.db
            .execute("UPDATE windows SET cur_pane = CASE cur_pane WHEN ?1 THEN ?2 WHEN ?2 THEN ?1 ELSE cur_pane END", (a, b))
            .map_err(fail)?;
    }
    Ok(())
}

/// Only what the tests of this Job need: make the pane current.
fn select_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("select-pane", args, "DdeLlMmRUZPT:t:")?;
    let loc = resolve(cx.db, o.val('t'), Kind::Pane)?;
    cx.db
        .execute("UPDATE windows SET cur_pane = ?1 WHERE id = ?2", (loc.pane, loc.window))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Only what the tests of this Job need: remove the pane (and its window, then its session, when it was
/// the last). No hook fires; ids are not reused.
fn kill_pane(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("kill-pane", args, "at:")?;
    let loc = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let (session, window, pane) = (loc.session, loc.window.expect("window"), loc.pane.expect("pane"));
    let (db, fail) = (cx.db, |e: rusqlite::Error| e.to_string());
    db.execute("DELETE FROM panes WHERE id = ?1", [pane]).map_err(fail)?;
    db.execute("DELETE FROM env WHERE scope = 'pane' AND owner = ?1", [pane]).map_err(fail)?;
    db.execute("DELETE FROM options WHERE scope = 'pane' AND owner = ?1", [pane]).map_err(fail)?;
    let left: Option<i64> = s(db, "SELECT id FROM panes WHERE window = ?1 ORDER BY pos LIMIT 1", [window]);
    if left.is_some() {
        db.execute("UPDATE windows SET cur_pane = ?1 WHERE id = ?2 AND cur_pane = ?3", (left, window, pane)).map_err(fail)?;
        return Ok(());
    }
    db.execute("DELETE FROM windows WHERE id = ?1", [window]).map_err(fail)?;
    db.execute("DELETE FROM options WHERE scope = 'window' AND owner = ?1", [window]).map_err(fail)?;
    let next: Option<i64> = s::<Option<i64>>(db, "SELECT max(id) FROM windows WHERE session = ?1", [session]).flatten();
    if next.is_some() {
        db.execute("UPDATE sessions SET cur_window = ?1 WHERE id = ?2 AND cur_window = ?3", (next, session, window)).map_err(fail)?;
        return Ok(());
    }
    db.execute("DELETE FROM sessions WHERE id = ?1", [session]).map_err(fail)?;
    Ok(())
}

// ---- options and environment ----

/// The scope an option or environment command acts on: `-g`/`-s` the server; `-p` a pane; `-w` a window;
/// else by the target's sigil (`%` pane, `@` window), else the session.
fn option_scope(db: &Connection, o: &Opts) -> R<(&'static str, i64)> {
    if o.has('g') || o.has('s') {
        return Ok(("server", 0));
    }
    let t = o.val('t');
    let sigil = t.and_then(|t| t.chars().next());
    if o.has('p') || (!o.has('w') && sigil == Some('%')) {
        return Ok(("pane", resolve(db, t, Kind::Pane)?.pane.expect("pane")));
    }
    if o.has('w') || sigil == Some('@') {
        return Ok(("window", resolve(db, t, Kind::Window)?.window.expect("window")));
    }
    Ok(("session", resolve(db, t, Kind::Session)?.session))
}

fn set(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("set", args, "agopqsuwFUt:")?;
    let name = o.rest.first().ok_or("usage: set-option [-aFgopqsuUw] [-t target-pane] option [value]")?;
    let (scope, owner) = option_scope(cx.db, &o)?;
    let fail = |e: rusqlite::Error| e.to_string();
    if o.has('u') {
        cx.db.execute("DELETE FROM options WHERE scope = ?1 AND owner = ?2 AND key = ?3", (scope, owner, name)).map_err(fail)?;
        return Ok(());
    }
    let old = option_in(cx.db, scope, owner, name);
    if o.has('o') && old.is_some() {
        return Err(format!("already set: {name}"));
    }
    let given = o.rest.get(1).cloned().unwrap_or_default();
    let value = if o.has('a') { old.unwrap_or_default() + &given } else { given };
    cx.db
        .execute(
            "INSERT INTO options VALUES (?1, ?2, ?3, ?4) ON CONFLICT (scope, owner, key) DO UPDATE SET value = excluded.value",
            (scope, owner, name, value),
        )
        .map_err(fail)?;
    Ok(())
}

fn show(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("show", args, "AgHpqsvwt:")?;
    let (scope, owner) = option_scope(cx.db, &o)?;
    let rows: Vec<(String, String)> = {
        let (sql, name) = match o.rest.first() {
            Some(n) => ("SELECT key, value FROM options WHERE scope = ?1 AND owner = ?2 AND key = ?3 ORDER BY rowid", n.as_str()),
            None => ("SELECT key, value FROM options WHERE scope = ?1 AND owner = ?2 AND ?3 = ?3 ORDER BY rowid", ""),
        };
        let mut st = cx.db.prepare(sql).expect("prepare");
        st.query_map((scope, owner, name), |r| Ok((r.get(0)?, r.get(1)?))).expect("query").map(|r| r.expect("row")).collect()
    };
    if rows.is_empty() {
        return match (o.rest.first(), o.has('q')) {
            (Some(n), false) => Err(format!("invalid option: {n}")),
            _ => Ok(()),
        };
    }
    for (k, v) in rows {
        cx.out.push_str(&if o.has('v') { format!("{v}\n") } else { format!("{k} {v}\n") });
    }
    Ok(())
}

fn environment_scope(db: &Connection, o: &Opts) -> R<(&'static str, i64)> {
    if o.has('g') {
        return Ok(("server", 0));
    }
    Ok(("session", resolve(db, o.val('t'), Kind::Session)?.session))
}

fn set_environment(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("set-environment", args, "Fgrut:")?;
    let name = o.rest.first().ok_or("usage: set-environment [-Fgru] [-t target-session] name [value]")?;
    let (scope, owner) = environment_scope(cx.db, &o)?;
    let fail = |e: rusqlite::Error| e.to_string();
    if o.has('u') {
        cx.db.execute("DELETE FROM env WHERE scope = ?1 AND owner = ?2 AND key = ?3", (scope, owner, name)).map_err(fail)?;
        return Ok(());
    }
    let value = if o.has('r') { None } else { Some(o.rest.get(1).cloned().unwrap_or_default()) };
    cx.db.execute("INSERT OR REPLACE INTO env VALUES (?1, ?2, ?3, ?4)", (scope, owner, name, value)).map_err(fail)?;
    Ok(())
}

fn show_environment(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("show-environment", args, "gst:")?;
    let (scope, owner) = environment_scope(cx.db, &o)?;
    let mut st = cx.db.prepare("SELECT key, value FROM env WHERE scope = ?1 AND owner = ?2 ORDER BY rowid").expect("prepare");
    let rows: Vec<(String, Option<String>)> =
        st.query_map((scope, owner), |r| Ok((r.get(0)?, r.get(1)?))).expect("query").map(|r| r.expect("row")).collect();
    let wanted = o.rest.first();
    if let Some(n) = wanted {
        if !rows.iter().any(|(k, _)| k == n) {
            return Err(format!("unknown variable: {n}"));
        }
    }
    for (k, v) in rows.iter().filter(|(k, _)| wanted.is_none_or(|w| w == k)) {
        match v {
            Some(v) => cx.out.push_str(&format!("{k}={v}\n")),
            None => cx.out.push_str(&format!("-{k}\n")),
        }
    }
    Ok(())
}

fn display_message(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("display-message", args, "aIpvc:d:F:t:N")?;
    let loc = resolve(cx.db, o.val('t'), Kind::Pane)?;
    let fmt = o.rest.first().map(String::as_str).or(o.val('F')).unwrap_or("#{session_name}:#{window_index}.#{pane_index}");
    if o.has('p') {
        say(cx, fmt, loc);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
