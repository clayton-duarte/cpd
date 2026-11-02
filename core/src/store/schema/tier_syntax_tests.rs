//! The migration `tier-syntax-g-x` (contract §4.4, §4.5; Tier model v2): `jobs.tier` accepts `<G|H|S|O><0-4|X>`
//! (old rows are kept byte for byte) and `tier_catalog` accepts a `G` row and is rebuilt empty, as a cache.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn job(key: &str, tier: &str) -> String {
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
           VALUES ('pay', 'api', '{key}', 'T', '{tier}', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 1, 'work', NULL, cpd_txn(), 't');"
    )
}

/// A store as every migration before `tier-syntax-g-x` leaves it, with cached rows and a Job `old`. The test fails here
/// while the migration is missing.
fn before_tier_syntax() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-tier-syntax-g-x"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "tier-syntax-g-x is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('pay', 'ctx', 'Plan', 'plan', 'lead', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');
             {}
             INSERT INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at) VALUES
               ('1.3.0', '', 'O', 'bifrost/vertex/claude-opus-5', '[0,2,3,4]', 't1'),
               ('1.3.0', '', 'S', 'bifrost/vertex/claude-sonnet-5-5', '[0,1,2,3]', 't1'),
               ('1.3.0', '', 'H', 'bifrost/vertex/claude-haiku-5', '[0,1,2,3]', 't1');",
            job("old", "S2")
        ),
    )
    .unwrap();
    drop(store);
    (dir, home)
}

fn migrated() -> (tempfile::TempDir, Store) {
    let (dir, home) = before_tier_syntax();
    let store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    (dir, store)
}

#[test]
fn jobs_accept_g_and_x_tiers_and_keep_old_rows() {
    let (_dir, mut store) = migrated();
    for (key, tier) in [("gx", "GX"), ("hx", "HX"), ("g2", "G2"), ("o4", "O4")] {
        write(&mut store, &job(key, tier)).unwrap_or_else(|e| panic!("{tier}: {e:?}"));
    }
    for (key, tier) in [
        ("five", "S5"),
        ("low", "s2"),
        ("long", "S22"),
        ("bare", "S"),
    ] {
        let refused = write(&mut store, &job(key, tier));
        assert!(
            format!("{refused:?}").contains("CHECK constraint failed"),
            "{tier}: {refused:?}"
        );
    }
    let old: String = store
        .read(|c| Ok(c.query_row("SELECT tier FROM jobs WHERE key = 'old'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(old, "S2", "an old row is kept byte for byte");
    let all: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM jobs", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(all, 5);
}

#[test]
fn tier_catalog_accepts_g_rows_and_is_rebuilt_empty() {
    let (_dir, mut store) = migrated();
    let rows: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM tier_catalog", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(
        rows, 0,
        "the cache is dropped: the next `cpd tiers` recomputes it"
    );
    write(
        &mut store,
        "INSERT INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at)
           VALUES ('1.3.0', '', 'G', 'google/gemini-4', '[]', 't')",
    )
    .expect("a G row");
    let bad = write(
        &mut store,
        "INSERT INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at)
           VALUES ('1.3.0', '', 'Z', 'm', '[]', 't')",
    );
    assert!(
        format!("{bad:?}").contains("CHECK constraint failed"),
        "{bad:?}"
    );
}
