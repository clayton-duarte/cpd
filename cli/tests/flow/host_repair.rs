//! `cpd host approve` and `cpd host repair` (commands.md §9.11, contract §3.7 E6, hs-d618): the
//! Manager-only approval file and the User-approved repair of the checkout `$HOME/projects/cpd`.
//!
//! The fixture is [`crate::secret::Fx`] (`Who`, the Sessions the role checks need). Every external
//! tool is faked: `git` is a script in the env's `bin/` (hs-d613), so no test touches the real git
//! or a real checkout.

use crate::common::Run;
use crate::secret::{Fx, Who};

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The old HEAD the fake `git rev-parse HEAD` prints before the checkout.
const OLD: &str = "1111111111111111111111111111111111111111";
/// The new HEAD the fake `git rev-parse HEAD` prints after the checkout.
const NEW: &str = "2222222222222222222222222222222222222222";
/// The patch the fake `git diff HEAD` prints when the checkout is dirty.
const PATCH: &str = "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-# cpd\n+# cpd, edited";

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The checkout the repair always works in: `$HOME/projects/cpd`, no argument.
fn checkout(fx: &Fx) -> String {
    fx.env.home().join("projects/cpd").display().to_string()
}

/// `run/` directory of the approval files.
fn approval_dir(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("run/host/approve")
}

/// The backup directory of the saved diffs.
fn backup_dir(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("run/cpd-install-backups")
}

/// A fake `git` on `PATH` (`fx.env.fake`): it logs its argv and answers canned output. `rev-parse
/// HEAD` prints [`OLD`] before the checkout and [`NEW`] after it (a marker the checkout leaves);
/// `diff HEAD` prints `patch`; every other call prints nothing and exits 0. `fail_checkout` makes
/// the `checkout` call exit 1 with a line on stderr.
fn git_fake(fx: &Fx, patch: &str, fail_checkout: bool) {
    let root = fx.env.root();
    let (old, new, patch_file, marker) = (
        root.join("git-old"),
        root.join("git-new"),
        root.join("git-patch"),
        root.join("git-checked-out"),
    );
    let _ = fs::remove_file(&marker);
    fs::write(&old, format!("{OLD}\n")).unwrap();
    fs::write(&new, format!("{NEW}\n")).unwrap();
    fs::write(&patch_file, patch).unwrap();
    let fail = if fail_checkout {
        "echo 'fake git: checkout refused' >&2; exit 1"
    } else {
        ""
    };
    let body = format!(
        "if [ \"$3\" = rev-parse ] && [ \"$4\" = HEAD ]; then\n\
           if [ -f '{marker}' ]; then cat '{new}'; else cat '{old}'; fi\n\
           exit 0\n\
         fi\n\
         if [ \"$3\" = diff ] && [ \"$4\" = HEAD ]; then cat '{patch}'; exit 0; fi\n\
         if [ \"$3\" = checkout ]; then\n\
         {fail}\n\
           : > '{marker}'\n\
           exit 0\n\
         fi\n\
         exit 0\n",
        marker = marker.display(),
        new = new.display(),
        old = old.display(),
        patch = patch_file.display(),
    );
    fx.env.fake("git", &body);
}

fn expect(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// The five git calls of one repair, exactly as commands.md §9.11 names them (the Lead added the
/// fifth `rev-parse HEAD` after the checkout: it is what the output's `head` is). Returns the
/// timestamp `<ts>` and the checkout directory.
fn assert_five_calls(fx: &Fx) -> (String, String) {
    let dir = checkout(fx);
    let calls = fx.env.calls("git");
    assert_eq!(calls.len(), 5, "{calls:?}");
    assert_eq!(
        calls[0],
        expect(&["-C", dir.as_str(), "rev-parse", "HEAD"]),
        "{calls:?}"
    );
    assert_eq!(
        calls[1],
        expect(&["-C", dir.as_str(), "diff", "HEAD"]),
        "{calls:?}"
    );
    assert_eq!(
        calls[3],
        expect(&[
            "-C",
            dir.as_str(),
            "checkout",
            "--force",
            "-B",
            "main",
            "origin/main"
        ]),
        "{calls:?}"
    );
    assert_eq!(
        calls[4], calls[0],
        "the second rev-parse HEAD prints the new head: {calls:?}"
    );
    let update = &calls[2];
    assert_eq!(update[0], "-C", "{calls:?}");
    assert_eq!(update[1], dir, "{calls:?}");
    assert_eq!(update[2], "update-ref", "{calls:?}");
    let ts = update[3]
        .strip_prefix("refs/cpd-host/")
        .unwrap_or_else(|| panic!("the ref is not refs/cpd-host/<ts>: {calls:?}"))
        .to_owned();
    assert_eq!(update[4], OLD, "update-ref saves the old head: {calls:?}");
    (ts, dir)
}

/// Every approval file in `run/host/approve/`, with what it holds.
fn approvals(fx: &Fx) -> Vec<(PathBuf, Value)> {
    let dir = approval_dir(fx);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .map(|e| {
            let path = e.unwrap().path();
            (
                path.clone(),
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap(),
            )
        })
        .collect()
}

/// Writes a private approval file for `session`, dated `at`.
fn write_approval(fx: &Fx, name: &str, session: &str, at: OffsetDateTime) {
    let dir = approval_dir(fx);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join(name);
    fs::write(
        &path,
        json!({"session": session, "at": at.format(&Rfc3339).unwrap()}).to_string(),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Rewrites the `at` of the one approval file in `run/host/approve/` (private, as it was).
fn backdate(fx: &Fx, at: OffsetDateTime) {
    let file = &approvals(fx)[0].0;
    let session =
        serde_json::from_str::<Value>(&fs::read_to_string(file).unwrap()).unwrap()["session"]
            .clone();
    fs::write(
        file,
        json!({"session": session, "at": at.format(&Rfc3339).unwrap()}).to_string(),
    )
    .unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o600)).unwrap();
}

fn host(fx: &Fx, who: Who) -> Run {
    fx.run(who, &["host", "repair"], None)
}

#[test]
fn approve_is_the_managers_and_writes_a_private_ten_minute_approval() {
    let fx = Fx::new();
    git_fake(&fx, "", false);

    let r = fx.run(Who::Manager, &["host", "approve"], None);
    assert_eq!(
        r.ok(),
        json!({"ok": true}),
        "the docs' output is exactly {{ok:true}}: {r:?}"
    );

    let dir = approval_dir(&fx);
    assert_eq!(mode(&dir), 0o700, "{dir:?}");
    let files = approvals(&fx);
    assert_eq!(files.len(), 1, "approve writes exactly one file: {files:?}");
    let (path, held) = &files[0];
    assert_eq!(
        path.extension().and_then(|e| e.to_str()),
        Some("json"),
        "{path:?}"
    );
    assert_eq!(mode(path), 0o600, "{path:?}");
    let mut keys: Vec<&str> = held
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["at", "session"], "{held:?}");
    assert_eq!(held["session"], "mgr1");
    let at = OffsetDateTime::parse(held["at"].as_str().unwrap(), &Rfc3339).unwrap();
    let now = OffsetDateTime::now_utc();
    // The docs: valid 10 minutes. A fresh approval is live now; the expired case is below.
    assert!(
        at <= now && now - at < time::Duration::minutes(1),
        "the approval is fresh: {held:?}"
    );
    assert_eq!(
        fx.env.calls("git"),
        Vec::<Vec<String>>::new(),
        "approve runs no git"
    );
}

#[test]
fn approve_is_refused_to_a_lead_and_a_worker() {
    let fx = Fx::new();
    git_fake(&fx, "", false);
    for who in [Who::Lead, Who::Worker] {
        let r = fx.run(who, &["host", "approve"], None);
        let e = r.error();
        assert_eq!(e["code"], "not_a_manager", "{who:?}: {r:?}");
        assert_eq!(r.exit, 3, "{who:?}: {r:?}");
    }
    assert!(
        approvals(&fx).is_empty(),
        "nothing is written for a non-Manager"
    );
    assert_eq!(
        fx.env.calls("git"),
        Vec::<Vec<String>>::new(),
        "approve runs no git"
    );
}

#[test]
fn repair_without_a_live_approval_is_refused_and_runs_no_git() {
    let fx = Fx::new();
    git_fake(&fx, PATCH, false);

    let r = host(&fx, Who::Manager);
    let e = r.error();
    assert_eq!(e["code"], "host_approval_required", "no approval: {r:?}");
    assert_eq!(r.exit, 3, "{r:?}");

    // No sleeping to expire: an approval dated past its 10 minutes (by 30 s) is dead.
    write_approval(
        &fx,
        "00000000-0000-4000-8000-000000000001.json",
        "mgr1",
        OffsetDateTime::now_utc() - time::Duration::minutes(10) - time::Duration::seconds(30),
    );
    let r = host(&fx, Who::Manager);
    let e = r.error();
    assert_eq!(
        e["code"], "host_approval_required",
        "an expired approval: {r:?}"
    );
    assert_eq!(r.exit, 3, "{r:?}");

    assert_eq!(
        fx.env.calls("git"),
        Vec::<Vec<String>>::new(),
        "no git runs without a live approval"
    );
}

#[test]
fn repair_by_a_manager_with_an_approval_saves_then_resets_main_and_consumes_the_approval() {
    let fx = Fx::new();
    git_fake(&fx, PATCH, false);
    assert_eq!(
        fx.run(Who::Manager, &["host", "approve"], None).ok(),
        json!({"ok": true})
    );
    // Still live, 30 s from its 10-minute expiry: the repair proves the approval lasts ten minutes.
    backdate(
        &fx,
        OffsetDateTime::now_utc() - time::Duration::minutes(10) + time::Duration::seconds(30),
    );

    let r = host(&fx, Who::Manager);
    let out = r.ok();
    let (ts, _dir) = assert_five_calls(&fx);
    assert_eq!(out["saved"]["ref"], format!("refs/cpd-host/{ts}"), "{r:?}");
    let patch = backup_dir(&fx).join(format!("{ts}.patch"));
    assert_eq!(out["saved"]["patch"], patch.display().to_string(), "{r:?}");
    assert_eq!(out["head"], NEW, "{r:?}");
    assert_eq!(fs::read_to_string(&patch).unwrap(), PATCH);
    assert_eq!(mode(&patch), 0o600, "{patch:?}");

    assert!(
        approvals(&fx).is_empty(),
        "the approval is consumed by the repair"
    );
    let calls = fx.env.calls("git").len();
    let r = host(&fx, Who::Manager);
    assert_eq!(
        r.error()["code"],
        "host_approval_required",
        "an approval is one repair: {r:?}"
    );
    assert_eq!(
        fx.env.calls("git").len(),
        calls,
        "the refused repair runs no git"
    );
}

#[test]
fn repair_from_the_user_needs_no_approval() {
    let fx = Fx::new();
    git_fake(&fx, "", false);

    // The User at a terminal has no Session and no approval file.
    let r = host(&fx, Who::Nobody);
    let out = r.ok();
    let (ts, _dir) = assert_five_calls(&fx);
    assert_eq!(out["saved"]["ref"], format!("refs/cpd-host/{ts}"), "{r:?}");
    assert_eq!(out["saved"]["patch"], Value::Null, "{r:?}");
    assert_eq!(out["head"], NEW, "{r:?}");
    assert!(approvals(&fx).is_empty(), "the User needs no approval file");
}

#[test]
fn a_failing_git_step_stops_the_repair_names_the_argv_and_keeps_the_approval() {
    let fx = Fx::new();
    git_fake(&fx, PATCH, true);
    assert_eq!(
        fx.run(Who::Manager, &["host", "approve"], None).ok(),
        json!({"ok": true})
    );

    let r = host(&fx, Who::Manager);
    let e = r.error();
    assert_eq!(e["code"], "git_failed", "{r:?}");
    assert_eq!(r.exit, 1, "{r:?}");
    let dir = checkout(&fx);
    assert_eq!(
        e["details"]["argv"],
        json!([
            "git",
            "-C",
            dir,
            "checkout",
            "--force",
            "-B",
            "main",
            "origin/main"
        ]),
        "{r:?}"
    );

    let calls = fx.env.calls("git");
    assert_eq!(calls.len(), 4, "the checkout is the last call: {calls:?}");
    assert_eq!(
        approvals(&fx).len(),
        1,
        "a failing repair keeps the approval"
    );
}

#[test]
fn a_clean_checkout_writes_no_patch_but_still_saves_the_old_head() {
    let fx = Fx::new();
    git_fake(&fx, "", false);
    assert_eq!(
        fx.run(Who::Manager, &["host", "approve"], None).ok(),
        json!({"ok": true})
    );

    let r = host(&fx, Who::Manager);
    let out = r.ok();
    let (ts, _dir) = assert_five_calls(&fx);
    assert_eq!(out["saved"]["ref"], format!("refs/cpd-host/{ts}"), "{r:?}");
    assert_eq!(
        out["saved"]["patch"],
        Value::Null,
        "a clean diff writes no patch: {r:?}"
    );
    assert_eq!(out["head"], NEW, "{r:?}");
    let backups = fs::read_dir(backup_dir(&fx))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(backups, 0, "a clean checkout leaves no patch file");
    assert!(
        approvals(&fx).is_empty(),
        "the repair consumed the approval"
    );
}

/// Helper that sets up a diverged applied migration in `fx.env`'s store.
fn setup_diverged_store(fx: &Fx) {
    let db_path = fx.env.cpd_home().join("cpd.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function(
        "cpd_txn",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |_| Ok(txn),
    )
    .unwrap();

    let has_waiting_on: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('jobs') WHERE name = 'waiting_on'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    if !has_waiting_on {
        conn.execute_batch("ALTER TABLE jobs ADD COLUMN waiting_on TEXT;")
            .unwrap();
    }

    let diverged_migration = "20261110T2200-draft-waiting-on";
    let mac_pro_checksum = "d0e0badbbd9c0000000000000000000000000000000000000000000000000000";

    let row_exists: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM schema_migrations WHERE name = ?1",
            [diverged_migration],
            |r| r.get(0),
        )
        .unwrap();

    if row_exists {
        conn.execute_batch(
            "DROP TRIGGER IF EXISTS schema_migrations_no_update;
             UPDATE schema_migrations SET checksum = 'd0e0badbbd9c0000000000000000000000000000000000000000000000000000'
             WHERE name = '20261110T2200-draft-waiting-on';
             CREATE TRIGGER schema_migrations_no_update BEFORE UPDATE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;",
        ).unwrap();
    } else {
        conn.execute(
            "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
             VALUES (?1, 'additive', ?2, (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
            rusqlite::params![diverged_migration, mac_pro_checksum, txn],
        )
        .unwrap();
    }
}

#[test]
fn diverged_store_repair_yes_succeeds_and_backs_up() {
    let fx = Fx::new();
    git_fake(&fx, "", false);

    // Set up a diverged store in the test environment
    setup_diverged_store(&fx);

    // Run `cpd host repair --yes` as User (Nobody) through CLI harness
    let r = fx.run(Who::Nobody, &["host", "repair", "--yes"], None);
    let out = r.ok();

    // Asserts command succeeds, output JSON contains store_repaired and store_backup
    assert_eq!(
        out["store_repaired"],
        json!(true),
        "output must indicate store_repaired: {out:?}"
    );
    let backup_str = out["store_backup"]
        .as_str()
        .expect("output must contain store_backup path");
    let backup_path = PathBuf::from(backup_str);
    assert!(
        backup_path.exists(),
        "backup file must exist at {backup_str}"
    );
    assert!(
        backup_str.contains("run/backups/"),
        "backup must be under run/backups/<ts>/cpd.db: {backup_str}"
    );
    assert!(
        backup_path.ends_with("cpd.db"),
        "backup path must end with cpd.db: {backup_str}"
    );

    // Records repair in schema_migration_repairs
    let db_path = fx.env.cpd_home().join("cpd.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let repair_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM schema_migration_repairs WHERE name = '20261110T2200-draft-waiting-on'",
            [],
            |r| r.get(0),
        )
        .expect("schema_migration_repairs must be readable");
    assert_eq!(repair_count, 1, "exactly one repair row must be recorded");
    drop(conn);

    // The store can subsequently be opened normally
    let home = cpd_core::home::CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let store = cpd_core::store::Store::open(&home, 5000);
    assert!(
        store.is_ok(),
        "store must open normally after repair: {:?}",
        store.err()
    );
}

#[test]
fn diverged_store_repair_no_leaves_store_byte_identical() {
    let fx = Fx::new();
    git_fake(&fx, "", false);

    // Set up diverged store
    setup_diverged_store(&fx);

    let db_path = fx.env.cpd_home().join("cpd.db");
    let pre_bytes = fs::read(&db_path).unwrap();

    // Runs `cpd host repair` with 'N' input via stdin
    let r = fx.run(Who::Nobody, &["host", "repair"], Some("N\n"));

    // Asserts command fails with host_approval_required
    assert_eq!(
        r.error()["code"],
        "host_approval_required",
        "repair with N must fail with host_approval_required: {r:?}"
    );

    // Leaves the store byte-identical to before the run
    let post_bytes = fs::read(&db_path).unwrap();
    assert_eq!(
        pre_bytes, post_bytes,
        "store must remain byte-identical after N"
    );
}
