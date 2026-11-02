//! Engine step E22, Permanent Contexts (contract §3.7 step E22, §2.2 "Permanent Contexts"; Context records pro-d338 to
//! pro-d342): every tick ensures the Contexts `cpd`, `host` and `codereview` and a live Manager for each. In-process on a
//! prepared database on virtual time, fake tmux and fake spawn only. Literals only: the tests pin the docs, not the API.

use crate::common;

use common::fx::Fx as Ip;
use serde_json::Value;

/// Ticks of the engine for "a later tick writes nothing more".
const LATER_TICKS: usize = 5;

/// The three Permanent Contexts.
const PERMANENT: [&str; 3] = ["cpd", "host", "codereview"];

/// The engine in-process on virtual time over an empty store.
fn fx() -> Ip {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("");
    env.engine_routes_on_tmux();
    env
}

/// A live pane running `sleep`, for an `idle` Manager.
fn pane(fx: &Ip) -> String {
    let out = fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "x",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn column(fx: &Ip, sql: &str) -> Vec<String> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare(sql)?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
    })
}

fn n(fx: &Ip, sql: &str) -> i64 {
    fx.env.query(sql)
}

/// Every Context id, sorted.
fn contexts(fx: &Ip) -> Vec<String> {
    column(fx, "SELECT id FROM contexts ORDER BY id")
}

/// The live (not archived) Manager Sessions of Context `ctx`: (id, state).
fn live_managers(fx: &Ip, ctx: &str) -> Vec<(String, String)> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, state FROM sessions WHERE context = ?1 AND role = 'manager' AND plan IS NULL AND archived_at IS NULL ORDER BY id",
        )?;
        stmt.query_map([ctx], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()
    })
}

/// Events of `kind` of Context `ctx`.
fn of_context(fx: &Ip, kind: &str, ctx: &str) -> i64 {
    n(
        fx,
        &format!("SELECT count(*) FROM events WHERE kind = '{kind}' AND context = '{ctx}'"),
    )
}

/// Every Event but the engine's own `watch-started` and `watch-stopped`.
fn events(fx: &Ip) -> i64 {
    n(
        fx,
        "SELECT count(*) FROM events WHERE kind NOT IN ('watch-started', 'watch-stopped')",
    )
}

/// Every `error` Event: (text, data).
fn errors(fx: &Ip) -> Vec<(String, Value)> {
    fx.env.read(|conn| {
        let mut stmt =
            conn.prepare("SELECT text, data FROM events WHERE kind = 'error' ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(t, d)| (t, serde_json::from_str(&d).unwrap()))
            .collect())
    })
}

/// A Context row `id` and a live Manager `m_<id>` in `state` (`asleep`: with `asleep_at` set), as another command left them.
fn seeded(fx: &Ip, id: &str, state: &str, asleep: bool) {
    let (col, val) = if asleep {
        (", asleep_at", ", 't'")
    } else {
        ("", "")
    };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('{id}', '{id}', 'clayton-duarte', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at{col})
           VALUES ('m_{id}', 'manager', '{id}', 'Manager', '/', 'O3', '{state}', 't', 't'{val});"
    ));
}

#[test]
fn a_tick_creates_the_three_permanent_contexts_and_their_stopped_managers() {
    let fx = fx();
    fx.watch_ticks(1).unwrap();

    assert_eq!(
        contexts(&fx),
        ["codereview", "cpd", "host"],
        "the three Contexts and no other"
    );
    assert_eq!(
        n(
            &fx,
            "SELECT count(*) FROM contexts WHERE owner = 'clayton-duarte' AND archived_at IS NULL AND deleted_at IS NULL"
        ),
        3
    );
    assert_eq!(
        column(&fx, "SELECT title FROM contexts WHERE id = 'host'"),
        ["Host: local maintenance of this machine"]
    );
    for ctx in PERMANENT {
        let managers = live_managers(&fx, ctx);
        assert_eq!(managers.len(), 1, "one live Manager of {ctx}: {managers:?}");
        assert_eq!(managers[0].1, "stopped", "{ctx}");
        assert_eq!(of_context(&fx, "context-created", ctx), 1, "{ctx}");
        assert_eq!(of_context(&fx, "session-registered", ctx), 1, "{ctx}");
    }
    assert_eq!(n(&fx, "SELECT count(*) FROM sessions"), 3);
}

#[test]
fn a_tick_unarchives_an_archived_permanent_context_and_gives_it_a_manager() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on, archived_at) VALUES ('host', 'Host: local maintenance of this machine', 'clayton-duarte', 't', 'm1', 't');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('oldm', 'manager', 'host', 'Manager', '/', 'O3', 'stopped', 't', 't', 't'),
                  ('oldl', 'lead', 'host', 'Lead', '/', 'O3', 'stopped', 't', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, archived_at)
           VALUES ('oldp', 'host', 'Old', 'grounding', 'oldl', 't', 't');",
    );
    fx.watch_ticks(1).unwrap();

    assert_eq!(
        n(
            &fx,
            "SELECT count(*) FROM contexts WHERE id = 'host' AND archived_at IS NULL"
        ),
        1
    );
    assert_eq!(
        n(
            &fx,
            "SELECT count(*) FROM plans WHERE id = 'oldp' AND archived_at IS NOT NULL"
        ),
        1,
        "the Plan stays archived"
    );
    assert_eq!(
        n(
            &fx,
            "SELECT count(*) FROM sessions WHERE id IN ('oldm', 'oldl') AND archived_at IS NOT NULL"
        ),
        2,
        "the old Sessions stay archived"
    );
    let managers = live_managers(&fx, "host");
    assert_eq!(managers.len(), 1, "{managers:?}");
    assert_ne!(managers[0].0, "oldm");
    assert_eq!(managers[0].1, "stopped");
}

/// What a later tick must not change: counts of Events, Sessions and Contexts, and the Manager ids.
fn snapshot(fx: &Ip) -> (i64, i64, i64, Vec<String>) {
    (
        events(fx),
        n(fx, "SELECT count(*) FROM sessions"),
        n(fx, "SELECT count(*) FROM contexts"),
        column(
            fx,
            "SELECT id FROM sessions WHERE role = 'manager' ORDER BY id",
        ),
    )
}

#[test]
fn a_later_tick_writes_nothing_more() {
    let fx = fx();
    fx.watch_ticks(1).unwrap();
    let first = snapshot(&fx);
    assert_eq!(first.2, 3, "the first tick creates the three Contexts");
    assert_eq!(first.3.len(), 3, "and their Managers");

    fx.watch_ticks(LATER_TICKS).unwrap();
    assert_eq!(snapshot(&fx), first);
}

/// Not in the red list: passes before the implementation.
#[test]
fn a_tick_writes_nothing_when_the_three_exist_with_a_live_manager() {
    let fx = fx();
    let p = pane(&fx);
    seeded(&fx, "cpd", "stopped", false);
    seeded(&fx, "host", "idle", false);
    fx.env.sql(&format!(
        "UPDATE sessions SET tmux_pane = '{p}' WHERE id = 'm_host';"
    ));
    seeded(&fx, "codereview", "stopped", true);
    let before = snapshot(&fx);
    fx.watch_ticks(1).unwrap();
    assert_eq!(snapshot(&fx), before);
}

#[test]
fn a_failed_ensure_raises_one_error_event_and_the_tick_goes_on() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on, deleted_at) VALUES ('codereview', 'codereview', 'clayton-duarte', 't', 'm1', 't');",
    );
    fx.watch_ticks(1).unwrap();

    for ctx in ["cpd", "host"] {
        assert_eq!(live_managers(&fx, ctx).len(), 1, "{ctx} is still ensured");
    }
    let failed: Vec<_> = errors(&fx)
        .into_iter()
        .filter(|(t, _)| t.contains("E22 (Permanent Contexts)"))
        .collect();
    assert_eq!(failed.len(), 1, "one error Event: {failed:?}");
    assert_eq!(failed[0].1["code"], "context_exists", "{:?}", failed[0].1);
    assert_eq!(errors(&fx).len(), 1, "{:?}", errors(&fx));

    // Once until it clears: the failure stands, no second Event.
    fx.watch_ticks(1).unwrap();
    assert_eq!(errors(&fx).len(), 1, "{:?}", errors(&fx));
}
