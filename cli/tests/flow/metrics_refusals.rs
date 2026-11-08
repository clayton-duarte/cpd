//! `cpd metrics refusals` (commands.md §14.7, Plan `agent-refusals`; air-d85): the read-only
//! aggregate of the `guard` Events (contract §5.2) — the refusal rate per guard, role, model
//! and day over a half-open window, and zero on a store with no refusals.
//!
//! Read-only and machine-wide: one JSON value, the UTC calendar day of each Event, and the
//! rate of every row `count / window.days` rounded to two decimals.

use crate::common::fx::Fx;
use serde_json::json;

/// A fixture store: the Context `ctx`, the Sessions `s1` (worker, S2) and `s2` (lead, O3), and
/// six `guard` Events over two UTC days — three per day.
fn fixture() -> Fx {
    let fx = Fx::with_ports();
    let env = &fx.env;
    env.write_config("version = 1\nmachine = \"m1\"\n");
    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on)
           VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('s1', 'worker', 'ctx', 'W1', '/', 'S2', 'idle', 't', 't'),
                  ('s2', 'lead', 'ctx', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO events (txn, at, kind, context, session, text, data)
           VALUES
             (cpd_txn(), '2026-11-06T09:00:00.000Z', 'guard', 'ctx', 's1', 'φ: usage',
              '{\"code\":\"usage\",\"role\":\"worker\",\"tier\":\"S2\",\"model\":\"p/sonnet\",\"tool\":\"bash\",\"excerpt\":\"cat x\"}'),
             (cpd_txn(), '2026-11-06T10:00:00.000Z', 'guard', 'ctx', 's1', 'φ: usage',
              '{\"code\":\"usage\",\"role\":\"worker\",\"tier\":\"S2\",\"model\":\"p/sonnet\",\"tool\":\"bash\",\"excerpt\":\"cat y\"}'),
             (cpd_txn(), '2026-11-06T11:00:00.000Z', 'guard', 'ctx', 's1', 'φ: network',
              '{\"code\":\"network\",\"role\":\"worker\",\"tier\":\"S2\",\"model\":\"p/sonnet\",\"tool\":\"bash\",\"excerpt\":\"curl x\"}'),
             (cpd_txn(), '2026-11-07T09:00:00.000Z', 'guard', 'ctx', 's2', 'φ: usage',
              '{\"code\":\"usage\",\"role\":\"lead\",\"tier\":\"O3\",\"model\":\"p/opus\",\"tool\":\"bash\",\"excerpt\":\"cat z\"}'),
             (cpd_txn(), '2026-11-07T10:00:00.000Z', 'guard', 'ctx', 's2', 'φ: network',
              '{\"code\":\"network\",\"role\":\"lead\",\"tier\":\"O3\",\"model\":\"p/opus\",\"tool\":\"bash\",\"excerpt\":\"curl z\"}'),
             (cpd_txn(), '2026-11-07T11:00:00.000Z', 'guard', 'ctx', 's1', 'φ: scope',
              '{\"code\":\"scope\",\"role\":\"worker\",\"tier\":\"S2\",\"model\":\"p/sonnet\",\"tool\":\"bash\",\"excerpt\":\"rg /\"}');
        ",
    );
    fx
}

#[test]
fn reports_rate_per_guard_role_model_and_day() {
    let fx = fixture();
    let store = fx.env.cpd_home().join("cpd.db").display().to_string();
    let out = fx
        .cpd(&[
            "metrics",
            "refusals",
            "--since",
            "2026-11-01",
            "--until",
            "2026-11-08",
        ])
        .ok();
    assert_eq!(
        out,
        json!({
            "ok": true,
            "store": store,
            "window": {"since": "2026-11-01T00:00:00Z", "until": "2026-11-08T00:00:00Z", "days": 7.0},
            "total": 6,
            "per_guard": [
                {"guard": "usage", "count": 3, "per_day": 0.43},
                {"guard": "network", "count": 2, "per_day": 0.29},
                {"guard": "scope", "count": 1, "per_day": 0.14}
            ],
            "per_role": [
                {"role": "worker", "count": 4, "per_day": 0.57},
                {"role": "lead", "count": 2, "per_day": 0.29}
            ],
            "per_model": [
                {"model": "p/sonnet", "count": 4, "per_day": 0.57},
                {"model": "p/opus", "count": 2, "per_day": 0.29}
            ],
            "per_day": [
                {"day": "2026-11-06", "count": 3, "per_guard": {"usage": 2, "network": 1}},
                {"day": "2026-11-07", "count": 3, "per_guard": {"usage": 1, "network": 1, "scope": 1}}
            ]
        })
    );
}

#[test]
fn empty_events_reports_zero() {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    let store = fx.env.cpd_home().join("cpd.db").display().to_string();
    let out = fx
        .cpd(&[
            "metrics",
            "refusals",
            "--since",
            "2026-11-01",
            "--until",
            "2026-11-08",
        ])
        .ok();
    assert_eq!(
        out,
        json!({
            "ok": true,
            "store": store,
            "window": {"since": "2026-11-01T00:00:00Z", "until": "2026-11-08T00:00:00Z", "days": 7.0},
            "total": 0,
            "per_guard": [],
            "per_role": [],
            "per_model": [],
            "per_day": []
        })
    );
}
