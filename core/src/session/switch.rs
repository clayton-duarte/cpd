//! `session switch-failed` and `session switched` (commands.md §10.18a, §10.18b): the pi adapter records on the row what its
//! model switch did. Data writes, no Event; idempotent.

use serde_json::json;

use super::row::{self, Role};
use super::{Env, SwitchFailedArgs, SwitchedArgs, not_a_lead};
use crate::args::SessionArg;
use crate::clock;
use crate::error::CpdError;
use crate::ids::SessionId;
use crate::output::Output;
use crate::session::row::Row;

/// Writes `switch_failed` of the Session `reference` names: `record` gets the row it is written for, in the transaction.
fn write(reference: Option<crate::args::SessionRef>, what: &str, record: impl Fn(&Row) -> Option<String>) -> Result<Output, CpdError> {
    let reference = SessionArg { session: reference }.require()?.clone();
    let mut env = Env::open()?;
    let id: SessionId = env.resolve(&reference)?;
    env.store.write(what, |tx| {
        let row = row::load(tx, &id)?;
        if row.role == Role::Worker {
            return Err(not_a_lead(&row.id, row.role));
        }
        tx.execute("UPDATE sessions SET switch_failed = ?1 WHERE id = ?2", rusqlite::params![record(&row), row.id.as_str()])?;
        Ok(())
    })?;
    Ok(Output::success(json!({"session": id})))
}

pub(super) fn failed(args: SwitchFailedArgs) -> Result<Output, CpdError> {
    let SwitchFailedArgs { code, message, session } = args;
    let at = clock::now();
    write(session, "session switch-failed", |row| {
        Some(json!({"tier": row.tier, "code": code.0, "message": message.0, "at": at}).to_string())
    })
}

pub(super) fn switched(args: SwitchedArgs) -> Result<Output, CpdError> {
    write(args.session, "session switched", |_| None)
}
