//! The Context digest (contract §4.6 "The Context digest", "Consumers"; hs-d549, hs-d551): rendered by cpd from the
//! tables on demand, with no model and no file. Short forms by topic, then the topic list; [`first_message`] is the
//! one rendering of "the Context" for a first message.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::Connection;

use super::inforce::{in_force_without, retired};
use super::render::{Indexed, raw};
use super::Record;
use crate::config::Config;
use crate::error::CpdError;
use crate::ids::{ContextId, RecordKind};

/// The size limit of a digest, in bytes (32 KB; contract §4.6).
pub const LIMIT: usize = 32_768;

/// The most raw records a first message lists after the digest (contract §4.6 "Consumers").
pub const NEWER_MAX: usize = 20;

/// A rendered digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    pub text: String,
    /// The relevant records listed (not the dropped ones).
    pub records: usize,
}

/// A curated record in force.
struct Curated<'a> {
    seq: i64,
    record: &'a Record,
    topic: String,
    short: String,
}

/// The Context's records and curations, read in one transaction.
struct Loaded {
    records: Vec<Indexed>,
    retired: HashSet<crate::ids::RecordId>,
    curations: HashMap<String, (String, String)>,
}

fn load(conn: &Connection, context: &ContextId) -> Result<Loaded, CpdError> {
    let mut stmt = conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
    let records = stmt.query_map([context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT record, topic, short FROM record_curations WHERE context = ?1")?;
    let curations = stmt
        .query_map([context.as_str()], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, String>(1)?, r.get::<_, String>(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Loaded { records, retired: retired(conn, context)?, curations })
}

/// The curated records in force, oldest `seq` first.
fn curated<'a>(loaded: &'a Loaded) -> (Vec<&'a Indexed>, Vec<Curated<'a>>) {
    let in_force = in_force_without(&loaded.records, i64::MAX, &loaded.retired);
    let curated = in_force
        .iter()
        .filter_map(|(seq, record)| {
            let (topic, short) = loaded.curations.get(&record.id.to_string())?;
            Some(Curated { seq: *seq, record, topic: topic.clone(), short: short.clone() })
        })
        .collect();
    (in_force, curated)
}

/// The ids a brief of a live Plan cites: every word of its text that is a record id of the Context.
fn brief_citations(conn: &Connection, loaded: &Loaded) -> Result<HashSet<String>, CpdError> {
    let mut stmt = conn.prepare("SELECT id FROM plans WHERE deleted_at IS NULL AND archived_at IS NULL AND phase <> 'done'")?;
    let live: HashSet<String> = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
    let mut cited = HashSet::new();
    for (_, record) in &loaded.records {
        if record.kind() == RecordKind::Brief && record.plan().is_some_and(|p| live.contains(p.id.as_str())) {
            let words = record.text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'));
            cited.extend(words.map(|w| w.trim_matches('-').to_owned()).filter(|w| !w.is_empty()));
        }
    }
    Ok(cited)
}

/// Renders the digest of `context`: the relevant curated records in force by topic, then the topic list. Empty when
/// no record is curated.
pub fn render(conn: &Connection, context: &ContextId, recent_days: u32) -> Result<Digest, CpdError> {
    let loaded = load(conn, context)?;
    let (in_force, curated) = curated(&loaded);
    let cutoff: String = conn.query_row(
        &format!("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{recent_days} days')"),
        [],
        |r| r.get(0),
    )?;
    let answered: HashSet<&crate::ids::RecordId> = in_force.iter().filter_map(|(_, r)| r.question()).collect();
    let cited = brief_citations(conn, &loaded)?;
    let mut relevant: Vec<&Curated> = curated
        .iter()
        .filter(|c| {
            c.record.at.as_str() >= cutoff.as_str()
                || (c.record.kind() == RecordKind::Question && !answered.contains(&c.record.id))
                || cited.contains(&c.record.id.to_string())
        })
        .collect();
    relevant.sort_by(|a, b| (&a.record.at, a.seq).cmp(&(&b.record.at, b.seq)));

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &curated {
        *counts.entry(c.topic.as_str()).or_default() += 1;
    }
    if counts.is_empty() {
        return Ok(Digest { text: String::new(), records: 0 });
    }
    // Topics by their newest relevant record, newest first.
    let mut newest: Vec<(&str, usize)> = Vec::new();
    for (i, c) in relevant.iter().enumerate() {
        match newest.iter_mut().find(|(t, _)| *t == c.topic) {
            Some(entry) => entry.1 = i,
            None => newest.push((c.topic.as_str(), i)),
        }
    }
    newest.sort_by(|a, b| b.1.cmp(&a.1));
    let topic_list: Vec<String> = counts.iter().map(|(t, n)| format!("- {t} ({n})")).collect();

    let build = |dropped: usize| -> String {
        let mut sections: Vec<String> = Vec::new();
        for (topic, _) in &newest {
            let mut lines = vec![format!("## {topic}")];
            lines.extend(relevant[dropped..].iter().filter(|c| c.topic == *topic).map(|c| format!("- [{}] {}", c.record.id, c.short)));
            let older = relevant[..dropped].iter().filter(|c| c.topic == *topic).count();
            if older > 0 {
                lines.push(format!("and {older} older: cpd context show --topic {topic}"));
            }
            if lines.len() > 1 {
                sections.push(lines.join("\n"));
            }
        }
        sections.push(format!("## Topics\n{}", topic_list.join("\n")));
        sections.join("\n\n")
    };
    let mut dropped = 0;
    let mut text = build(0);
    while text.len() > LIMIT && dropped < relevant.len() {
        dropped += 1;
        text = build(dropped);
    }
    Ok(Digest { text, records: relevant.len() - dropped })
}

/// Every curated record in force of `topic`, oldest first: id and short form.
pub fn by_topic(conn: &Connection, context: &ContextId, topic: &str) -> Result<Vec<(String, String)>, CpdError> {
    let loaded = load(conn, context)?;
    let (_, curated) = curated(&loaded);
    Ok(curated.into_iter().filter(|c| c.topic == topic).map(|c| (c.record.id.to_string(), c.short)).collect())
}

/// "The Context" of a Lead's or Manager's first message (contract §4.6 "Consumers"): the digest, then the records in
/// force that are not curated, raw, oldest first, at most [`NEWER_MAX`], then, when more exist,
/// `and <K> more: cpd context show --context <c> --since <seq>`, `<seq>` the `seq` before the first record not listed.
pub(crate) fn first_message(conn: &Connection, config: &Config, context: &ContextId) -> Result<String, CpdError> {
    let digest = render(conn, context, config.context_recent_days)?;
    let loaded = load(conn, context)?;
    let (in_force, _) = curated(&loaded);
    let uncurated: Vec<&&Indexed> = in_force.iter().filter(|(_, r)| !loaded.curations.contains_key(&r.id.to_string())).collect();
    let mut parts = Vec::new();
    if !digest.text.is_empty() {
        parts.push(digest.text);
    }
    parts.extend(uncurated.iter().take(NEWER_MAX).map(|(_, r)| raw(r)));
    if uncurated.len() > NEWER_MAX {
        let before = uncurated[NEWER_MAX].0 - 1;
        parts.push(format!("and {} more: cpd context show --context {context} --since {before}", uncurated.len() - NEWER_MAX));
    }
    Ok(parts.join("\n\n"))
}

/// A Manager's first message (commands.md §3.10): its header, then "the Context" as [`first_message`] renders it.
/// The one rendering `context manager` and `session first-message` share.
pub(crate) fn manager_message(conn: &Connection, config: &Config, context: &ContextId, title: &str) -> Result<String, CpdError> {
    let text = first_message(conn, config, context)?;
    Ok(format!("You are the Manager of Context {context} \"{title}\". Follow the cpd-manager skill.\n\n{text}\n"))
}
