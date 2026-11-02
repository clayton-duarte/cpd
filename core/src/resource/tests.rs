//! The resource locks and the holder record (contract §1.4): the module's contract, on real files in a temporary home.

use std::fs::{self, File, OpenOptions};

use super::*;
use crate::lock;
use crate::policy::{Declared, ResourceScope};

struct World {
    _dir: tempfile::TempDir,
    home: CpdHome,
    owner: OwnerLogin,
    repo: RepoName,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    World {
        _dir: dir,
        home,
        owner: OwnerLogin::parse("acme").unwrap(),
        repo: RepoName::parse("cpd").unwrap(),
    }
}

impl World {
    fn pool(&self, name: &str) -> Pool {
        Pool::Repo {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            name: name.to_owned(),
            slots: 1,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        lock_path(&self.home, &self.pool(name), 1)
    }

    fn take(&self, names: &[&str], holder: &Holder) -> Take {
        let pools: Vec<Pool> = names.iter().map(|n| self.pool(n)).collect();
        take(&self.home, &pools, holder).unwrap()
    }

    fn held(&self, names: &[&str], holder: &Holder) -> Held {
        match self.take(names, holder) {
            Take::Held(held) => held,
            Take::Busy { resource, holders } => {
                panic!("expected {names:?} free, but {resource} is held by {holders:?}")
            }
        }
    }

    fn read(&self, name: &str) -> Option<Holder> {
        holders(&self.home, &self.pool(name)).unwrap().remove(0)
    }
}

fn command(pid: u32) -> Holder {
    Holder::Command {
        pid,
        command: "npm run dev".to_owned(),
        session: Some("s1".to_owned()),
        attempt: None,
        at: "2026-10-26T21:40:00.000Z".to_owned(),
    }
}

fn attempt() -> Holder {
    Holder::Attempt {
        plan: "pl".to_owned(),
        job: "api".to_owned(),
        attempt: "api-a1".to_owned(),
        at: "2026-10-26T21:41:00.000Z".to_owned(),
    }
}

/// A lock file held by this test with `content` in it, as a taker that wrote a record (or none) would leave it.
fn held_file(world: &World, name: &str, content: &str) -> File {
    fs::create_dir_all(world.home.locks_dir()).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(world.path(name))
        .unwrap();
    assert!(lock::try_lock(&file).unwrap());
    fs::write(world.path(name), content).unwrap();
    file
}

#[test]
fn resource_lock_take_writes_the_holder_record_and_a_second_take_is_busy() {
    let w = world();
    let holder = command(4242);

    let held = w.held(&["e2e-port"], &holder);

    let on_disk: Holder =
        serde_json::from_str(&fs::read_to_string(w.path("e2e-port")).unwrap()).unwrap();
    assert_eq!(on_disk, holder);
    match w.take(&["e2e-port"], &attempt()) {
        Take::Busy { resource, holders } => assert_eq!(
            (resource.as_str(), holders),
            ("e2e-port", vec![Some(holder)])
        ),
        Take::Held(_) => panic!("a second take of a held resource must be busy"),
    }
    drop(held);
}

#[test]
fn resource_lock_a_free_lock_with_a_stale_record_reads_as_free() {
    let w = world();
    drop(w.held(&["e2e-port"], &command(4242)));

    assert!(
        !fs::read_to_string(w.path("e2e-port")).unwrap().is_empty(),
        "the record stays in the file"
    );
    assert_eq!(w.read("e2e-port"), None);
    drop(w.held(&["e2e-port"], &attempt()));
}

#[test]
fn resource_lock_read_shows_the_holder_of_a_held_lock() {
    let w = world();
    let _held = w.held(&["e2e-port"], &attempt());

    assert_eq!(w.read("e2e-port"), Some(attempt()));
    assert_eq!(w.read("other"), None);
}

#[test]
fn resource_lock_a_held_lock_with_an_empty_record_is_holder_unknown() {
    let w = world();
    let _empty = held_file(&w, "empty", "");
    let _garbage = held_file(&w, "garbage", "{\"by\": ");

    assert_eq!(w.read("empty"), Some(Holder::Unknown));
    assert_eq!(w.read("garbage"), Some(Holder::Unknown));
    assert_eq!(
        serde_json::to_value(Holder::Unknown).unwrap(),
        serde_json::json!({"by": "unknown"})
    );
}

#[test]
fn resource_lock_taking_several_is_all_or_none() {
    let w = world();
    let _b = w.held(&["b"], &command(1));

    match w.take(&["c", "b", "a"], &attempt()) {
        Take::Busy { resource, holders } => {
            assert_eq!((resource.as_str(), holders), ("b", vec![Some(command(1))]))
        }
        Take::Held(_) => panic!("b is held: none may be taken"),
    }
    // `a` was tried before `b` in name order and was dropped again: it is free.
    assert_eq!(w.read("a"), None);
    let all = w.held(&["c", "a"], &attempt());
    assert_eq!(all.names(), ["a", "c"]);
    assert_eq!(w.read("a"), Some(attempt()));
    assert_eq!(w.read("c"), Some(attempt()));
}

fn pool_of(scope: ResourceScope, name: &str, slots: u8, owner: &str, repo: &str) -> Pool {
    let declared = Declared {
        name: name.to_owned(),
        scope,
        slots,
        source: "/p/x.toml".to_owned(),
    };
    Pool::of(
        &declared,
        &OwnerLogin::parse(owner).unwrap(),
        &RepoName::parse(repo).unwrap(),
    )
}

/// A repo pool of `slots` slots of `acme/cpd`.
fn slotted(w: &World, name: &str, slots: u8) -> Pool {
    Pool::Repo {
        owner: w.owner.clone(),
        repo: w.repo.clone(),
        name: name.to_owned(),
        slots,
    }
}

/// The record of slot `n` as a taker wrote it.
fn record_of(w: &World, pool: &Pool, slot: u8) -> Holder {
    serde_json::from_str(&fs::read_to_string(lock_path(&w.home, pool, slot)).unwrap()).unwrap()
}

fn took(w: &World, pools: &[Pool], holder: &Holder) -> Held {
    match take(&w.home, pools, holder).unwrap() {
        Take::Held(held) => held,
        Take::Busy { resource, holders } => {
            panic!("expected a free slot, but {resource} is full: {holders:?}")
        }
    }
}

/// Takes every slot of `pool` for a tenant, then finds `other` (the same pool seen from another repo) full.
fn fills_and_blocks(w: &World, pool: &Pool, other: &Pool) -> Vec<Held> {
    let held: Vec<Held> = (1..=pool.slots())
        .map(|n| took(w, std::slice::from_ref(pool), &command(u32::from(n))))
        .collect();
    assert!(
        matches!(
            take(&w.home, std::slice::from_ref(other), &attempt()).unwrap(),
            Take::Busy { .. }
        ),
        "{other:?} shares the full pool"
    );
    held
}

#[test]
fn resource_host_pool_is_one_pool_for_every_repo() {
    let w = world();
    let a = pool_of(ResourceScope::Host, "shared-port", 2, "acme", "web");
    let b = pool_of(ResourceScope::Host, "shared-port", 2, "other", "api");

    assert_eq!(a, b, "a host pool does not depend on the repo");
    assert_eq!(a.slots(), 2, "the pool has the declaration's slots");
    assert_eq!(
        lock_path(&w.home, &a, 2),
        w.home.locks_dir().join("resource.host.shared-port.2.lock")
    );
    drop(fills_and_blocks(&w, &a, &b));
}

#[test]
fn resource_owner_pool_is_shared_by_the_repos_of_one_owner() {
    let w = world();
    let web = pool_of(ResourceScope::Owner, "staging", 2, "acme", "web");
    let api = pool_of(ResourceScope::Owner, "staging", 2, "acme", "api");
    let elsewhere = pool_of(ResourceScope::Owner, "staging", 2, "other", "web");

    assert_eq!(web, api, "an owner pool does not depend on the repo");
    assert_ne!(web, elsewhere, "another owner has its own pool");
    assert_eq!(web.slots(), 2, "the pool has the declaration's slots");
    assert_eq!(
        lock_path(&w.home, &web, 2),
        w.home
            .locks_dir()
            .join("resource.owner.acme.staging.2.lock")
    );
    let held = fills_and_blocks(&w, &web, &api);
    drop(took(&w, &[elsewhere], &command(9)));
    drop(held);
}

#[test]
fn resource_repo_pool_is_one_pool_per_repo() {
    let w = world();
    let web = pool_of(ResourceScope::Repo, "staging", 2, "acme", "web");
    let api = pool_of(ResourceScope::Repo, "staging", 2, "acme", "api");

    assert_ne!(web, api, "a repo pool belongs to its repo");
    assert_eq!(web.slots(), 2, "the pool has the declaration's slots");
    assert_eq!(
        lock_path(&w.home, &web, 2),
        w.home
            .locks_dir()
            .join("resource.repo.acme.web.staging.2.lock")
    );
    let held = fills_and_blocks(&w, &web, &web);
    drop(took(&w, &[api], &command(9)));
    drop(held);
}

#[test]
fn resource_lock_file_names_are_injective_per_scope_owner_repo_name_slot() {
    let w = world();
    let mut seen = std::collections::BTreeMap::new();
    let mut count = 0;
    // Names that could collide under a `--` or a `.` scheme: a repo with dots, a name with `--`, a repo named like a name.
    for slot in 1..=3u8 {
        let mut pools = vec![
            Pool::Host {
                name: "a".to_owned(),
                slots: 3,
            },
            Pool::Host {
                name: "a--b".to_owned(),
                slots: 3,
            },
            Pool::Host {
                name: "b".to_owned(),
                slots: 3,
            },
        ];
        for owner in ["acme", "acme-x"] {
            let owner = OwnerLogin::parse(owner).unwrap();
            for name in ["a", "a--b", "b"] {
                pools.push(Pool::Owner {
                    owner: owner.clone(),
                    name: name.to_owned(),
                    slots: 3,
                });
                for repo in ["a", "b", "a.b", "a--b"] {
                    pools.push(Pool::Repo {
                        owner: owner.clone(),
                        repo: RepoName::parse(repo).unwrap(),
                        name: name.to_owned(),
                        slots: 3,
                    });
                }
            }
        }
        for pool in pools {
            count += 1;
            let path = lock_path(&w.home, &pool, slot);
            if let Some(other) = seen.insert(path.clone(), (pool.clone(), slot)) {
                panic!(
                    "{} is the lock of {other:?} and of {:?}",
                    path.display(),
                    (pool, slot)
                );
            }
        }
    }
    assert_eq!(seen.len(), count);
    let pool = slotted(&w, "e2e-port", 2);
    assert_eq!(
        lock_path(&w.home, &pool, 2),
        w.home
            .locks_dir()
            .join("resource.repo.acme.cpd.e2e-port.2.lock")
    );
    assert_eq!(
        lock_path(
            &w.home,
            &Pool::Owner {
                owner: w.owner.clone(),
                name: "db".to_owned(),
                slots: 2
            },
            2
        ),
        w.home.locks_dir().join("resource.owner.acme.db.2.lock")
    );
}

#[test]
fn resource_take_gets_the_lowest_free_slot_and_is_busy_only_when_every_slot_is_held() {
    let w = world();
    let pool = slotted(&w, "e2e-port", 3);

    let first = took(&w, std::slice::from_ref(&pool), &command(1));
    let second = took(&w, std::slice::from_ref(&pool), &command(2));
    let third = took(&w, std::slice::from_ref(&pool), &command(3));
    assert_eq!(
        (
            first.slot("e2e-port"),
            second.slot("e2e-port"),
            third.slot("e2e-port")
        ),
        (Some(1), Some(2), Some(3))
    );
    assert_eq!(
        record_of(&w, &pool, 2),
        command(2),
        "each slot has its own holder record"
    );
    assert_eq!(
        holders(&w.home, &pool).unwrap(),
        vec![Some(command(1)), Some(command(2)), Some(command(3))]
    );
    match take(&w.home, std::slice::from_ref(&pool), &command(4)).unwrap() {
        Take::Busy { resource, holders } => {
            assert_eq!(resource, "e2e-port");
            assert_eq!(
                holders,
                vec![Some(command(1)), Some(command(2)), Some(command(3))],
                "every slot, in slot order"
            );
        }
        Take::Held(_) => panic!("every slot is held"),
    }

    // A slot freed in the middle is the lowest free one.
    drop(second);
    assert_eq!(
        holders(&w.home, &pool).unwrap(),
        vec![Some(command(1)), None, Some(command(3))]
    );
    let again = took(&w, std::slice::from_ref(&pool), &attempt());
    assert_eq!(again.slot("e2e-port"), Some(2));

    // A pool whose slots were lowered keeps the held slots held and offers none above the count.
    let lowered = slotted(&w, "e2e-port", 2);
    drop(first);
    let five = took(&w, std::slice::from_ref(&lowered), &command(5));
    assert_eq!(five.slot("e2e-port"), Some(1));
    match take(&w.home, std::slice::from_ref(&lowered), &command(6)).unwrap() {
        Take::Busy { holders, .. } => assert_eq!(holders.len(), 2),
        Take::Held(_) => panic!("slot 3 is above the count and is never offered"),
    }
    drop((third, again, five));
}

#[test]
fn resource_take_is_all_or_none_across_resources_with_slots() {
    let w = world();
    let (a, b) = (slotted(&w, "a", 2), slotted(&w, "b", 1));
    let _a1 = took(&w, std::slice::from_ref(&a), &command(1));
    let _b1 = took(&w, std::slice::from_ref(&b), &command(2));

    // `a` has a free slot (the second), `b` has none: nothing is kept, not even `a`'s slot 2.
    match take(&w.home, &[b.clone(), a.clone()], &attempt()).unwrap() {
        Take::Busy { resource, holders } => {
            assert_eq!((resource.as_str(), holders), ("b", vec![Some(command(2))]))
        }
        Take::Held(_) => panic!("b is full: none may be taken"),
    }
    assert_eq!(holders(&w.home, &a).unwrap(), vec![Some(command(1)), None]);

    drop(_b1);
    let both = took(&w, &[b, a.clone()], &attempt());
    assert_eq!((both.slot("a"), both.slot("b")), (Some(2), Some(1)));
    assert_eq!(
        holders(&w.home, &a).unwrap(),
        vec![Some(command(1)), Some(attempt())]
    );
}

#[test]
fn resource_slot_lock_is_free_again_when_its_holder_dies() {
    let w = world();
    let pool = slotted(&w, "e2e-port", 2);
    let first = took(&w, std::slice::from_ref(&pool), &command(1));
    let second = took(&w, std::slice::from_ref(&pool), &command(2));
    assert_eq!(
        holders(&w.home, &pool).unwrap(),
        vec![Some(command(1)), Some(command(2))]
    );

    // The holder of slot 1 dies: the kernel closes its descriptor; no code releases a slot by name.
    drop(first);

    // Slot 1's file still holds its record, but the lock is free: it reads free, and the next taker gets slot 1 again.
    assert!(
        !fs::read_to_string(lock_path(&w.home, &pool, 1))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        holders(&w.home, &pool).unwrap(),
        vec![None, Some(command(2))]
    );
    assert_eq!(
        took(&w, std::slice::from_ref(&pool), &attempt()).slot("e2e-port"),
        Some(1)
    );
    drop(second);
}

/// A store with Plan `pay` and its Attempts: `a-a1` running, `b-a1` starting, `c-a1` running, `d-a1` ended (all four of different Jobs; `d` is `failed`, so a tenant still).
fn store_with_attempts() -> (tempfile::TempDir, crate::store::Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = crate::store::Store::open(&home, 5000).unwrap();
    let fingerprint = "f".repeat(64);
    let mut sql = String::from(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'api', '/r', 'main', 'dispatch', 'cpd/pay', '/w', cpd_txn(), 't', 't');",
    );
    for (job, status, at) in [
        ("a", "running", "2026-10-26T21:40:00.000Z"),
        ("b", "starting", "2026-10-26T21:41:00.000Z"),
        ("c", "running", "2026-10-26T21:42:00.000Z"),
        ("d", "ended", "2026-10-26T21:43:00.000Z"),
    ] {
        let (job_status, ended) = if status == "ended" {
            ("failed", ", outcome, cause, ended_at")
        } else {
            ("running", "")
        };
        let cause = if status == "ended" {
            "'crashed'"
        } else {
            "NULL"
        };
        let ended_values = if status == "ended" {
            ", 'failed', 'crashed', 't'"
        } else {
            ""
        };
        sql += &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'api', '{job}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{job_status}', {cause}, 0, '{fingerprint}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at{ended})
               VALUES ('pay', '{job}-a1', '{job}', 1, 'S2', 'ladder', '{status}', 'cpd/pay--{job}-a1', '/w', '{at}'{ended_values});"
        );
    }
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

#[test]
fn workers_is_a_host_resource_with_concurrency_slots_and_attempts_as_tenants() {
    let (dir, mut store) = store_with_attempts();

    let pool = store.read(|conn| workers(conn, 3)).unwrap();

    assert_eq!(WORKERS, "workers");
    assert_eq!(pool.slots, 3, "slots = concurrency");
    let tenant = |job: &str, at: &str| Holder::Attempt {
        plan: "pay".to_owned(),
        job: job.to_owned(),
        attempt: format!("{job}-a1"),
        at: at.to_owned(),
    };
    assert_eq!(
        pool.holders,
        [
            tenant("a", "2026-10-26T21:40:00.000Z"),
            tenant("b", "2026-10-26T21:41:00.000Z"),
            tenant("c", "2026-10-26T21:42:00.000Z"),
            tenant("d", "2026-10-26T21:43:00.000Z")
        ],
        "every tenant Job of `workers` in start order: the `failed` Job `d` keeps its worker though its Attempt ended"
    );
    // The slots follow the config, the tenants do not.
    assert_eq!(store.read(|conn| workers(conn, 1)).unwrap().slots, 1);
    assert_eq!(
        store.read(|conn| workers(conn, 1)).unwrap().holders.len(),
        4,
        "an urgent Job's slot beyond concurrency is a tenant too"
    );
    // No lock file: the Ledger is the record.
    let locks = dir.path().join("locks");
    assert!(
        !locks.exists()
            || fs::read_dir(&locks).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("workers"))
    );
}

#[test]
fn resource_run_takes_a_slot_only_when_job_tenants_plus_commands_are_below_slots() {
    // The free slots of a pool of 2 are `slots` minus the Job tenants (the database, no slot file) minus the held command slots.
    let (dir, mut store) = store_with_attempts();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let pool = Pool::Repo {
        owner: OwnerLogin::parse("acme").unwrap(),
        repo: RepoName::parse("cpd").unwrap(),
        name: "e2e-port".to_owned(),
        slots: 2,
    };
    let elsewhere = Pool::Repo {
        owner: OwnerLogin::parse("acme").unwrap(),
        repo: RepoName::parse("other").unwrap(),
        name: "e2e-port".to_owned(),
        slots: 2,
    };
    let free = |store: &mut crate::store::Store, pool: &Pool| {
        store.read(|conn| free_slots(conn, &home, pool)).unwrap()
    };
    assert_eq!(free(&mut store, &pool), 2, "nobody holds it");

    // Job `a` is a tenant of the pool: one slot, with no lock file.
    store.write("fixture", |tx| { tx.execute_batch("INSERT INTO job_pools (plan, job, pool) VALUES ('pay', 'a', 'repo:acme/cpd:e2e-port');")?; Ok(()) }).unwrap();
    assert_eq!(free(&mut store, &pool), 1, "one Job tenant");
    assert_eq!(
        free(&mut store, &elsewhere),
        2,
        "the pool of another repo is another pool"
    );
    assert!(
        !lock_path(&home, &pool, 1).exists(),
        "a Job takes no lock file"
    );

    // A command holds slot 1: no slot is free.
    fs::create_dir_all(home.locks_dir()).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(&home, &pool, 1))
        .unwrap();
    assert!(lock::try_lock(&file).unwrap());
    assert_eq!(
        free(&mut store, &pool),
        0,
        "one Job tenant and one command are the two slots"
    );

    // The command ends: the slot is free again; a pool whose slots are lowered below what is taken is never negative.
    drop(file);
    assert_eq!(free(&mut store, &pool), 1);
    let one = Pool::Repo {
        owner: OwnerLogin::parse("acme").unwrap(),
        repo: RepoName::parse("cpd").unwrap(),
        name: "e2e-port".to_owned(),
        slots: 1,
    };
    store.write("fixture", |tx| { tx.execute_batch("INSERT INTO job_pools (plan, job, pool) VALUES ('pay', 'b', 'repo:acme/cpd:e2e-port');")?; Ok(()) }).unwrap();
    assert_eq!(
        free(&mut store, &one),
        0,
        "two tenants of a pool of one: never below 0"
    );
}
