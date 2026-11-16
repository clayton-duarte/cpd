//! Alert on an uninterrupted period of observed green checks. Never merges.
use super::{SOURCE, engine::Engine};
use cpd_core::error::CpdError;
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::SessionId;
use rusqlite::OptionalExtension;

#[derive(Default)]
pub(super) struct GreenAlert {
    since: Option<u64>,
    alerted: bool,
}

impl GreenAlert {
    pub(super) fn observe(&mut self, now: u64, green: bool) -> bool {
        if !green {
            self.since = None;
            self.alerted = false;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.alerted && now.saturating_sub(since) >= 1800 {
            self.alerted = true;
            return true;
        }
        false
    }
}

pub(super) fn run(engine: &mut Engine) {
    if let Err(e) = tick(engine) {
        engine.log.line(&format!(
            "green-unmerged: {} ({})",
            e.message(),
            e.code().as_str()
        ));
    }
}

fn tick(engine: &mut Engine) -> Result<(), CpdError> {
    let now = engine.ports.clock.utc().unix_timestamp();
    engine.store.write(SOURCE, |tx| {
        tx.execute("DELETE FROM green_unmerged_periods WHERE pull_request NOT IN (SELECT p.id FROM pull_requests p WHERE p.state = 'open' AND p.checks = 'passed' AND EXISTS (SELECT 1 FROM workflows w WHERE w.pull_request = p.id AND w.status = 'open'))", [])?;
        tx.execute("INSERT OR IGNORE INTO green_unmerged_periods (pull_request, since, alerted) SELECT p.id, ?1, 0 FROM pull_requests p WHERE p.state = 'open' AND p.checks = 'passed' AND EXISTS (SELECT 1 FROM workflows w WHERE w.pull_request = p.id AND w.status = 'open')", [now])?;
        let mut stmt = tx.prepare("SELECT g.pull_request, g.since, g.alerted, p.repo, p.number FROM green_unmerged_periods g JOIN pull_requests p ON p.id = g.pull_request")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for (id, since, alerted, repo, number) in rows {
            let mut alert = GreenAlert { since: Some(since.max(0) as u64), alerted };
            if !alert.observe(now.max(0) as u64, true) { continue; }
            let manager: Option<String> = tx.query_row("SELECT id FROM sessions WHERE role = 'manager' AND context = 'cpd' AND archived_at IS NULL ORDER BY created_at, id LIMIT 1", [], |r| r.get(0)).optional()?;
            if let Some(manager) = manager {
                let text = format!("{repo}#{number} has been green and unmerged for 30 minutes. Report readiness to the User; this alert does not authorize a merge.");
                events::append(tx, NewEvent::session_message(&text, "cpd", &text, true, None)?.session(SessionId::parse(&manager)?))?;
                tx.execute("UPDATE green_unmerged_periods SET alerted = 1 WHERE pull_request = ?1", [id])?;
            }
        }
        Ok(())
    })
}
