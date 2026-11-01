//! `context owner` (commands.md §3.13): sets or changes the owner of a Context while no PR row exists.
//! Sessions already started keep the policy text of the old owner until they restart (contract §8.3).

use clap::Args;
use serde_json::json;

use super::{ContextRow, Select};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::OwnerLogin;
use crate::output::Output;
use crate::policy::{PolicyKey, confirm_fallback, owner_in_use, resolve};

#[derive(Args, Debug)]
pub struct OwnerArgs {
    /// The GitHub owner, its policy key
    login: OwnerLogin,
    #[command(flatten)]
    select: Select,
    /// Link an unknown owner to clayton-duarte's policy (the User confirmed)
    #[arg(long)]
    confirm_fallback: bool,
}

pub(super) fn owner(args: OwnerArgs) -> Result<Output, CpdError> {
    let Env {
        home, mut store, ..
    } = Env::open()?;
    let id = args.select.resolve(&mut store)?;
    let login = &args.login;
    let out = |already: bool| {
        let mut out = json!({"context": {"id": id, "owner": login}});
        if already {
            out["already"] = json!(true);
        }
        Ok(Output::success(out))
    };
    let row = store.read(|conn| ContextRow::require(conn, &id))?;
    if row.owner.as_deref() == Some(login.as_str()) {
        return out(true);
    }
    // An unconfirmed owner with the flag is linked inside the write, after the refusals.
    let link = match resolve(&home, &PolicyKey::Owner(login.clone())) {
        Err(e) if e.code() == ErrorCode::OwnerUnconfirmed && args.confirm_fallback => true,
        other => other.map(|_| false)?,
    };
    let already = store.write("context owner", |tx| {
        let current: Option<String> = tx.query_row(
            "SELECT owner FROM contexts WHERE id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if current.as_deref() == Some(login.as_str()) {
            return Ok(true);
        }
        if let Some(current) = current {
            let mut stmt = tx.prepare(
                "SELECT prs.plan || '/' || prs.key FROM prs JOIN plans ON plans.id = prs.plan
                 WHERE plans.context = ?1 AND plans.deleted_at IS NULL ORDER BY 1",
            )?;
            let prs = stmt
                .query_map([id.as_str()], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !prs.is_empty() {
                return Err(owner_in_use(&id, &OwnerLogin::parse(&current)?, &prs));
            }
        }
        if link {
            confirm_fallback(&home, login)?;
        }
        tx.execute(
            "UPDATE contexts SET owner = ?1 WHERE id = ?2",
            [login.as_str(), id.as_str()],
        )?;
        let text = format!("Context {id} has the owner {login}.");
        append(
            tx,
            NewEvent::context_owner(text, login.as_str()).context(id.clone()),
        )?;
        Ok(false)
    })?;
    out(already)
}
