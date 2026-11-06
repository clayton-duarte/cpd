//! `cpd swr list|show|invalidate|endpoints|mutation show` (commands.md §32; docs/architecture/swr.md §13): every command
//! prints one JSON value, reads the `swr_*` tables and never reaches upstream. The rows are fixtures; the command's
//! clock is the `ManualClock` of `Fx::with_ports` (2026-01-01T00:00:00.000Z).

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::json;

fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx
}

const FETCHED: &str = "2025-12-31T23:59:18.000Z";

fn queries(fx: &Fx) {
    fx.env.sql(&format!(
        "INSERT INTO swr_queries (key, system, status, data_hash, fetched_at, stale_after, invalidated, error_count)
           VALUES ('github:pr:owner/repo#12', 'github', 'success', 'h1', '{FETCHED}', '2026-01-01T00:00:05.000Z', 0, 0);
         INSERT INTO swr_queries (key, system, status, invalidated, error_count, last_error)
           VALUES ('slack:channel:C1', 'slack', 'error', 1, 2, 'rate limited');"
    ));
}

#[test]
fn swr_cmd_list_shows_queries_with_status_and_age() {
    let fx = fx();
    queries(&fx);

    let all = fx.cpd(&["swr", "list"]).ok();
    assert_eq!(
        all["queries"],
        json!([
            {"key": "github:pr:owner/repo#12", "system": "github", "status": "success", "fetched_at": FETCHED,
             "stale_after": "2026-01-01T00:00:05.000Z", "invalidated": false, "error_count": 0, "last_error": null,
             "inactive_at": null, "age_seconds": 42},
            {"key": "slack:channel:C1", "system": "slack", "status": "error", "fetched_at": null,
             "stale_after": null, "invalidated": true, "error_count": 2, "last_error": "rate limited",
             "inactive_at": null, "age_seconds": null}
        ]),
        "{all}"
    );

    let slack = fx.cpd(&["swr", "list", "--system", "slack"]).ok();
    assert_eq!(slack["queries"].as_array().unwrap().len(), 1, "{slack}");
    assert_eq!(slack["queries"][0]["key"], "slack:channel:C1");

    refused(&fx.cpd(&["swr", "list", "--system", "nope"]), "usage", 2);
}

#[test]
fn swr_cmd_invalidate_marks_the_query() {
    let fx = fx();
    queries(&fx);

    let out = fx
        .cpd(&["swr", "invalidate", "github:pr:owner/repo#12"])
        .ok();
    assert_eq!(
        out,
        json!({"ok": true, "key": "github:pr:owner/repo#12", "invalidated": true, "already": false})
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT invalidated FROM swr_queries WHERE key = 'github:pr:owner/repo#12'"
        ),
        1
    );

    let again = fx
        .cpd(&["swr", "invalidate", "github:pr:owner/repo#12"])
        .ok();
    assert_eq!(again["already"], true, "{again}");

    let error = refused(
        &fx.cpd(&["swr", "invalidate", "github:pr:owner/repo#99"]),
        "query_not_found",
        4,
    );
    assert_eq!(error["details"]["key"], "github:pr:owner/repo#99");
    refused(&fx.cpd(&["swr", "invalidate", "not-a-key"]), "usage", 2);
}

#[test]
fn swr_cmd_endpoints_lists_health() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO swr_endpoints (endpoint, status, fetch_status, paused_until, last_ok_at, rate_remaining, rate_reset_at)
           VALUES ('github', 'success', 'paused', '2026-01-01T00:10:00.000Z', '2025-12-31T23:59:00.000Z', 0, '2026-01-01T00:10:00.000Z');
         INSERT INTO swr_endpoints (endpoint, status, failing_since, last_error, error_count)
           VALUES ('bifrost', 'error', '2025-12-31T23:50:00.000Z', 'connection refused', 3);",
    );

    let out = fx.cpd(&["swr", "endpoints"]).ok();
    let blank = |endpoint: &str| {
        json!({"endpoint": endpoint, "status": "pending", "fetch_status": "idle", "paused_until": null,
               "failing_since": null, "reported_at": null, "last_error": null, "error_count": 0,
               "last_ok_at": null, "rate_remaining": null, "rate_reset_at": null})
    };
    assert_eq!(
        out["endpoints"],
        json!([
            {"endpoint": "github", "status": "success", "fetch_status": "paused",
             "paused_until": "2026-01-01T00:10:00.000Z", "failing_since": null, "reported_at": null,
             "last_error": null, "error_count": 0, "last_ok_at": "2025-12-31T23:59:00.000Z",
             "rate_remaining": 0, "rate_reset_at": "2026-01-01T00:10:00.000Z"},
            blank("slack"),
            blank("jira"),
            blank("ollama.lab"),
            {"endpoint": "bifrost", "status": "error", "fetch_status": "idle", "paused_until": null,
             "failing_since": "2025-12-31T23:50:00.000Z", "reported_at": null,
             "last_error": "connection refused", "error_count": 3, "last_ok_at": null,
             "rate_remaining": null, "rate_reset_at": null}
        ]),
        "every endpoint in the documented order, a missing row with the column defaults: {out}"
    );
}

#[test]
fn swr_cmd_mutation_show_prints_status() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO swr_mutations (id, system, target_key, op_kind, vars_json, patch_json, status, submitted_at,
                                    started_at, completed_at, attempt_count, context_json)
           VALUES (7, 'github', 'github:pr:owner/repo#12', 'merge', '{\"method\":\"squash\"}', '{\"state\":\"merged\"}',
                   'revalidating', '2025-12-31T23:58:00.000Z', '2025-12-31T23:58:02.000Z', '2025-12-31T23:58:09.000Z', 1,
                   '{\"plan\":\"p1\",\"workflow\":\"w1\"}');",
    );

    let out = fx.cpd(&["swr", "mutation", "show", "7"]).ok();
    assert_eq!(
        out["mutation"],
        json!({"id": 7, "system": "github", "target_key": "github:pr:owner/repo#12", "op_kind": "merge",
               "status": "revalidating", "vars": {"method": "squash"}, "patch": {"state": "merged"},
               "context": {"plan": "p1", "workflow": "w1"},
               "submitted_at": "2025-12-31T23:58:00.000Z", "started_at": "2025-12-31T23:58:02.000Z",
               "completed_at": "2025-12-31T23:58:09.000Z", "next_attempt_at": null, "attempt_count": 1,
               "error": null}),
        "{out}"
    );

    let error = refused(
        &fx.cpd(&["swr", "mutation", "show", "99"]),
        "mutation_not_found",
        4,
    );
    assert_eq!(error["details"]["id"], 99);
    refused(&fx.cpd(&["swr", "mutation", "show", "0"]), "usage", 2);
}
