//! `is_commit` on built `notify::Event` values; no real watcher is created.

use std::path::Path;

use notify::event::{
    AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind,
};
use notify::{Event, EventKind};

use super::watch::is_commit;

fn at(kind: EventKind, file: &str) -> Event {
    Event::new(kind).add_path(Path::new("/home/x/.cpd").join(file))
}

fn data() -> EventKind {
    EventKind::Modify(ModifyKind::Data(DataChange::Content))
}

#[test]
fn a_data_write_to_the_database_file_is_a_commit() {
    assert!(is_commit(&at(data(), "cpd.db")));
    assert!(is_commit(&at(EventKind::Modify(ModifyKind::Any), "cpd.db")));
}

#[test]
fn a_data_write_to_the_wal_is_a_commit() {
    assert!(is_commit(&at(data(), "cpd.db-wal")));
    assert!(is_commit(&at(
        EventKind::Modify(ModifyKind::Any),
        "cpd.db-wal"
    )));
}

#[test]
fn a_create_or_remove_of_the_wal_is_a_commit() {
    assert!(is_commit(&at(
        EventKind::Create(CreateKind::File),
        "cpd.db-wal"
    )));
    assert!(is_commit(&at(
        EventKind::Remove(RemoveKind::File),
        "cpd.db-wal"
    )));
}

#[test]
fn a_write_to_the_shm_file_is_not_a_commit() {
    assert!(!is_commit(&at(data(), "cpd.db-shm")));
    assert!(!is_commit(&at(
        EventKind::Create(CreateKind::File),
        "cpd.db-shm"
    )));
}

#[test]
fn a_write_to_another_file_of_the_home_is_not_a_commit() {
    assert!(!is_commit(&at(data(), "config.toml")));
    assert!(!is_commit(&at(data(), "locks/plan.lock")));
    assert!(!is_commit(&at(data(), "run/engine.pid")));
}

#[test]
fn an_access_or_open_event_of_the_wal_is_not_a_commit() {
    assert!(!is_commit(&at(
        EventKind::Access(AccessKind::Open(AccessMode::Any)),
        "cpd.db-wal"
    )));
    assert!(!is_commit(&at(
        EventKind::Access(AccessKind::Close(AccessMode::Write)),
        "cpd.db-wal"
    )));
    assert!(!is_commit(&at(
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
        "cpd.db"
    )));
}
