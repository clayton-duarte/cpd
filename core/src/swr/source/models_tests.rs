//! The models datasource on SQLite (docs/architecture/swr-models.md §2, §5): rows written from a validated answer,
//! `show` and the inbox feed read them. A temp store; time is passed in from a `ManualClock`.

use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::models::{apply_answer, changed, show};
use crate::clock::{Clock, ManualClock, timestamp};
use crate::home::CpdHome;
use crate::models::{Answer, Entry, Listed};
use crate::store::Store;

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn listed(provider: &str, ids: &[&str]) -> Entry {
    Entry::Listed(Listed {
        provider: provider.into(),
        auth: "oauth".into(),
        base: None,
        ids: ids.iter().map(|i| (*i).to_owned()).collect(),
        known: None,
    })
}

fn answer(entries: Vec<Entry>) -> Answer {
    Answer {
        pi_version: "1.3.0".into(),
        entries,
    }
}

fn after(clock: &ManualClock, secs: u64) -> OffsetDateTime {
    clock.advance(Duration::from_secs(secs));
    clock.utc()
}

fn text(store: &mut Store, sql: &str) -> Option<String> {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, Option<String>>(0))?))
        .unwrap()
}

fn int(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn models_refresh_writes_swr_models_and_no_json_file() {
    let (_dir, home, mut store) = store();
    let now = ManualClock::new().utc();
    apply_answer(
        &mut store,
        &answer(vec![listed("zeta", &["b", "a"]), listed("alpha", &["m"])]),
        now,
    )
    .unwrap();

    assert_eq!(int(&mut store, "SELECT count(*) FROM swr_models"), 2);
    let catalog: Value = serde_json::from_str(
        &text(
            &mut store,
            "SELECT catalog_json FROM swr_models WHERE provider = 'zeta'",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(catalog["ids"], json!(["a", "b"]), "ids are sorted");
    assert_eq!(catalog["pi_version"], "1.3.0");
    assert_eq!(catalog["source"]["auth"], "oauth");
    assert_eq!(
        text(
            &mut store,
            "SELECT polled_at FROM swr_models WHERE provider = 'zeta'"
        ),
        Some(timestamp(now))
    );
    assert_eq!(
        text(
            &mut store,
            "SELECT changes_json FROM swr_models WHERE provider = 'zeta'"
        ),
        None,
        "a first write announces nothing"
    );
    let files: Vec<_> = std::fs::read_dir(home.run_dir())
        .map(|d| d.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(
        files
            .iter()
            .all(|f| !f.to_string_lossy().starts_with("models-")),
        "no run/models-<provider>.json: {files:?}"
    );
}

#[test]
fn models_show_reads_swr_models() {
    let (_dir, _home, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    apply_answer(
        &mut store,
        &answer(vec![listed("zeta", &["b", "a"]), listed("alpha", &["m"])]),
        t0,
    )
    .unwrap();
    let txns = int(&mut store, "SELECT count(*) FROM txns");

    let out = show(&mut store, None, after(&clock, 90)).unwrap();
    let providers = out["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2);
    let zeta = providers.iter().find(|p| p["provider"] == "zeta").unwrap();
    assert_eq!(zeta["ids"], json!(["a", "b"]));
    assert_eq!(zeta["count"], 2);
    assert_eq!(zeta["auth"], "oauth");
    assert_eq!(zeta["pi_version"], "1.3.0");
    assert_eq!(zeta["age_seconds"], 90);
    assert_eq!(zeta["polled_at"], timestamp(t0));
    assert!(zeta["changes"].is_null());
    assert_eq!(out["warnings"], json!([]));

    let one = show(&mut store, Some("alpha"), clock.utc()).unwrap();
    assert_eq!(one["providers"].as_array().unwrap().len(), 1);
    assert_eq!(one["providers"][0]["provider"], "alpha");
    assert_eq!(
        int(&mut store, "SELECT count(*) FROM txns"),
        txns,
        "show writes nothing"
    );
}

#[test]
fn models_changes_feed_the_inbox_item_for_seven_days() {
    let (_dir, _home, mut store) = store();
    let clock = ManualClock::new();
    apply_answer(
        &mut store,
        &answer(vec![
            listed("zeta", &["a"]),
            listed("embeds", &["e1"]),
            listed("github-copilot", &["c1"]),
        ]),
        clock.utc(),
    )
    .unwrap();
    assert!(
        changed(&mut store, clock.utc()).unwrap().is_empty(),
        "a first write announces nothing"
    );

    let t1 = after(&clock, 3600);
    apply_answer(
        &mut store,
        &answer(vec![
            listed("zeta", &["a", "b"]),
            listed("embeds", &["e1", "text-embedding-3"]),
            listed("github-copilot", &["c1", "image-gen"]),
        ]),
        t1,
    )
    .unwrap();

    let rows = changed(&mut store, t1 + time::Duration::days(6)).unwrap();
    let mut names: Vec<&str> = rows.iter().map(|(p, _)| p.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["github-copilot", "zeta"],
        "a non-chat-only change of another provider is not announced; Copilot's list is whole"
    );
    let zeta = &rows.iter().find(|(p, _)| p == "zeta").unwrap().1;
    assert_eq!(zeta.at, timestamp(t1));
    assert_eq!(zeta.appeared, ["b"]);
    assert!(zeta.went_away.is_empty());

    assert!(
        changed(&mut store, t1 + time::Duration::days(8))
            .unwrap()
            .is_empty(),
        "the item ends seven days after changes.at"
    );
}
