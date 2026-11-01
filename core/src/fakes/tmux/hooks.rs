//! Hooks and bindings (docs/implementing.md, Tests, "The fake tmux"): `set-hook`, `show-hooks`, `bind`,
//! `list-keys`, `run-shell`, `if -F`, and the interpreter that runs a hook's command string.
//!
//! A hook fires inside the call that causes it. The interpreter is `dispatch` itself (every command of the
//! fake is available to a hook) after a small tokenizer and a format expansion of every word. A hook fired
//! while a hook runs is queued and drained before the outer call returns; the drain is capped.

use super::target::{resolve, Kind, Loc};
use super::{dispatch, format, variable, Cx, Opts, R};
use std::process::{Command, Stdio};
use std::time::Duration;

/// A hook waiting to run: its name, its target and the format variables of the event.
pub struct Pending {
    name: String,
    loc: Loc,
    vars: Vec<(String, String)>,
}

/// A hook that keeps firing itself is cut off here.
const DRAIN_LIMIT: usize = 64;

/// Fires the hook `name` with `loc` as target and `vars` as extra format variables. At the top it runs
/// the hook and drains what the hooks fired; inside a hook it only queues.
pub(super) fn fire(cx: &mut Cx, name: &str, loc: Loc, vars: Vec<(String, String)>) {
    cx.queue.push(Pending { name: name.to_string(), loc, vars });
    if cx.depth > 0 {
        return;
    }
    cx.depth = 1;
    let mut ran = 0;
    while ran < DRAIN_LIMIT && !cx.queue.is_empty() {
        let p = cx.queue.remove(0);
        ran += 1;
        let cmd: Option<String> = super::s(cx.db, "SELECT cmd FROM hooks WHERE name = ?1", [&p.name]);
        if let Some(cmd) = cmd {
            // As in tmux, a failing hook command does not fail the command that fired it.
            let _ = run_string(cx, &cmd, p.loc, &p.vars);
        }
    }
    cx.queue.clear();
    cx.depth = 0;
}

/// Splits a tmux command string into commands (on `;`) of words. Quotes group (`\"` inside a double-quoted word is a quote); a quote closes only before
/// whitespace, `;` or the end, so `'run-shell 'echo x''` is one word holding `run-shell 'echo x'`.
fn tokenize(src: &str) -> R<Vec<Vec<String>>> {
    let chars: Vec<char> = src.chars().collect();
    let closes = |j: usize| chars.get(j).is_none_or(|c| c.is_whitespace() || *c == ';');
    let (mut cmds, mut cur, mut word, mut in_word, mut i) = (Vec::new(), Vec::new(), String::new(), false, 0);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                in_word = true;
                i += 1;
                while i < chars.len() && !(chars[i] == c && closes(i + 1)) {
                    // As in tmux, `\"` in a double-quoted word is a quote of the word's own (the nested command of a hook).
                    if c == '"' && chars[i] == '\\' && chars.get(i + 1) == Some(&'"') {
                        i += 1;
                    }
                    word.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(format!("unterminated quote in: {src}"));
                }
            }
            '\\' if i + 1 < chars.len() => {
                in_word = true;
                i += 1;
                word.push(chars[i]);
            }
            c if c.is_whitespace() || c == ';' => {
                if in_word {
                    cur.push(std::mem::take(&mut word));
                    in_word = false;
                }
                if c == ';' && !cur.is_empty() {
                    cmds.push(std::mem::take(&mut cur));
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
        i += 1;
    }
    if in_word {
        cur.push(word);
    }
    if !cur.is_empty() {
        cmds.push(cur);
    }
    Ok(cmds)
}

fn expand(cx: &Cx, loc: Loc, w: &str) -> String {
    format::expand(w, &|n| variable(cx, loc, n))
}

/// Runs a command string with `loc` as the target of commands that name none and `vars` as extra format
/// variables; every word is format-expanded first. Stops at the first failing command.
pub(super) fn run_string(cx: &mut Cx, src: &str, loc: Loc, vars: &[(String, String)]) -> R<()> {
    let cmds = tokenize(src)?;
    let (saved_loc, saved_vars) = (cx.loc, cx.vars.len());
    cx.loc = Some(loc);
    cx.vars.extend(vars.iter().cloned());
    let mut result = Ok(());
    for c in cmds {
        let words: Vec<String> = c.iter().map(|w| expand(cx, loc, w)).collect();
        result = dispatch(cx, &words);
        if result.is_err() {
            break;
        }
    }
    cx.loc = saved_loc;
    cx.vars.truncate(saved_vars);
    result
}

/// The target a command string runs against: the hook's, else the default pane.
pub(super) fn current_loc(cx: &Cx) -> Loc {
    cx.loc
        .or_else(|| resolve(cx.db, None, Kind::Pane).ok())
        .unwrap_or(Loc { session: -1, window: None, pane: None })
}

/// `if -F format then [else]`. The shell form of `if-shell` is not modelled.
pub(super) fn if_cmd(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("if", args, "bFnt:")?;
    if !o.has('F') {
        return Err("if: only -F is supported by the fake tmux".into());
    }
    let [cond, then, rest @ ..] = &o.rest[..] else { return Err("usage: if [-bF] [-t target-pane] shell-command command [command]".into()) };
    let loc = current_loc(cx);
    let branch = if format::truthy(&expand(cx, loc, cond)) { Some(then) } else { rest.first() };
    match branch {
        Some(b) => run_string(cx, b, loc, &[]),
        None => Ok(()),
    }
}

/// How long a `run-shell` command may run: the fake waits for it (`-b` too), the real tmux does not.
const RUN_SHELL_DEADLINE: Duration = Duration::from_secs(60);

/// `run-shell [-b] command`: `/bin/sh -c command`, handed to the `Runner` with the caller's directory and
/// environment, and the call goes on when it ends (`-b` is the same; no thread, no sleep). The child may
/// call the fake again, so the transaction is committed before it starts and a new one opened after it
/// ends, otherwise the child's call would wait on the lock. Its output, its exit status and a `Runner`
/// that could not run it are not modelled: tmux ignores them too.
pub(super) fn run_shell(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("run-shell", args, "bCEc:d:t:")?;
    let cmd = o.rest.first().ok_or("usage: run-shell [-bC] [-c start-directory] [-d delay] [-t target-pane] [shell-command]")?;
    let runner = cx.runner.ok_or("run-shell: this fake tmux has no Runner (FakeTmux::with_runner)")?;
    let fail = |e: rusqlite::Error| e.to_string();
    cx.db.execute_batch("COMMIT").map_err(fail)?;
    let mut sh = Command::new("/bin/sh");
    sh.arg("-c")
        .arg(cmd)
        .current_dir(&cx.inv.cwd)
        .env_clear()
        .envs(cx.inv.env.iter().cloned())
        .stdin(Stdio::null());
    let _ = runner.output(&mut sh, "sh", RUN_SHELL_DEADLINE);
    cx.db.execute_batch("BEGIN IMMEDIATE").map_err(fail)
}

pub(super) fn set_hook(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("set-hook", args, "agpRuwt:")?;
    let name = o.rest.first().ok_or("usage: set-hook [-agpRuw] [-t target-pane] hook [command]")?;
    let fail = |e: rusqlite::Error| e.to_string();
    if o.has('u') {
        cx.db.execute("DELETE FROM hooks WHERE name = ?1", [name]).map_err(fail)?;
        return Ok(());
    }
    let mut cmd = o.rest.get(1).cloned().ok_or("set-hook: command expected")?;
    if o.has('a')
        && let Some(old) = super::s::<String>(cx.db, "SELECT cmd FROM hooks WHERE name = ?1", [name]) {
            cmd = format!("{old} ; {cmd}");
        }
    cx.db
        .execute("INSERT INTO hooks (name, cmd) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET cmd = excluded.cmd", (name, cmd))
        .map_err(fail)?;
    Ok(())
}

pub(super) fn show_hooks(cx: &mut Cx, args: &[String]) -> R<()> {
    Opts::parse("show-hooks", args, "gpwt:")?;
    let mut st = cx.db.prepare("SELECT name, cmd FROM hooks ORDER BY rowid").expect("prepare");
    let rows: Vec<(String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).expect("query").map(|r| r.expect("row")).collect();
    for (name, cmd) in rows {
        cx.out.push_str(&format!("{name} {cmd}\n"));
    }
    Ok(())
}

/// `bind [-nr] [-T table] key command...`; `-n` is the root table.
pub(super) fn bind(cx: &mut Cx, args: &[String]) -> R<()> {
    let o = Opts::parse("bind", args, "nrN:T:")?;
    let [key, cmd @ ..] = &o.rest[..] else { return Err("usage: bind-key [-nr] [-T key-table] key [command [argument ...]]".into()) };
    let table = if o.has('n') { "root" } else { o.val('T').unwrap_or("prefix") };
    cx.db
        .execute(
            "INSERT INTO bindings (key, tbl, cmd) VALUES (?1, ?2, ?3) ON CONFLICT (key) DO UPDATE SET tbl = excluded.tbl, cmd = excluded.cmd",
            (key, table, cmd.join(" ")),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn list_keys(cx: &mut Cx, args: &[String]) -> R<()> {
    Opts::parse("list-keys", args, "1aNP:T:")?;
    let mut st = cx.db.prepare("SELECT key, tbl, cmd FROM bindings ORDER BY rowid").expect("prepare");
    let rows: Vec<(String, String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).expect("query").map(|r| r.expect("row")).collect();
    for (key, table, cmd) in rows {
        cx.out.push_str(&format!("bind-key -T {table} {key} {cmd}\n"));
    }
    Ok(())
}
