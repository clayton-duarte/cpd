//! Editing the Spec of a queued Locked Job that has no Attempt (contract §3.2, §4.4; commands.md §5.2; ADR-0014).

use serde_json::json;

use super::job::{
    add_touching, count, drafted, engine, events, fixture, job, job_add, job_row, lock,
    locked_fingerprint_of, locked_tdd, spec_file, test_spec_file, touches_fx, write_spec,
};
use crate::common::fx::Fx;
use crate::common::refused;

/// A Spec file whose `goal` and `acceptance` differ from `spec_file`'s.
fn new_spec(fx: &Fx, name: &str) -> String {
    write_spec(&fx.env, name, &json!({"goal": "New goal.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test", "more"], "out_of_scope": ["Everything else."]}).to_string())
}

/// Plan `pl` locked, Jobs `a` and `b` (needs `a`) queued.
fn locked() -> Fx {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    fx
}

fn writes(fx: &Fx) -> (i64, i64, i64) {
    (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "history"),
    )
}

#[test]
fn a_queued_job_with_no_attempt_takes_a_new_spec_and_its_fingerprint_follows() {
    let fx = locked();
    let old = job_row(&fx.env, "pl", "a", "fingerprint");
    let spec = new_spec(&fx, "new.json");
    let out = job(&fx, &["edit", "a", "--spec", &spec], "pl").ok();
    assert_eq!(out["job"], json!({"key": "a", "status": "queued"}), "{out}");
    assert_eq!(out["fields"], json!(["goal", "acceptance"]), "{out}");
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "New goal.");
    assert_eq!(
        job_row(&fx.env, "pl", "a", "acceptance"),
        "[\"cargo test\",\"more\"]"
    );
    // What is Locked is as it was.
    assert_eq!(job_row(&fx.env, "pl", "a", "title"), "Title of a");
    assert_eq!(job_row(&fx.env, "pl", "a", "tier"), "S2");
    assert_eq!(job_row(&fx.env, "pl", "a", "kind"), "work");
    assert_eq!(job_row(&fx.env, "pl", "a", "pr"), "repo");
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "queued");
    assert_eq!(
        fx.env
            .query::<String>("SELECT group_concat(needs) FROM dependencies WHERE job = 'b'"),
        "a"
    );
    // The fingerprint follows the new Spec.
    let new = job_row(&fx.env, "pl", "a", "fingerprint");
    assert_ne!(new, old);
    assert_eq!(new, json!(locked_fingerprint_of(&fx.env, "a")));
    assert_eq!(
        fx.cpd(&["plan", "show", "--plan", "pl"]).ok()["tamper"],
        json!([])
    );
}

#[test]
fn the_edit_is_one_job_edited_event_and_history_keeps_the_old_text() {
    let fx = locked();
    let (events_before, history_before) = (count(&fx.env, "events"), count(&fx.env, "history"));
    let spec = new_spec(&fx, "new.json");
    job(&fx, &["edit", "a", "--spec", &spec], "pl").ok();
    assert_eq!(count(&fx.env, "events"), events_before + 1);
    let edited = events(&fx, "job-edited");
    let last = edited.last().unwrap();
    assert_eq!(edited.len(), 1, "the drafts were only added, never edited");
    assert_eq!(
        (&last["job"], &last["data"]),
        (&json!("a"), &json!({"fields": ["goal", "acceptance"]}))
    );
    assert!(last["text"].as_str().unwrap().contains("Locked"), "{last}");
    assert!(count(&fx.env, "history") > history_before);
    let (before, after): (String, String) = fx.env.read(|c| {
        c.query_row(
            "SELECT json_extract(before, '$.goal'), json_extract(after, '$.goal') FROM history
             WHERE tbl = 'jobs' AND op = 'update' AND json_extract(after, '$.goal') IS NOT NULL ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    });
    assert_eq!((before.as_str(), after.as_str()), ("Do it.", "New goal."));
}

#[test]
fn an_edit_that_changes_nothing_writes_nothing() {
    let fx = locked();
    let spec = new_spec(&fx, "new.json");
    job(&fx, &["edit", "a", "--spec", &spec], "pl").ok();
    let before = writes(&fx);
    let out = job(&fx, &["edit", "a", "--spec", &spec], "pl").ok();
    assert_eq!(out["already"], true, "{out}");
    assert_eq!(out["fields"], json!([]));
    assert_eq!(writes(&fx), before);
    // The same Spec as the Job was locked with, on a Job never edited.
    let same = spec_file(&fx.env);
    let before = writes(&fx);
    assert_eq!(
        job(&fx, &["edit", "b", "--spec", &same], "pl").ok()["already"],
        true
    );
    assert_eq!(writes(&fx), before);
}

#[test]
fn only_the_spec_of_a_queued_job_can_be_edited() {
    let fx = locked();
    let spec = new_spec(&fx, "new.json");
    let before = writes(&fx);
    for flag in [
        &["--title", "Else"][..],
        &["--tier", "S3"],
        &["--pr", "repo"],
        &["--kind", "docs"],
        &["--needs", "a"],
        &["--no-needs"],
    ] {
        let mut args = vec!["edit", "b", "--spec", spec.as_str()];
        args.extend(flag);
        let e = refused(&job(&fx, &args, "pl"), "job_locked", 3);
        assert_eq!(e["details"]["status"], "queued", "{flag:?}");
        assert_eq!(writes(&fx), before, "{flag:?}");
        assert_eq!(job_row(&fx.env, "pl", "b", "goal"), "Do it.", "{flag:?}");
    }
}

#[test]
fn a_job_that_has_an_attempt_or_is_not_queued_stays_locked() {
    let fx = Fx::new();
    drafted(&fx);
    for key in ["c", "d", "e", "f"] {
        job_add(&fx, "pl", key, "S2", &[]).ok();
    }
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "running");
    engine(&fx.env, "pl", "b", "failed");
    engine(&fx.env, "pl", "c", "merged");
    job(&fx, &["cancel", "d", "--reason", "No."], "pl").ok();
    // `e` failed and was queued again: it keeps its Attempt.
    engine(&fx.env, "pl", "e", "failed");
    job(&fx, &["retry", "e", "--tier", "S3"], "pl").ok();
    assert_eq!(job_row(&fx.env, "pl", "e", "status"), "queued");
    let spec = new_spec(&fx, "new.json");
    // `f`, never started, is the one that can be edited: first, so the refusals below are of the Jobs that started.
    job(&fx, &["edit", "f", "--spec", &spec], "pl").ok();
    let before = writes(&fx);
    for (key, status) in [
        ("a", "running"),
        ("b", "failed"),
        ("c", "merged"),
        ("d", "cancelled"),
        ("e", "queued"),
    ] {
        let e = refused(
            &job(&fx, &["edit", key, "--spec", &spec], "pl"),
            "job_locked",
            3,
        );
        assert_eq!(e["details"]["status"], status, "{key}");
        assert!(!e["details"]["attempts"].is_null(), "{key}: {e}");
        assert_eq!(job_row(&fx.env, "pl", key, "goal"), "Do it.", "{key}");
    }
    assert_eq!(writes(&fx), before);
}

#[test]
fn an_open_or_update_job_is_never_edited() {
    let fx = locked();
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pl', 'repo', 'open-x', 'Open it', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, 'open', '{fp}', cpd_txn(), 't'),
                      ('pl', 'repo', 'update-x', 'Update it', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, 'update', '{fp}', cpd_txn(), 't');",
            fp = "f".repeat(64)
        ),
    );
    let spec = new_spec(&fx, "new.json");
    let before = writes(&fx);
    for key in ["open-x", "update-x"] {
        refused(
            &job(&fx, &["edit", key, "--spec", &spec], "pl"),
            "job_locked",
            3,
        );
        assert_eq!(job_row(&fx.env, "pl", key, "goal"), "g", "{key}");
    }
    assert_eq!(writes(&fx), before);
}

#[test]
fn a_worker_cannot_edit_a_locked_job() {
    let fx = locked();
    let spec = new_spec(&fx, "new.json");
    let args = ["job", "edit", "a", "--spec", spec.as_str(), "--plan", "pl"];
    // The Lead's edit works (and is the one that moves the Spec).
    fx.cpd_with(
        fx.env.root(),
        &["job", "edit", "b", "--spec", spec.as_str(), "--plan", "pl"],
        &[],
    )
    .ok();
    let before = writes(&fx);
    refused(
        &fx.cpd_with(fx.env.root(), &args, &[("CPD_ATTEMPT", "a-a1")]),
        "not_a_lead",
        3,
    );
    assert_eq!(writes(&fx), before);
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "Do it.");
    fx.cpd_with(fx.env.root(), &args, &[]).ok();
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "New goal.");
}

#[test]
fn the_new_spec_is_checked_as_a_drafts_is() {
    let fx = touches_fx();
    add_touching(&fx, "a", &["src/a.rs"]).ok();
    lock(&fx.env, "pl");
    // A new valid Spec is taken, so the refusals below are the checks and not the lock.
    let good = new_spec(&fx, "good.json");
    job(&fx, &["edit", "a", "--spec", &good], "pl").ok();
    let before = writes(&fx);
    let unknown = write_spec(&fx.env, "unknown.json", &json!({"goal": "g", "context": "c", "touches": ["src/a.rs"], "acceptance": ["t"], "out_of_scope": [], "bogus": 1}).to_string());
    refused(
        &job(&fx, &["edit", "a", "--spec", &unknown], "pl"),
        "spec_invalid",
        2,
    );
    let missing = write_spec(&fx.env, "missing.json", &json!({"goal": "g2", "context": "c", "touches": ["nope/x.rs"], "acceptance": ["t"], "out_of_scope": []}).to_string());
    let e = refused(
        &job(&fx, &["edit", "a", "--spec", &missing], "pl"),
        "touches_missing",
        3,
    );
    assert_eq!(e["details"]["missing"], json!(["nope/x.rs"]), "{e}");
    assert_eq!(writes(&fx), before);
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "New goal.");
    drop(fx);

    // A `test` Job keeps `tests` and `test_files`.
    let fx = Fx::new();
    locked_tdd(&fx);
    lock(&fx.env, "pl");
    let plain = new_spec(&fx, "plain.json");
    refused(
        &job(&fx, &["edit", "t", "--spec", &plain], "pl"),
        "spec_invalid",
        2,
    );
    let tests = test_spec_file(
        &fx.env,
        "t2.json",
        json!(["t::two"]),
        json!(["tests/t2.rs"]),
    );
    job(&fx, &["edit", "t", "--spec", &tests], "pl").ok();
    assert_eq!(job_row(&fx.env, "pl", "t", "tests"), "[\"t::two\"]");
    assert_eq!(
        job_row(&fx.env, "pl", "t", "test_files"),
        "[\"tests/t2.rs\"]"
    );
}
