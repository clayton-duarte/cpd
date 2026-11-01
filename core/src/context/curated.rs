//! The Context's curated records in force (contract §4.6 "Consumers"; hs-d549, hs-d551): the view
//! `context show --topic` reads. The curator writes `topic`/`short` through `record_curations`; this
//! module only reads them, in one transaction with the records and their retirements.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use super::Record;
use super::inforce::{in_force_without, retired};
use super::render::Indexed;
use crate::error::CpdError;
use crate::ids::{ContextId, RecordId};

/// The Context's records and curations, read in one transaction.
struct Loaded {
    records: Vec<Indexed>,
    retired: HashSet<RecordId>,
    curations: HashMap<String, (String, String)>,
}

fn load(conn: &Connection, context: &ContextId) -> Result<Loaded, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq",
        Record::COLUMNS
    ))?;
    let records = stmt
        .query_map([context.as_str()], Record::from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt =
        conn.prepare("SELECT record, topic, short FROM record_curations WHERE context = ?1")?;
    let curations = stmt
        .query_map([context.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, String>(1)?, r.get::<_, String>(2)?),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Loaded {
        records,
        retired: retired(conn, context)?,
        curations,
    })
}

/// The curated records in force, oldest `seq` first: every record, the `topic` and `short` the curator gave it.
fn curated(loaded: &Loaded) -> Vec<(&Record, &str, &str)> {
    let in_force = in_force_without(&loaded.records, i64::MAX, &loaded.retired);
    in_force
        .iter()
        .filter_map(|(_, record)| {
            let (topic, short) = loaded.curations.get(&record.id.to_string())?;
            Some((record, topic.as_str(), short.as_str()))
        })
        .collect()
}

/// Every curated record in force of `topic`, oldest first: id and short form.
pub fn by_topic(
    conn: &Connection,
    context: &ContextId,
    topic: &str,
) -> Result<Vec<(String, String)>, CpdError> {
    let loaded = load(conn, context)?;
    Ok(curated(&loaded)
        .into_iter()
        .filter(|(_, t, _)| *t == topic)
        .map(|(record, _, short)| (record.id.to_string(), short.to_owned()))
        .collect())
}

/// The topics in use (a curated record in force has them), sorted.
pub fn topics(conn: &Connection, context: &ContextId) -> Result<Vec<String>, CpdError> {
    let loaded = load(conn, context)?;
    Ok(curated(&loaded)
        .into_iter()
        .map(|(_, topic, _)| topic.to_owned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}
