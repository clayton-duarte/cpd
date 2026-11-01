//! The `tier_catalog` cache (commands.md §14.3, contract §9): pi's catalog for
//! the configured models, kept per pi version and `models.json` fingerprint. Read through [`Store::read`],
//! written through [`Store::write`], never by other means (contract §4.3).

use rusqlite::{Connection, params};

use super::tier::{Catalog, Letter, TierModel};
use crate::error::{CpdError, ErrorCode};
use crate::store::{Store, WriteTxn};

/// The `txns.command` of the cache's write transaction.
const COMMAND: &str = "tiers";

/// The cached Catalog for `pi_version`, when it holds a row for each of the
/// three configured models (`models` in `O, S, H` order). `None` is a cache miss.
/// `g` is the configured G model: G is never cached (hs-d562), a catalog is built with it.
pub(super) fn read(
    store: &mut Store,
    pi_version: &str,
    models_sha: &str,
    models: [&str; 3],
    g: &str,
) -> Result<Option<Catalog>, CpdError> {
    store.read(|conn| {
        let mut found = Vec::with_capacity(3);
        for (letter, model) in [Letter::O, Letter::S, Letter::H].into_iter().zip(models) {
            match row(conn, pi_version, models_sha, letter, model)? {
                Some(efforts) => found.push(TierModel {
                    model: model.to_owned(),
                    efforts,
                }),
                None => return Ok(None),
            }
        }
        let [o, s, h]: [TierModel; 3] = found.try_into().expect("one model per letter");
        let g = TierModel {
            model: g.to_owned(),
            efforts: vec![],
        };
        Ok(Some(Catalog {
            pi_version: pi_version.to_owned(),
            o,
            s,
            h,
            g,
        }))
    })
}

fn row(
    conn: &Connection,
    pi_version: &str,
    models_sha: &str,
    letter: Letter,
    model: &str,
) -> Result<Option<Vec<u8>>, CpdError> {
    let mut stmt = conn.prepare("SELECT efforts FROM tier_catalog WHERE pi_version = ?1 AND models_sha = ?2 AND letter = ?3 AND model = ?4")?;
    let mut rows = stmt.query(params![pi_version, models_sha, letter.to_string(), model])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let text: String = row.get(0)?;
    let efforts: Vec<u8> = serde_json::from_str(&text).map_err(|e| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("tier_catalog holds invalid efforts for {model}: {e}."),
        )
        .with("pi_version", pi_version)
        .with("models_sha", models_sha)
        .with("letter", letter.to_string())
        .with("model", model)
        .with("efforts", text)
    })?;
    Ok(Some(efforts))
}

/// Inserts `catalog`'s rows in one transaction (a cache: no Event). The table
/// is append-only (§4.4): rows of other versions or models stay, and a row
/// already there (same version, `models_sha`, letter and model, so the same efforts) is kept.
pub(super) fn write(
    store: &mut Store,
    models_sha: &str,
    catalog: &Catalog,
) -> Result<(), CpdError> {
    store.write(COMMAND, |tx: &WriteTxn<'_>| {
        for letter in [Letter::O, Letter::S, Letter::H] {
            let TierModel { model, efforts } = catalog.model(letter);
            tx.execute(
                "INSERT OR IGNORE INTO tier_catalog (pi_version, models_sha, letter, model, efforts, read_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![catalog.pi_version, models_sha, letter.to_string(), model, serde_json::to_string(efforts).expect("a list of numbers")],
            )?;
        }
        Ok(())
    })
}
