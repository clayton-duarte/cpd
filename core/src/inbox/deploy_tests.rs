//! E8's `deploy` condition (contract §2.2 "The deploy rule"): one yellow item per run of `run/deploy-runs.json`.

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Filter, Item, Kind, State, Urgency, list, reconcile};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-10-27 15:00 UTC);
/// When the reconcile runs: later than every run's `created_at`.
const AT: &str = "2026-10-27T15:00:00.000Z";
const CREATED: &str = "2026-10-27T13:05:00Z";

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn run_entry(run_id: u64, created_at: &str) -> Value {
    json!({"repo": "acme/pay", "workflow": "main.yml", "run_id": run_id, "environment": "production, staging",
           "title": "Release 1.2", "url": format!("https://github.com/acme/pay/actions/runs/{run_id}"), "created_at": created_at})
}

fn write_cache(home: &CpdHome, runs: &[Value]) {
    let path = home.run_dir().join("deploy-runs.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let cache = json!({"version": 1, "polled_at": "2026-10-27T14:59:55.000Z", "runs": runs});
    std::fs::write(path, cache.to_string()).unwrap();
}

/// The `deploy` conditions E8 computes now.
fn deploys(store: &mut Store, home: &CpdHome) -> Vec<Condition> {
    conditions_with(store, home, NOW, 20, 1, None).unwrap().into_iter().filter(|c| c.kind == Kind::Deploy).collect()
}

/// One E8 pass: every condition, reconciled at `AT`.
fn pass(store: &mut Store, home: &CpdHome) -> super::model::Changes {
    let all = conditions_with(store, home, NOW, 20, 1, None).unwrap();
    reconcile(store, &all, AT).unwrap()
}

fn open_deploys(store: &mut Store) -> Vec<Item> {
    store.read(|c| list(c, &Filter { context: None, state: State::Open })).unwrap().into_iter().filter(|i| i.kind == Kind::Deploy).collect()
}

fn visible(store: &mut Store) -> Vec<Item> {
    store.read(|c| list(c, &Filter { context: None, state: State::Visible { now: AT.to_owned() } })).unwrap()
}

#[test]
fn a_cached_run_opens_one_yellow_deploy_item() {
    let (_dir, home, mut store) = store();
    write_cache(&home, &[run_entry(40508749055, CREATED)]);

    let found = deploys(&mut store, &home);
    assert_eq!(found.len(), 1, "{found:?}");
    let c = &found[0];
    assert_eq!((c.kind, c.urgency), (Kind::Deploy, Urgency::Yellow));
    assert_eq!(c.subject, "deploy:acme/pay#40508749055");
    assert_eq!(c.label, "acme/pay deploy production, staging: Release 1.2");
    assert_eq!(c.url.as_deref(), Some("https://github.com/acme/pay/actions/runs/40508749055"));
    assert_eq!((&c.plan, &c.context, &c.job, &c.session, &c.record, &c.event), (&None, &None, &None, &None, &None, &None));

    pass(&mut store, &home);
    let items = open_deploys(&mut store);
    assert_eq!(items.len(), 1, "{items:?}");
    // The age counts from when the run began waiting, not from when cpd saw it.
    assert_eq!(items[0].opened_at, CREATED);
    assert_eq!(items[0].urgency, Urgency::Yellow);
}

#[test]
fn the_deploy_item_stays_open_while_the_run_is_cached() {
    let (_dir, home, mut store) = store();
    write_cache(&home, &[run_entry(7, CREATED)]);
    pass(&mut store, &home);
    let first = open_deploys(&mut store);
    assert_eq!(first.len(), 1, "{first:?}");

    // The poll rewrites the file with the same run: nothing changes.
    write_cache(&home, &[run_entry(7, CREATED)]);
    let again = pass(&mut store, &home);
    assert!(again.is_empty(), "{again:?}");
    let second = open_deploys(&mut store);
    assert_eq!(second.iter().map(|i| i.id).collect::<Vec<_>>(), first.iter().map(|i| i.id).collect::<Vec<_>>());
    assert_eq!(second[0].closed_at, None);
}

#[test]
fn the_deploy_item_closes_when_the_run_leaves_the_cache() {
    let (_dir, home, mut store) = store();
    write_cache(&home, &[run_entry(7, CREATED), run_entry(8, "2026-10-27T13:10:00Z")]);
    pass(&mut store, &home);
    assert_eq!(open_deploys(&mut store).len(), 2);

    write_cache(&home, &[run_entry(8, "2026-10-27T13:10:00Z")]);
    pass(&mut store, &home);
    let open = open_deploys(&mut store);
    assert_eq!(open.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["deploy:acme/pay#8"]);
    let closed: Vec<Item> = store.read(|c| list(c, &Filter { context: None, state: State::Closed { limit: 10 } })).unwrap();
    assert_eq!(closed.iter().map(|i| (i.kind, i.subject.as_str())).collect::<Vec<_>>(), [(Kind::Deploy, "deploy:acme/pay#7")]);

    // The run returns: a new item.
    write_cache(&home, &[run_entry(7, CREATED), run_entry(8, "2026-10-27T13:10:00Z")]);
    pass(&mut store, &home);
    assert_eq!(open_deploys(&mut store).len(), 2);
}

#[test]
fn a_snoozed_or_dismissed_deploy_item_stays_hidden() {
    let (_dir, home, mut store) = store();
    write_cache(&home, &[run_entry(7, CREATED), run_entry(8, "2026-10-27T13:10:00Z")]);
    pass(&mut store, &home);
    let shown: Vec<String> = visible(&mut store).into_iter().filter(|i| i.kind == Kind::Deploy).map(|i| i.subject).collect();
    assert_eq!(shown, ["deploy:acme/pay#7", "deploy:acme/pay#8"]);

    store
        .write("fixture", |tx| {
            tx.execute("UPDATE inbox_items SET snoozed_until = '2026-10-28T00:00:00.000Z' WHERE subject = 'deploy:acme/pay#7'", [])?;
            tx.execute("UPDATE inbox_items SET dismissed_at = '2026-10-27T14:00:00.000Z' WHERE subject = 'deploy:acme/pay#8'", [])?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
    // A later pass with the same runs neither shows nor reopens them.
    pass(&mut store, &home);
    let shown: Vec<String> = visible(&mut store).into_iter().filter(|i| i.kind == Kind::Deploy).map(|i| i.subject).collect();
    assert!(shown.is_empty(), "{shown:?}");
    assert_eq!(open_deploys(&mut store).len(), 2, "both stay open, hidden");
}
