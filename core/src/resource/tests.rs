//! The resource locks and the holder record (contract §1.4): the module's contract, on real files in a temporary home.

use std::fs::{self, File, OpenOptions};

use super::*;
use crate::lock;

struct World {
    _dir: tempfile::TempDir,
    home: CpdHome,
    owner: OwnerLogin,
    repo: RepoName,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    World { _dir: dir, home, owner: OwnerLogin::parse("acme").unwrap(), repo: RepoName::parse("cpd").unwrap() }
}

impl World {
    fn path(&self, name: &str) -> PathBuf {
        self.home.locks_dir().join(format!("resource-acme--cpd--{name}.lock"))
    }

    fn take(&self, names: &[&str], holder: &Holder) -> Take {
        take(&self.home, &self.owner, &self.repo, names, holder).unwrap()
    }

    fn held(&self, names: &[&str], holder: &Holder) -> Held {
        match self.take(names, holder) {
            Take::Held(held) => held,
            Take::Busy { resource, holder } => panic!("expected {names:?} free, but {resource} is held by {holder:?}"),
        }
    }

    fn read(&self, name: &str) -> Option<Holder> {
        read(&self.home, &self.owner, &self.repo, name).unwrap()
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
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(world.path(name)).unwrap();
    assert!(lock::try_lock(&file).unwrap());
    fs::write(world.path(name), content).unwrap();
    file
}

#[test]
fn resource_lock_take_writes_the_holder_record_and_a_second_take_is_busy() {
    let w = world();
    let holder = command(4242);

    let held = w.held(&["e2e-port"], &holder);

    let on_disk: Holder = serde_json::from_str(&fs::read_to_string(w.path("e2e-port")).unwrap()).unwrap();
    assert_eq!(on_disk, holder);
    match w.take(&["e2e-port"], &attempt()) {
        Take::Busy { resource, holder: found } => assert_eq!((resource.as_str(), found), ("e2e-port", holder)),
        Take::Held(_) => panic!("a second take of a held resource must be busy"),
    }
    drop(held);
}

#[test]
fn resource_lock_a_free_lock_with_a_stale_record_reads_as_free() {
    let w = world();
    drop(w.held(&["e2e-port"], &command(4242)));

    assert!(!fs::read_to_string(w.path("e2e-port")).unwrap().is_empty(), "the record stays in the file");
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
    assert_eq!(serde_json::to_value(Holder::Unknown).unwrap(), serde_json::json!({"by": "unknown"}));
}

#[test]
fn resource_lock_taking_several_is_all_or_none() {
    let w = world();
    let _b = w.held(&["b"], &command(1));

    match w.take(&["c", "b", "a"], &attempt()) {
        Take::Busy { resource, holder } => assert_eq!((resource.as_str(), holder), ("b", command(1))),
        Take::Held(_) => panic!("b is held: none may be taken"),
    }
    // `a` was tried before `b` in name order and was dropped again: it is free.
    assert_eq!(w.read("a"), None);
    let all = w.held(&["c", "a"], &attempt());
    assert_eq!(all.names(), ["a", "c"]);
    assert_eq!(w.read("a"), Some(attempt()));
    assert_eq!(w.read("c"), Some(attempt()));
}

#[test]
fn resource_lock_the_file_is_named_by_owner_repo_and_name() {
    let w = world();

    assert_eq!(lock_path(&w.home, &w.owner, &w.repo, "e2e-port"), w.home.locks_dir().join("resource-acme--cpd--e2e-port.lock"));
    let _held = w.held(&["e2e-port"], &attempt());
    assert!(w.path("e2e-port").is_file());
}
