//! E8, a repeated ask (contract §3.7 "A repeated ask"): ONE message per (plan, job), ever, to the live Manager of the
//! Job's Context. The record that it was sent is the message itself.

use super::SOURCE;
use super::report::{live_manager, message_exists};
use cpd_core::error::CpdError;
use cpd_core::inbox::asks::RepeatedAsk;
use cpd_core::session::tell_manager_of;
use cpd_core::store::Store;

/// What every message about a Job's repeated ask begins with: the once-record.
fn prefix(ask: &RepeatedAsk) -> String {
    format!("Job {}/{} asked ", ask.plan, ask.job)
}

fn text(ask: &RepeatedAsk, minutes: i64) -> String {
    format!(
        "{}the same question {} times over {} Attempt(s), none answered, the first {minutes} min ago: \"{}\". \
         Nudge its Lead (cpd session send {} \"<text>\" --unblocks \"<reason>\"), or ask the User.",
        prefix(ask),
        ask.count,
        ask.attempts,
        ask.question,
        ask.plan
    )
}

/// Tells each case's Context's live Manager once, ever. With no live Manager nothing is written. Returns how many were sent.
pub(super) fn tell_repeated_asks(
    store: &mut Store,
    asks: &[RepeatedAsk],
) -> Result<usize, CpdError> {
    let mut sent = 0;
    for ask in asks {
        let untold = store.read(|conn| match live_manager(conn, &ask.context)? {
            Some(manager) => Ok(!message_exists(conn, &manager, None, &prefix(ask))?),
            None => Ok(false),
        })?;
        if !untold {
            continue;
        }
        let text = text(ask, ask.minutes);
        sent += store.write(SOURCE, |tx| {
            tell_manager_of(tx, Some(ask.context.as_str()), &text)
        })?;
    }
    Ok(sent)
}
