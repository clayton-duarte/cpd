//! Config migrations (contract §8.1 "Config migrations", commands.md §14.2, hs-d722):
//! an ordered, additive list of idempotent transforms over `config.toml`, applied by
//! `cpd config --migrate` — and so by `cpd install` before the binary is replaced.

/// One transform of `config.toml`. `run` returns the migrated text only when the stale
/// state is present and its text changed, else `None`; it never fails.
#[allow(dead_code)]
pub struct ConfigMigration {
    /// The stable name `cpd config --migrate` reports and `cpd install` prints.
    pub name: &'static str,
    /// A human sentence of what the transform does.
    pub description: &'static str,
    /// The transform: `Some(new_text)` when it changed the text, else `None`.
    #[allow(dead_code)]
    pub run: fn(&str) -> Option<String>,
}

/// The migrations in the order they are applied (contract §8.1): `distill-tier-gx` then
/// `drop-test-concurrency`.
#[allow(dead_code)]
pub const MIGRATIONS: &[ConfigMigration] = &[
    ConfigMigration {
        name: "distill-tier-gx",
        description: "rewrite a top-level context_distill_tier \"H2\" to \"GX\" (hs-d722, hs-d562)",
        run: distill_tier_gx,
    },
    ConfigMigration {
        name: "drop-test-concurrency",
        description: "remove the top-level test_concurrency key (hs-d722, hs-d679)",
        run: drop_test_concurrency,
    },
];

/// Folds the migrations in order over `text`, returning the final text and the names of
/// the migrations that changed it, in list order (contract §8.1).
#[allow(dead_code)]
pub fn apply(_text: &str) -> (String, Vec<&'static str>) {
    todo!()
}

/// `distill-tier-gx` (hs-d722): a top-level `context_distill_tier` whose value is exactly
/// `"H2"` becomes `"GX"`; every other line, comment and the formatting survive.
fn distill_tier_gx(_text: &str) -> Option<String> {
    todo!()
}

/// `drop-test-concurrency` (hs-d679): the top-level `test_concurrency` line is removed; every
/// other line, comment and the formatting survive.
fn drop_test_concurrency(_text: &str) -> Option<String> {
    todo!()
}
