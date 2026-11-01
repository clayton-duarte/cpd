//! The config migrations (contract §8.1 "Config migrations", commands.md §14.2, hs-d722):
//! `distill-tier-gx`, `drop-test-concurrency` and `drop-plannotator-ports`, the ordered, additive, idempotent,
//! comment-preserving transforms `cpd config --migrate` applies before the binary is replaced.

use super::migrate::{MIGRATIONS, apply};

/// The stale config the acceptance builds its cases from.
const STALE: &str = "version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"H2\"  # stale\ntest_concurrency = 2\nconcurrency = 5\n";

/// `STALE` after the two migrations: the value rewritten and the key's line removed.
const MIGRATED: &str =
    "version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"GX\"  # stale\nconcurrency = 5\n";

/// Migrations run in the order of [`MIGRATIONS`], whose first two entries are the two
/// named in the contract, then `drop-plannotator-ports`.
#[test]
fn migrate_reports_the_applied_names_in_order() {
    let (_, names) = apply(STALE);
    assert_eq!(names, vec!["distill-tier-gx", "drop-test-concurrency"]);
    assert_eq!(
        MIGRATIONS.iter().map(|m| m.name).collect::<Vec<_>>(),
        vec![
            "distill-tier-gx",
            "drop-test-concurrency",
            "drop-plannotator-ports"
        ]
    );

    // A config only one migration touches reports only that one.
    let (_, names) = apply("version = 1\nmachine = \"hs\"\ntest_concurrency = 2\n");
    assert_eq!(names, vec!["drop-test-concurrency"]);
}

/// `context_distill_tier`'s `"H2"` becomes `"GX"`, and only that value changes.
#[test]
fn migrate_rewrites_only_h2_to_gx() {
    let (text, names) = apply(STALE);
    assert_eq!(text, MIGRATED, "only context_distill_tier's H2 becomes GX");
    assert!(names.contains(&"distill-tier-gx"), "{names:?}");
}

/// The top-level `test_concurrency` line is removed.
#[test]
fn migrate_drops_test_concurrency() {
    let (text, names) = apply(STALE);
    assert!(!text.contains("test_concurrency"), "{text}");
    assert!(text.contains("concurrency = 5\n"), "{text}");
    assert!(names.contains(&"drop-test-concurrency"), "{names:?}");
}

/// Applying the migrations to the migrated text changes nothing and reports no name.
#[test]
fn migrate_is_idempotent() {
    let (text, names) = apply(MIGRATED);
    assert_eq!(text, MIGRATED);
    assert!(names.is_empty(), "{names:?}");
}

/// Every other line, every comment and the formatting survive byte for byte: a config with
/// comments and unrelated keys, including a comment on the changed line.
#[test]
fn migrate_keeps_every_other_line_and_comment() {
    let input = "# the machine's config\nversion = 1\nmachine = \"hs\"  # the machine\n\ncontext_distill_tier = \"H2\"  # stale, fixed by the migration\nconcurrency = 5\ntest_concurrency = 2  # the old pool\n\n[timeouts]\nattempt_minutes = 30\n";
    let expected = "# the machine's config\nversion = 1\nmachine = \"hs\"  # the machine\n\ncontext_distill_tier = \"GX\"  # stale, fixed by the migration\nconcurrency = 5\n\n[timeouts]\nattempt_minutes = 30\n";
    let (text, _) = apply(input);
    assert_eq!(text, expected);
}

/// A `context_distill_tier` that is not `"H2"` is left exactly as it is, with no name reported.
#[test]
fn migrate_leaves_a_non_h2_value_alone() {
    for value in ["H0", "S1", "GX"] {
        let input = format!("version = 1\nmachine = \"hs\"\ncontext_distill_tier = \"{value}\"\n");
        let (text, names) = apply(&input);
        assert_eq!(text, input, "{value}");
        assert!(names.is_empty(), "{value}: {names:?}");
    }
}

/// The top-level `plannotator_ports` line is removed, its neighbours and comments kept.
#[test]
fn migrate_drops_plannotator_ports() {
    let input =
        "version = 1\n# keep\nplannotator_ports = \"19432-19456\"  # old\nconcurrency = 5\n";
    let (text, names) = apply(input);
    assert_eq!(text, "version = 1\n# keep\nconcurrency = 5\n");
    assert_eq!(names, vec!["drop-plannotator-ports"]);
}
