//! Red tests of the `[experimental]` config table, the `worker_runner` migration and the atomic setter
//! (contract §8.1, §8.2; Plan experimental-config, A1, A3, A8).

use std::path::Path;

use serde_json::json;

use super::experimental::{Feature, set};
use super::file::Sources;
use super::migrate::apply;
use super::{Config, Source, WorkerRunner, toggle};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

const BASE: &str = "version = 1\nmachine = \"hs\"\n";

fn parse(text: &str) -> Result<(Config, Sources), CpdError> {
    Config::parse(text, Path::new("config.toml"))
}

fn home_with(text: &str) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::write(home.config(), text).unwrap();
    (dir, home)
}

fn read_table(home: &CpdHome) -> toml::Table {
    std::fs::read_to_string(home.config())
        .unwrap()
        .parse()
        .unwrap()
}

fn experimental_of(table: &toml::Table) -> toml::Table {
    table["experimental"].as_table().unwrap().clone()
}

#[test]
fn experimental_defaults_are_true_except_durable_workers() {
    let (config, _) = parse(BASE).unwrap();
    let e = config.experimental;
    assert!(!e.durable_workers);
    assert!(e.write_curation);
    assert!(e.failure_triage);
    assert!(e.turn_question_guard);
}

#[test]
fn experimental_table_parses_four_booleans() {
    let (config, _) = parse(&format!(
        "{BASE}\n[experimental]\ndurable_workers = true\nwrite_curation = false\nfailure_triage = false\nturn_question_guard = false\n"
    ))
    .unwrap();
    let e = config.experimental;
    assert!(e.durable_workers);
    assert!(!e.write_curation);
    assert!(!e.failure_triage);
    assert!(!e.turn_question_guard);
}

#[test]
fn experimental_refuses_non_boolean_and_unknown_key() {
    let text = format!("{BASE}\n[experimental]\nwrite_curation = \"yes\"\n");
    let err = parse(&text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(err.details()["key"], json!("experimental.write_curation"));

    let text = format!("{BASE}\n[experimental]\nmystery = true\n");
    let err = parse(&text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(err.details()["key"], json!("experimental.mystery"));
}

#[test]
fn top_level_worker_runner_is_refused_as_unknown_key() {
    let err = parse(&format!("{BASE}worker_runner = \"durable\"\n")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(err.details()["key"], json!("worker_runner"));
    assert!(
        err.message().contains("not a known key"),
        "{}",
        err.message()
    );
}

#[test]
fn worker_runner_accessor_follows_durable_workers() {
    let (off, _) = parse(BASE).unwrap();
    assert_eq!(off.worker_runner(), WorkerRunner::Pi);
    let (on, _) = parse(&format!("{BASE}\n[experimental]\ndurable_workers = true\n")).unwrap();
    assert_eq!(on.worker_runner(), WorkerRunner::Durable);
}

#[test]
fn experimental_appears_in_config_json_with_sources() {
    let (config, sources) =
        parse(&format!("{BASE}\n[experimental]\nfailure_triage = false\n")).unwrap();
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(
        value["experimental"],
        json!({
            "durable_workers": false,
            "write_curation": true,
            "failure_triage": false,
            "turn_question_guard": true,
            "durable_leads": false,
        })
    );
    assert_eq!(sources["experimental.durable_leads"], Source::Default);
    assert_eq!(sources["experimental.failure_triage"], Source::File);
    assert_eq!(sources["experimental.durable_workers"], Source::Default);
    assert_eq!(sources["experimental.write_curation"], Source::Default);
    assert_eq!(sources["experimental.turn_question_guard"], Source::Default);
}

#[test]
fn migration_worker_runner_durable_becomes_durable_workers_true() {
    let (out, names) = apply(&format!(
        "{BASE}worker_runner = \"durable\"\nconcurrency = 5\n"
    ));
    assert_eq!(names, vec!["worker-runner-to-experimental"]);
    let table: toml::Table = out.parse().unwrap();
    assert!(!table.contains_key("worker_runner"));
    assert_eq!(table["concurrency"].as_integer(), Some(5));
    assert_eq!(
        experimental_of(&table)["durable_workers"].as_bool(),
        Some(true)
    );
}

#[test]
fn migration_worker_runner_pi_is_dropped() {
    let (out, names) = apply(&format!("{BASE}worker_runner = \"pi\"\nconcurrency = 5\n"));
    assert_eq!(names, vec!["worker-runner-to-experimental"]);
    let table: toml::Table = out.parse().unwrap();
    assert!(!table.contains_key("worker_runner"));
    assert!(!table.contains_key("experimental"));
    assert_eq!(table["concurrency"].as_integer(), Some(5));
}

#[test]
fn migration_is_idempotent_and_keeps_an_existing_experimental_table() {
    let start =
        format!("{BASE}worker_runner = \"durable\"\n\n[experimental]\nfailure_triage = false\n");
    let (once, names) = apply(&start);
    assert_eq!(names, vec!["worker-runner-to-experimental"]);
    let table: toml::Table = once.parse().unwrap();
    let e = experimental_of(&table);
    assert_eq!(e["durable_workers"].as_bool(), Some(true));
    assert_eq!(e["failure_triage"].as_bool(), Some(false));
    assert!(!table.contains_key("worker_runner"));

    let (twice, names) = apply(&once);
    assert_eq!(twice, once);
    assert!(names.is_empty());
}

#[test]
fn runner_toggle_writes_durable_workers_in_the_experimental_table() {
    let (_dir, home) = home_with(&format!(
        "{BASE}concurrency = 5\n\n[experimental]\nfailure_triage = false\n"
    ));
    assert_eq!(toggle(&home).unwrap(), WorkerRunner::Durable);
    let table = read_table(&home);
    assert!(!table.contains_key("worker_runner"));
    assert_eq!(table["concurrency"].as_integer(), Some(5));
    let e = experimental_of(&table);
    assert_eq!(e["durable_workers"].as_bool(), Some(true));
    assert_eq!(e["failure_triage"].as_bool(), Some(false));
    assert_eq!(toggle(&home).unwrap(), WorkerRunner::Pi);
    assert_eq!(
        experimental_of(&read_table(&home))["durable_workers"].as_bool(),
        Some(false)
    );
}

#[test]
fn set_experimental_writes_one_key_atomically_keeping_comments() {
    let before = format!(
        "# my config\n{BASE}concurrency = 5 # five\n\n[experimental]\n# triage note\nfailure_triage = true # keep\nwrite_curation = false\n"
    );
    let (dir, home) = home_with(&before);
    set(&home, Feature::FailureTriage, false).unwrap();
    let after = std::fs::read_to_string(home.config()).unwrap();
    assert_eq!(
        after,
        before.replace("failure_triage = true", "failure_triage = false")
    );
    // Atomic: no temporary file is left beside config.toml.
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["config.toml"]);

    // The table is created when missing.
    let (_d2, home2) = home_with(&format!("{BASE}# note\nconcurrency = 5\n"));
    set(&home2, Feature::DurableWorkers, true).unwrap();
    let text = std::fs::read_to_string(home2.config()).unwrap();
    assert!(text.contains("# note"));
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(table["concurrency"].as_integer(), Some(5));
    assert_eq!(
        experimental_of(&table)["durable_workers"].as_bool(),
        Some(true)
    );

    // A config that does not parse is refused and left as it was.
    let broken = format!("{BASE}concurrency = \"many\"\n");
    let (_d3, home3) = home_with(&broken);
    let err = set(&home3, Feature::WriteCuration, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(std::fs::read_to_string(home3.config()).unwrap(), broken);
}

#[test]
fn durable_leads_defaults_to_false() {
    let (config, _) = parse(BASE).unwrap();
    assert!(!config.experimental.durable_leads);
    // The key is known and parses when set.
    let (config, _) = parse(&format!("{BASE}\n[experimental]\ndurable_leads = true\n")).unwrap();
    assert!(config.experimental.durable_leads);
}

#[test]
fn durable_leads_rejects_a_non_bool() {
    let err = parse(&format!(
        "{BASE}\n[experimental]\ndurable_leads = \"yes\"\n"
    ))
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::ConfigInvalid);
    assert_eq!(err.details()["key"], json!("experimental.durable_leads"));
    assert!(err.to_string().contains("must be a boolean"), "{err}");
}
