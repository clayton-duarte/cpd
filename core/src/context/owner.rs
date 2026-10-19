//! `context owner` (commands.md §3.13): sets the owner of a Context that has none.

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::json;

use super::{ContextRow, Select, open};
use crate::error::CpdError;
use crate::events::{Kind, NewEvent, append};
use crate::ids::OwnerLogin;
use crate::output::Output;
use crate::policy::{PolicyKey, owner_set, resolve};

#[derive(Args, Debug)]
pub struct OwnerArgs {
    /// The GitHub owner, its policy key
    login: OwnerLogin,
    #[command(flatten)]
    select: Select,
}

pub(super) fn owner(args: OwnerArgs) -> Result<Output, CpdError> {
    let (home, _, mut store) = open()?;
    let id = args.select.resolve(&mut store)?;
    let login = &args.login;
    let out = |already: bool| {
        let mut out = json!({"context": {"id": id, "owner": login}});
        if already {
            out["already"] = json!(true);
        }
        Ok(Output::success(out))
    };
    // `Some(already)` when the Context has an owner: this one is `already`, another is `owner_set`.
    let has_owner = |owner: Option<String>| -> Result<Option<bool>, CpdError> {
        match owner {
            None => Ok(None),
            Some(o) if o == login.as_str() => Ok(Some(true)),
            Some(o) => Err(owner_set(&id, &OwnerLogin::parse(&o)?)),
        }
    };
    let row = store.read(|conn| ContextRow::require(conn, &id))?;
    if has_owner(row.owner)?.is_some() {
        return out(true);
    }
    resolve(&home, &PolicyKey::Owner(login.clone()))?;
    let already = store.write("context owner", |tx| {
        let changed =
            tx.execute("UPDATE contexts SET owner = ?1 WHERE id = ?2 AND owner IS NULL", [login.as_str(), id.as_str()])?;
        if changed == 0 {
            // Another process set it since the read.
            let owner = tx.query_row("SELECT owner FROM contexts WHERE id = ?1", [id.as_str()], |r| r.get(0)).optional()?;
            return Ok(has_owner(owner.flatten())?.is_some());
        }
        let text = format!("Context {id} has the owner {login}.");
        append(tx, NewEvent::new(Kind::ContextOwner, text, json!({"owner": login})).context(id.clone()))?;
        Ok(false)
    })?;
    out(already)
}
