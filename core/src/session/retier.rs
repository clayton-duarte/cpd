//! `session retier` (commands.md §10.15; contract §2.2 Session `tier`, §5.2 `session-retiered`, §6.5 "A Session's
//! Tier while it runs"; hs-d506): moves the live Leads and Managers to the Tier the config names now.
//!
//! The row is all it writes, with one Event per Session it moved. A running pi switches itself at its next idle (the
//! adapter, contract §5.3); a Session with no pi starts on the new Tier (`run`, commands.md §10.8).

use rusqlite::Connection;
use serde_json::{Value, json};

use super::archive::archived;
use super::row::{self, Role, Row};
use super::state::scoped;
use super::{Env, RetierArgs, not_a_lead};
use crate::config::{Config, Tier};
use crate::error::CpdError;
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;

/// The Tier the config gives a Session of this role, `None` for a Worker (its Tier is its Job's).
fn target(config: &Config, row: &Row) -> Option<Tier> {
    match row.role {
        Role::Lead => Some(config.lead_tier),
        Role::Manager => Some(config.manager_tier),
        Role::Worker => None,
    }
}

/// The selected Sessions, each with its target: `<id>` (refused when it may not be retiered), or every live Lead and
/// Manager (`--all`).
fn selected(
    conn: &Connection,
    config: &Config,
    one: Option<&SessionId>,
    chosen: Option<Tier>,
) -> Result<Vec<(Row, Tier)>, CpdError> {
    match one {
        Some(id) => {
            let row = row::load(conn, id)?;
            let Some(tier) = target(config, &row) else {
                return Err(not_a_lead(&row.id, row.role));
            };
            if row.archived_at.is_some() {
                return Err(archived(&row));
            }
            Ok(vec![(row, chosen.unwrap_or(tier))])
        }
        None => Ok(row::live(conn)?
            .into_iter()
            .filter_map(|row| target(config, &row).map(|tier| (row, tier)))
            .collect()),
    }
}

/// `session retier` (commands.md §10.15).
pub(super) fn retier(args: RetierArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let one = args
        .id
        .as_ref()
        .map(|reference| env.resolve(reference))
        .transpose()?;
    let config = env.config.clone();
    if let (Some(tier), Some(id)) = (args.tier, one.as_ref()) {
        let home = env.home.clone();
        let models = env.store.read(|conn| {
            let row = row::load(conn, id)?;
            crate::policy::model::models_of_context(conn, &home, row.context.as_str())
        })?;
        crate::config::catalog::load_with(&models, &mut env.store)?.check(tier)?;
        tier.check_lead()?;
    }
    let chosen = env
        .store
        .read(|conn| selected(conn, &config, one.as_ref(), args.tier))?;
    let (moved, unchanged): (Vec<Value>, Vec<String>) = if chosen
        .iter()
        .any(|(row, tier)| row.tier != tier.to_string())
    {
        // One transaction: the rows are read again in it, so what is written is what was found there.
        env.store.write("session retier", |tx| {
            let (mut moved, mut unchanged) = (Vec::new(), Vec::new());
            for (row, tier) in selected(tx, &config, one.as_ref(), args.tier)? {
                let to = tier.to_string();
                if row.tier == to {
                    unchanged.push(row.id.to_string());
                    continue;
                }
                tx.execute("UPDATE sessions SET tier = ?1, switch_failed = NULL WHERE id = ?2", [to.as_str(), row.id.as_str()])?;
                let text = format!("Session {} moved from {} to {to}.", row.id, row.tier);
                let event = append(tx, scoped(NewEvent::session_retiered(text, &row.tier, &to), &row))?;
                moved.push(json!({"session": row.id, "role": row.role.as_str(), "from": row.tier, "to": to, "event": event}));
            }
            Ok((moved, unchanged))
        })?
    } else {
        (
            Vec::new(),
            chosen.iter().map(|(row, _)| row.id.to_string()).collect(),
        )
    };
    let already = moved.is_empty();
    let mut out = json!({"retiered": moved, "unchanged": unchanged});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}
