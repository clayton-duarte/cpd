//! The `models` source (docs/architecture/swr-models.md): the rows of `swr_models`, written from a validated helper
//! answer, and the readers `models show` and the Inbox read. The fetch is the node helper, run by E15 on the Runner
//! port; E15 owns the due and gate rules (`engine/src/watch/models.rs`), so the trait's `due` stays empty.

use time::OffsetDateTime;

use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::inbox::models::{WINDOW, announced};
use crate::models::{
    Answer, Catalog, Changes, Entry, Listed, Outcome, ProviderFailure, Source as Origin, Written,
};
use crate::store::Store;
use crate::swr::endpoint::{self, Kind};
use crate::swr::outbox::{Coalesce, Mutation};

pub struct Models;

impl Source for Models {
    fn system(&self) -> &'static str {
        "models"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["models"]
    }
    fn due(&self, _cx: &mut Cx<'_>, _now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        Ok(Vec::new())
    }
    fn fetch(
        &self,
        _cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        _poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        Ok(())
    }
    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        ExecOutcome::Failed {
            error: "not implemented".into(),
        }
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}

/// The error kind a helper code has: a transient outage can heal by waiting, a permanent one cannot (§4).
pub fn kind_of(code: &str) -> Kind {
    match code {
        "models_key_unset" | "models_unauthorized" | "models_tls" | "models_unparseable" => {
            Kind::Permanent
        }
        _ => Kind::Transient,
    }
}

/// The endpoint a provider's health is kept under: `github-copilot` by name, a custom provider's `host:port`.
fn endpoint_name(provider: &str, base: Option<&str>) -> String {
    base.map(|b| b.split_once("://").map_or(b, |(_, rest)| rest))
        .map(|b| b.split('/').next().unwrap_or(b))
        .filter(|b| !b.is_empty())
        .unwrap_or(provider)
        .to_owned()
}

struct Row {
    catalog: Option<Catalog>,
    changes: Option<Changes>,
    polled_at: Option<String>,
}

fn bad_row(provider: &str, what: &str, e: serde_json::Error) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("The {what} of {provider} in swr_models is not readable: {e}."),
    )
    .with("provider", provider)
}

fn parse<T: serde::de::DeserializeOwned>(
    provider: &str,
    what: &str,
    text: Option<String>,
) -> Result<Option<T>, CpdError> {
    text.map(|t| serde_json::from_str(&t).map_err(|e| bad_row(provider, what, e)))
        .transpose()
}

fn rows(store: &mut Store, provider: Option<&str>) -> Result<Vec<(String, Row)>, CpdError> {
    let raw = store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT provider, catalog_json, changes_json, polled_at FROM swr_models
             WHERE ?1 IS NULL OR provider = ?1 ORDER BY provider",
        )?;
        let rows = stmt
            .query_map([provider], |r| {
                Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    raw.into_iter()
        .map(|(provider, catalog, changes, polled_at)| {
            let row = Row {
                catalog: parse(&provider, "catalog", catalog)?,
                changes: parse(&provider, "changes", changes)?,
                polled_at,
            };
            Ok((provider, row))
        })
        .collect()
}

/// The newest `polled_at` of the rows: `None` when there is no row, or a provider has had no good list yet (its row
/// has no `polled_at`), so E15 is due at once (gated by the run end).
pub fn newest_polled_at(store: &mut Store) -> Result<Option<OffsetDateTime>, CpdError> {
    let newest: Option<String> = store.read(|c| {
        Ok(c.query_row(
            "SELECT CASE WHEN count(*) = count(polled_at) THEN max(polled_at) END FROM swr_models",
            [],
            |r| r.get(0),
        )?)
    })?;
    newest
        .map(|t| {
            OffsetDateTime::parse(&t, &time::format_description::well_known::Rfc3339).map_err(|e| {
                CpdError::new(
                    ErrorCode::StoreFailed,
                    format!("A polled_at in swr_models is not a time: {e}."),
                )
            })
        })
        .transpose()
}

fn sorted(ids: &[String]) -> Vec<String> {
    ids.iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn diff(old: &[String], new: &[String], at: &str) -> Option<Changes> {
    let (old, new): (BTreeSet<_>, BTreeSet<_>) = (old.iter().collect(), new.iter().collect());
    let appeared: Vec<String> = new.difference(&old).map(|s| (*s).clone()).collect();
    let went_away: Vec<String> = old.difference(&new).map(|s| (*s).clone()).collect();
    (!appeared.is_empty() || !went_away.is_empty()).then(|| Changes {
        at: at.to_owned(),
        appeared,
        went_away,
    })
}

fn failure_text(f: &ProviderFailure) -> String {
    format!("{}: {}", f.code, f.message)
}

/// Writes one good list in one transaction (§2.1); unchanged ids move only `polled_at` (§3.2).
fn write_listed(
    store: &mut Store,
    pi_version: &str,
    listed: &Listed,
    now: &str,
) -> Result<(Written, bool), CpdError> {
    let provider = listed.provider.as_str();
    let ids = sorted(&listed.ids);
    let kind = if listed.auth == "custom" {
        "openai-models"
    } else {
        "copilot-available"
    };
    let catalog = Catalog {
        pi_version: pi_version.to_owned(),
        source: Origin {
            kind: kind.to_owned(),
            auth: listed.auth.clone(),
            base: listed.base.clone(),
        },
        ids,
        known: listed.known.as_deref().map(sorted),
    };
    store.write("models refresh", |tx| {
        let seen: Option<(Option<String>, Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT catalog_json, changes_json, last_error FROM swr_models WHERE provider = ?1",
                [provider],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        // The endpoint hears of a provider that is new or was failing; a healthy one stays as it is (D5).
        let recovered = seen.as_ref().is_none_or(|(_, _, e)| e.is_some());
        let (old, kept, _) = seen.unwrap_or((None, None, None));
        let old: Option<Catalog> = parse(provider, "catalog", old)?;
        // Lists of another auth type are not comparable (an API-key login lists more than its plan allows).
        let comparable = old.filter(|o| o.source.auth == listed.auth);
        let this = comparable
            .as_ref()
            .and_then(|o| diff(&o.ids, &catalog.ids, now));
        let (catalog_json, changes_json) = match &comparable {
            // Same ids, same source: the catalog stays as it is and so does the last change.
            Some(o) if this.is_none() && o.source == catalog.source && o.known == catalog.known => {
                (None, None)
            }
            Some(_) => {
                let changes = match &this {
                    Some(c) => Some(json!(c).to_string()),
                    None => kept,
                };
                (Some(json!(catalog).to_string()), Some(changes))
            }
            None => (Some(json!(catalog).to_string()), Some(None)),
        };
        tx.execute(
            "INSERT OR IGNORE INTO swr_models (provider) VALUES (?1)",
            [provider],
        )?;
        tx.execute(
            "UPDATE swr_models SET polled_at = ?2, last_error = NULL, inactive_at = NULL,
               catalog_json = coalesce(?3, catalog_json),
               changes_json = CASE WHEN ?4 THEN ?5 ELSE changes_json END
             WHERE provider = ?1",
            params![
                provider,
                now,
                catalog_json,
                changes_json.is_some(),
                changes_json.flatten()
            ],
        )?;
        let written = Written {
            provider: provider.to_owned(),
            polled_at: now.to_owned(),
            count: catalog.ids.len(),
            changes: this,
        };
        Ok((written, recovered))
    })
}

/// A failed or empty provider keeps its row but for `last_error`; a provider with no row gets one with no catalog.
fn write_failed(store: &mut Store, failure: &ProviderFailure) -> Result<Option<String>, CpdError> {
    let text = failure_text(failure);
    store.write("models refresh", |tx| {
        let base: Option<String> = tx
            .query_row(
                "SELECT catalog_json FROM swr_models WHERE provider = ?1",
                [&failure.provider],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .and_then(|j| serde_json::from_str::<Catalog>(&j).ok())
            .and_then(|c| c.source.base);
        tx.execute(
            "INSERT OR IGNORE INTO swr_models (provider) VALUES (?1)",
            [&failure.provider],
        )?;
        tx.execute(
            "UPDATE swr_models SET last_error = ?2 WHERE provider = ?1",
            params![failure.provider, text],
        )?;
        Ok(base)
    })
}

/// Writes the rows of one validated helper answer, one `Store::write` per provider (swr-models.md §2.1), and feeds
/// each provider's endpoint (§4).
pub fn apply_answer(
    store: &mut Store,
    answer: &Answer,
    now: OffsetDateTime,
) -> Result<Vec<Outcome>, CpdError> {
    let stamp = timestamp(now);
    let mut outcomes = Vec::new();
    for entry in &answer.entries {
        let failure = match entry {
            Entry::Listed(listed) if !listed.ids.is_empty() => {
                let (written, recovered) = write_listed(store, &answer.pi_version, listed, &stamp)?;
                if recovered {
                    let name = endpoint_name(&listed.provider, listed.base.as_deref());
                    endpoint::record_ok(store, &name, now)?;
                }
                outcomes.push(Outcome::Written(written));
                continue;
            }
            Entry::Listed(listed) => ProviderFailure {
                provider: listed.provider.clone(),
                code: "models_empty".to_owned(),
                message: format!(
                    "The list of {} holds no id: the last good list stays.",
                    listed.provider
                ),
                details: serde_json::Map::new(),
            },
            Entry::Failed(failure) => failure.clone(),
        };
        let base = write_failed(store, &failure)?;
        let name = endpoint_name(&failure.provider, base.as_deref());
        // The report is not left to the outage clock: the engine reports a permanent code at once (§4).
        endpoint::record_failure(store, &name, now, &failure_text(&failure), Kind::Transient)?;
        outcomes.push(Outcome::Failed(failure));
    }
    Ok(outcomes)
}

/// The endpoint a failing provider is kept under (§4): what [`apply_answer`] recorded the failure against.
pub fn endpoint_of(store: &mut Store, provider: &str) -> Result<String, CpdError> {
    let base = rows(store, Some(provider))?
        .into_iter()
        .next()
        .and_then(|(_, r)| r.catalog)
        .and_then(|c| c.source.base);
    Ok(endpoint_name(provider, base.as_deref()))
}

/// `cpd models show`: `{providers, warnings}` read from the rows; writes nothing (swr-models.md §5).
pub fn show(
    store: &mut Store,
    provider: Option<&str>,
    now: OffsetDateTime,
) -> Result<Value, CpdError> {
    let mut lists = Vec::new();
    for (name, row) in rows(store, provider)? {
        let (Some(catalog), Some(polled)) = (row.catalog, row.polled_at) else {
            continue;
        };
        let polled_time =
            OffsetDateTime::parse(&polled, &time::format_description::well_known::Rfc3339)
                .map_err(|e| {
                    CpdError::new(
                        ErrorCode::StoreFailed,
                        format!("The polled_at of {name} in swr_models is not a time: {e}."),
                    )
                })?;
        lists.push(json!({
            "provider": name,
            "polled_at": polled,
            "age_seconds": (now - polled_time).whole_seconds().max(0),
            "pi_version": catalog.pi_version,
            "auth": catalog.source.auth,
            "count": catalog.ids.len(),
            "ids": catalog.ids,
            "changes": row.changes,
        }));
    }
    Ok(json!({"providers": lists, "warnings": []}))
}

/// The providers that have a row: a provider with none keeps its open Inbox items.
pub fn providers(store: &mut Store) -> Result<Vec<String>, CpdError> {
    Ok(rows(store, None)?.into_iter().map(|(p, _)| p).collect())
}

/// The change `at` of `provider`, when its row holds one.
pub fn changes_of(store: &mut Store, provider: &str) -> Result<Option<Changes>, CpdError> {
    Ok(rows(store, Some(provider))?
        .into_iter()
        .next()
        .and_then(|(_, r)| r.changes))
}

/// The rows whose `changes_json.at` is within `models::WINDOW` of `now` and whose announced part is not empty.
pub fn changed(store: &mut Store, now: OffsetDateTime) -> Result<Vec<(String, Changes)>, CpdError> {
    let cutoff = now - WINDOW;
    Ok(rows(store, None)?
        .into_iter()
        .filter_map(|(p, r)| r.changes.map(|c| (p, c)))
        .filter(|(_, c)| {
            OffsetDateTime::parse(&c.at, &time::format_description::well_known::Rfc3339)
                .is_ok_and(|at| at >= cutoff)
        })
        .filter(|(p, c)| {
            let (new, gone) = announced(p, c);
            !new.is_empty() || !gone.is_empty()
        })
        .collect())
}
