//! The `tier_catalog` cache is keyed by the content of `<agentDir>/models.json` too (commands.md §14.3, contract §4.4 and §9;
//! pro-d26, Plan bifrost, Workflow catfp): `models_sha` is the lowercase hex SHA-256 of the file's bytes, `""` when it does not
//! exist. Through [`load_from`] on a scripted `node` (no real pi, no node): the table is append-only, so a changed file adds rows.

use std::fs;
use std::path::Path;

use serde_json::json;

use super::*;
use crate::clock::ManualClock;
use crate::host::runner::{FakeRunner, Reply};
use crate::ports::Ports;

/// pi levels of the three configured models: O and H as given, S as `sonnet`.
fn levels(sonnet: &[&str]) -> Reply {
    let base = ["off", "low", "medium", "high"];
    Reply::ok(&json!({"O": base, "S": sonnet, "H": base, "G": ["off"]}).to_string())
}

const UP_TO_HIGH: [&str; 4] = ["off", "low", "medium", "high"];
const UP_TO_MAX: [&str; 6] = ["off", "low", "medium", "high", "xhigh", "max"];

const SONNET_UP_TO_HIGH: &str = r#"{"providers":{"bifrost":{"modelOverrides":{"claude-sonnet-5-5":{"thinkingLevelMap":{"minimal":null}}}}}}"#;
const SONNET_UP_TO_MAX: &str = r#"{"providers":{"bifrost":{"modelOverrides":{"claude-sonnet-5-5":{"thinkingLevelMap":{"minimal":null,"xhigh":"xhigh","max":"max"}}}}}}"#;

struct Rig {
    _dir: tempfile::TempDir,
    agent_dir: std::path::PathBuf,
    store: Store,
    models: Models,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        let agent_dir = dir.path().join("agent");
        fs::create_dir_all(&agent_dir).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        Rig {
            _dir: dir,
            agent_dir,
            store,
            models: crate::policy::default_policy().model.clone(),
        }
    }

    fn pi(&self) -> PiInstall {
        PiInstall {
            agent_dir: self.agent_dir.clone(),
            version: "1.3.0".into(),
            coding_agent_entry: "/nonexistent/coding-agent.js".into(),
            pi_ai_entry: "/nonexistent/pi-ai.js".into(),
        }
    }

    fn models_json(&self) -> std::path::PathBuf {
        self.agent_dir.join("models.json")
    }

    /// `load_from` on a `node` that answers with `replies` in turn; the runner is returned to count its calls. A call past the
    /// replies is unscripted and fails.
    fn load(&mut self, replies: Vec<Reply>) -> (FakeRunner, Result<Catalog, CpdError>) {
        let mut runner = FakeRunner::new(ManualClock::new());
        for reply in replies {
            runner = runner.once("node", |_| true, reply);
        }
        let ports = Ports {
            runner: std::sync::Arc::new(runner.clone()),
            ..Ports::real()
        };
        let pi = self.pi();
        let result = Ports::scope(&ports, || load_from(&pi, &self.models, &mut self.store));
        (runner, result)
    }

    fn rows(&mut self) -> i64 {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM tier_catalog WHERE pi_version = '1.3.0'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The distinct `models_sha` values, oldest first.
    fn shas(&mut self) -> Vec<String> {
        self.store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT models_sha FROM tier_catalog GROUP BY models_sha ORDER BY min(rowid)",
                )?;
                Ok(stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }
}

fn is_hex_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn efforts_of_s(catalog: &Catalog) -> Vec<u8> {
    catalog.s.efforts.clone()
}

/// Rule 1: the same pi version and the same `models.json` bytes are a cache hit.
#[test]
fn tier_catalog_same_version_and_same_models_json_bytes_is_a_cache_hit() {
    let mut rig = Rig::new();
    fs::write(rig.models_json(), SONNET_UP_TO_HIGH).unwrap();
    let (runner, first) = rig.load(vec![levels(&UP_TO_HIGH), levels(&UP_TO_MAX)]);
    assert_eq!(efforts_of_s(&first.unwrap()), [0, 1, 2, 3]);
    let (runner2, second) = rig.load(vec![]);
    assert_eq!(efforts_of_s(&second.unwrap()), [0, 1, 2, 3]);
    assert_eq!(runner.calls().len(), 1, "the catalog module ran once");
    assert_eq!(runner2.calls().len(), 0, "the second load asked nobody");
    assert_eq!(rig.rows(), 4);
    // The rows carry the file's fingerprint: 64 lowercase hex digits (the hash itself is the crate's).
    let shas = rig.shas();
    assert!(shas.len() == 1 && is_hex_sha256(&shas[0]), "{shas:?}");
}

/// Rule 2: the same version and refs, `models.json` changed: the second load recomputes, and the old rows stay.
#[test]
fn tier_catalog_changed_models_json_under_the_same_version_is_seen_and_the_old_rows_stay() {
    let mut rig = Rig::new();
    fs::write(rig.models_json(), SONNET_UP_TO_HIGH).unwrap();
    let (_, first) = rig.load(vec![levels(&UP_TO_HIGH)]);
    assert_eq!(efforts_of_s(&first.unwrap()), [0, 1, 2, 3]);
    assert_eq!(rig.rows(), 4);

    fs::write(rig.models_json(), SONNET_UP_TO_MAX).unwrap();
    let (runner, second) = rig.load(vec![levels(&UP_TO_MAX)]);
    assert_eq!(
        efforts_of_s(&second.unwrap()),
        [0, 1, 2, 3, 4],
        "the new thinkingLevelMap is seen"
    );
    assert_eq!(runner.calls().len(), 1, "the catalog module ran again");
    assert_eq!(rig.rows(), 8, "append-only: 4 + 4, not 4");
    let shas = rig.shas();
    assert!(
        shas.len() == 2 && shas.iter().all(|s| is_hex_sha256(s)) && shas[0] != shas[1],
        "{shas:?}"
    );

    // And the new bytes are warm, the old ones too (a file put back is a hit on its own rows).
    let (runner, third) = rig.load(vec![]);
    assert_eq!(efforts_of_s(&third.unwrap()), [0, 1, 2, 3, 4]);
    assert_eq!(runner.calls().len(), 0);
    fs::write(rig.models_json(), SONNET_UP_TO_HIGH).unwrap();
    let (runner, back) = rig.load(vec![]);
    assert_eq!(
        efforts_of_s(&back.unwrap()),
        [0, 1, 2, 3],
        "the first bytes' rows are still there"
    );
    assert_eq!(runner.calls().len(), 0);
    assert_eq!(rig.rows(), 8);
}

/// Rule 3: an absent `models.json` is a valid key, `""`.
#[test]
fn tier_catalog_an_absent_models_json_is_a_valid_key_the_empty_string() {
    let mut rig = Rig::new();
    assert!(!rig.models_json().exists());
    let (_, first) = rig.load(vec![levels(&UP_TO_HIGH)]);
    assert_eq!(efforts_of_s(&first.unwrap()), [0, 1, 2, 3]);
    assert_eq!(rig.shas(), [""]);
    let (runner, second) = rig.load(vec![]);
    assert_eq!(efforts_of_s(&second.unwrap()), [0, 1, 2, 3]);
    assert_eq!(runner.calls().len(), 0, "an absent file stays a hit");
    assert_eq!(rig.rows(), 4);
}

/// Rule 3: the file appearing later is a change.
#[test]
fn tier_catalog_a_models_json_that_appears_later_is_a_change() {
    let mut rig = Rig::new();
    let (_, first) = rig.load(vec![levels(&UP_TO_HIGH)]);
    assert_eq!(efforts_of_s(&first.unwrap()), [0, 1, 2, 3]);

    fs::write(rig.models_json(), SONNET_UP_TO_MAX).unwrap();
    let (runner, second) = rig.load(vec![levels(&UP_TO_MAX)]);
    assert_eq!(efforts_of_s(&second.unwrap()), [0, 1, 2, 3, 4]);
    assert_eq!(runner.calls().len(), 1);
    assert_eq!(rig.rows(), 8);
    let shas = rig.shas();
    assert_eq!(shas[0], "", "{shas:?}");
    assert!(is_hex_sha256(&shas[1]), "{shas:?}");
}

/// Rule 3: any other read error is `catalog_failed`, with the path in its details; nothing is asked of pi and nothing cached.
#[test]
fn tier_catalog_an_unreadable_models_json_is_catalog_failed_with_its_path() {
    let mut rig = Rig::new();
    // A directory in its place: reading it fails (EISDIR) without root tricks.
    fs::create_dir(rig.models_json()).unwrap();
    let (runner, result) = rig.load(vec![levels(&UP_TO_HIGH)]);
    let err = result.unwrap_err();
    assert_eq!(err.code(), ErrorCode::CatalogFailed, "{err:?}");
    let path = rig.models_json();
    let details = serde_json::Value::Object(err.details().clone()).to_string();
    assert!(
        details.contains(Path::new(&path).to_str().unwrap()),
        "the path is in the details: {details}"
    );
    assert_eq!(runner.calls().len(), 0, "no fallback: pi was not asked");
    assert_eq!(rig.rows(), 0);
}

/// pi's levels for each letter, as the catalog module prints them.
fn all_levels(o: &[&str], s: &[&str], h: &[&str], g: &[&str]) -> Reply {
    Reply::ok(&json!({"O": o, "S": s, "H": h, "G": g}).to_string())
}

/// Tier model v2: a model whose only level is `off`, or that has none, has no efforts: only `<letter>X` is valid for it.
#[test]
fn a_model_with_only_off_has_no_efforts() {
    let mut rig = Rig::new();
    let (_, result) = rig.load(vec![all_levels(
        &["off"],
        &[],
        &["off", "low", "medium", "high"],
        &["off"],
    )]);
    let catalog = result.unwrap();
    assert_eq!(catalog.o.efforts, Vec::<u8>::new(), "only off");
    assert_eq!(catalog.s.efforts, Vec::<u8>::new(), "no levels");
    assert_eq!(catalog.h.efforts, [0, 1, 2, 3]);
    assert_eq!(
        catalog.g.efforts,
        Vec::<u8>::new(),
        "G is read like the others"
    );
    // The empty list is what the cache keeps, and a hit gives it back.
    let (runner, again) = rig.load(vec![]);
    assert_eq!(again.unwrap(), catalog);
    assert_eq!(runner.calls().len(), 0);
}

/// pi's `minimal` and `max` have no effort digit: they are dropped; levels pi does not offer leave gaps, which are kept.
#[test]
fn minimal_and_max_are_dropped_and_gaps_are_kept() {
    let mut rig = Rig::new();
    let (_, result) = rig.load(vec![all_levels(
        &["off", "medium", "high", "xhigh"],
        &["minimal", "off", "low", "medium", "high", "xhigh", "max"],
        &["minimal", "low", "high", "max"],
        &["off"],
    )]);
    let catalog = result.unwrap();
    assert_eq!(catalog.o.efforts, [0, 2, 3, 4], "a gap is kept");
    assert_eq!(
        catalog.s.efforts,
        [0, 1, 2, 3, 4],
        "minimal and max are dropped"
    );
    assert_eq!(catalog.h.efforts, [1, 3]);
    assert_eq!(catalog.h.efforts.iter().max(), Some(&3), "no effort 5");
}

/// G is a letter like the others: its rows are written to `tier_catalog` and read back with their efforts.
#[test]
fn g_rows_are_cached_and_read_back_with_their_efforts() {
    let mut rig = Rig::new();
    let (_, first) = rig.load(vec![all_levels(
        &["off", "low"],
        &["off", "low"],
        &["off", "low"],
        &["off", "medium", "high"],
    )]);
    let first = first.unwrap();
    assert_eq!(first.g.efforts, [0, 2, 3]);
    assert_eq!(rig.rows(), 4, "one row per letter, G included");
    let (runner, second) = rig.load(vec![]);
    assert_eq!(second.unwrap(), first, "a hit reads G back from the cache");
    assert_eq!(runner.calls().len(), 0);

    // Through the cache's own functions: write a catalog, read it back.
    let models = rig.models.clone();
    let catalog = Catalog {
        pi_version: "2.0.0".into(),
        o: TierModel {
            model: models.o.clone(),
            efforts: vec![0, 2, 3, 4],
        },
        s: TierModel {
            model: models.s.clone(),
            efforts: vec![],
        },
        h: TierModel {
            model: models.h.clone(),
            efforts: vec![1, 3],
        },
        g: TierModel {
            model: models.g.clone(),
            efforts: vec![1, 2],
        },
    };
    cache::write(&mut rig.store, "sha", &catalog).unwrap();
    let refs = [models.o.as_str(), models.s.as_str(), models.h.as_str()];
    let read = cache::read(&mut rig.store, "2.0.0", "sha", refs, &models.g).unwrap();
    assert_eq!(read, Some(catalog));
    let g_rows: i64 = rig
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM tier_catalog WHERE pi_version = '2.0.0' AND letter = 'G'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(g_rows, 1);
}
