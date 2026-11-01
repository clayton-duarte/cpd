//! `__ctl`, the test-only control command: `__ctl pane-exit %p --status n` (or `--signal`: the program was killed, so
//! `#{pane_dead_status}` is empty), `__ctl screen %p text`,
//! `__ctl click status <range>`. `run` accepts it only as the first command of a call.

use super::hooks::{current_loc, fire, run_string};
use super::target::{resolve, Kind};
use super::{loc_of_pane, Cx, R};

fn pane_of(cx: &Cx, args: &[String]) -> R<i64> {
    let target = args.first().ok_or("__ctl: pane expected")?;
    Ok(resolve(cx.db, Some(target), Kind::Pane)?.pane.expect("pane"))
}

pub(super) fn ctl(cx: &mut Cx, args: &[String]) -> R<()> {
    let fail = |e: rusqlite::Error| e.to_string();
    let Some((sub, rest)) = args.split_first() else { return Err("__ctl: subcommand expected".into()) };
    match sub.as_str() {
        "pane-exit" => {
            let pane = pane_of(cx, rest)?;
            let status = match &rest[1..] {
                [flag, n] if flag == "--status" => Some(n.parse::<i64>().map_err(|_| format!("__ctl: bad status: {n}"))?),
                // Killed by a signal: tmux has no exit status to print.
                [flag] if flag == "--signal" => None,
                _ => return Err("usage: __ctl pane-exit <pane> (--status <n> | --signal)".into()),
            };
            cx.db.execute("UPDATE panes SET dead = 1, dead_status = ?1 WHERE id = ?2", (status, pane)).map_err(fail)?;
            let loc = loc_of_pane(cx.db, pane);
            fire(cx, "pane-died", loc, Vec::new());
            Ok(())
        }
        "screen" => {
            let pane = pane_of(cx, rest)?;
            let text = rest[1..].join(" ");
            cx.db
                .execute(
                    "INSERT INTO screens (pane, text) VALUES (?1, ?2) ON CONFLICT (pane) DO UPDATE SET text = excluded.text",
                    (pane, text),
                )
                .map_err(fail)?;
            Ok(())
        }
        "click" => {
            let [place, range] = rest else { return Err("usage: __ctl click status <range>".into()) };
            if place != "status" {
                return Err(format!("__ctl: unknown click place: {place}"));
            }
            let cmd: Option<String> = super::s(cx.db, "SELECT cmd FROM bindings WHERE key = 'MouseDown1Status'", []);
            let Some(cmd) = cmd else { return Ok(()) };
            let loc = current_loc(cx);
            run_string(cx, &cmd, loc, &[("mouse_status_range".into(), range.clone())])
        }
        other => Err(format!("__ctl: unknown subcommand: {other}")),
    }
}
