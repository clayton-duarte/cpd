//! `context curate` (commands.md §3.13a): the write-time neighbour curation of one record per run.

use std::collections::{HashMap, HashSet};

use clap::Args;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::curate::{Candidate, Verdict};
use super::curate_judge::{BifrostJudge, JudgePort, JudgeRequest, RecordView};
use super::curate_neighbours::{
    K, Neighbour, TOP, forced_ids, lexical_rank, neighbours, semantic_rank,
};
use super::curate_vectors::{VectorStore, content_hash};
use super::curate_verdict::{
    Gate, Outcome, Reason, Screen, Screened, Side, Why, confirm, gate, screen,
};
use super::curation::{
    Approval, Curation, NotCurable, Proposal, Sweep, decide_pending, not_curable, pending_pairs,
    propose_rejecting,
};
use super::distill::{Lock, State};
use super::{ContextRow, Select};
use crate::classifier::{Choice, ClassifierPort, Label, PiClassifier};
use crate::clock::{self, Clock};
use crate::config::{Config, locate_pi};
use crate::embedding::{BifrostEmbedding, EmbeddingPort};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

/// Records older than this are never picked without `--record`.
pub const WINDOW_HOURS: i64 = 24;
pub const DIMS: usize = 768;
/// The model the real embedder (`pi/lib/embed.ts`) answers with; the lookup before the first embed uses it.
pub(super) const EMBED_MODEL: &str = "vertex/gemini-embedding-2";
/// Missing vectors embedded per run, newest first.
pub const FILL: usize = 25;
/// Pending rows the sweep checks per run, oldest first.
const SWEEP: usize = 3;

pub struct Models<'a> {
    pub classifier: &'a dyn ClassifierPort,
    pub embedding: &'a dyn EmbeddingPort,
    pub judge: &'a dyn JudgePort,
}

/// The question `kev-4b` answers for a pair (contract §4.6 "The pair verdict").
const QUESTION: &str = "How does the new Context record relate to the neighbour record?";

const LABELS: [Label<'static>; 4] = [
    Label {
        name: "duplicate",
        definition: "The new record says the same thing as the neighbour, in other words.",
    },
    Label {
        name: "supersedes",
        definition: "The new record replaces the neighbour: it changes what the neighbour decided or noted.",
    },
    Label {
        name: "contradicts",
        definition: "The two records state opposite things and neither replaces the other.",
    },
    Label {
        name: "independent",
        definition: "The records are about different things, or add to each other without overlap.",
    },
];

/// The bounds of a topic and a short form (as `context distill`, contract §4.6 step 5).
const TOPIC_MAX: usize = 40;
const SHORT_MAX: usize = 100;

fn label_ok(topic: &str, short: &str) -> bool {
    (1..=TOPIC_MAX).contains(&topic.len())
        && topic
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        && (1..=SHORT_MAX).contains(&short.len())
        && !short.contains('\n')
}

fn due(state: &State, checked: &HashSet<RecordId>, now: OffsetDateTime) -> Vec<RecordId> {
    let cutoff = now - time::Duration::hours(WINDOW_HOURS);
    state
        .in_force_records()
        .into_iter()
        .filter(|(_, r)| gate(r.kind()) != Gate::Skip && !checked.contains(&r.id))
        .filter(|(_, r)| OffsetDateTime::parse(&r.at, &Rfc3339).is_ok_and(|at| at >= cutoff))
        .map(|(_, r)| r.id.clone())
        .collect()
}

/// The oldest in-force record of kind decision or note (gate != Skip), not in `checked`, written
/// within [`WINDOW_HOURS`] before `now` (RFC 3339 `at`); `None` when there is none.
pub fn next_record(
    state: &State,
    checked: &HashSet<RecordId>,
    now: OffsetDateTime,
) -> Option<RecordId> {
    due(state, checked, now).into_iter().next()
}

/// What one run counted.
#[derive(Default)]
struct Tally {
    free: usize,
    judged: usize,
    disagreed: usize,
    quote_failed: usize,
    rejected: usize,
    kev_unreachable: bool,
    retire: Vec<(RecordId, RecordId, Reason)>,
    /// The retired records both models agreed on (written approved).
    approved: HashSet<RecordId>,
    labels: Vec<(RecordId, String, String)>,
    /// `(record, topic, model)`: a provisional topic (`record_topics`).
    provisional: Vec<(RecordId, String, String)>,
    contradicts: Vec<Value>,
    embed_tokens: u64,
    kev_tokens_in: u64,
    kev_tokens_out: u64,
    judge_tokens_in: u64,
    judge_tokens_out: u64,
    /// The sum of the judge costs the gateway reported; `None` when it reported none.
    cost: Option<f64>,
}

impl Tally {
    fn add_kev(&mut self, usage: Option<crate::classifier::Usage>) {
        if let Some(u) = usage {
            self.kev_tokens_in += u.input_tokens;
            self.kev_tokens_out += u.output_tokens;
        }
    }

    fn add_judge(&mut self, usage: &Option<super::curate::JudgeUsage>) {
        if let Some(u) = usage {
            self.judge_tokens_in += u.tokens_in;
            self.judge_tokens_out += u.tokens_out;
            if let Some(c) = u.cost {
                self.cost = Some(self.cost.unwrap_or(0.0) + c);
            }
        }
    }
}

struct Pair<'a> {
    new: &'a Candidate,
    other: &'a Candidate,
    new_topic: Option<&'a str>,
    other_topic: Option<&'a str>,
}

fn view<'a>(c: &'a Candidate, topic: Option<&'a str>) -> RecordView<'a> {
    RecordView {
        id: &c.id,
        kind: c.kind,
        at: &c.at,
        topic,
        text: &c.text,
    }
}

fn side(c: &Candidate) -> Side<'_> {
    Side {
        id: &c.id,
        seq: c.seq,
        text: &c.text,
    }
}

fn candidates(state: &State) -> Vec<Candidate> {
    state
        .in_force_records()
        .into_iter()
        .map(|(seq, r)| Candidate {
            id: r.id.clone(),
            seq: *seq,
            kind: r.kind(),
            at: r.at.clone(),
            text: r.text.clone(),
        })
        .collect()
}

fn topics(store: &mut Store, context: &ContextId) -> Result<HashMap<RecordId, String>, CpdError> {
    let rows = store.read(|conn| {
        let mut stmt =
            conn.prepare("SELECT record, topic FROM record_curations WHERE context = ?1")?;
        Ok(stmt
            .query_map([context.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    rows.into_iter()
        .map(|(id, topic)| Ok((RecordId::parse(&id)?, topic)))
        .collect()
}

/// kev-4b's answer for one pair: `Err(())` for an error or a label that is none of the four.
fn ask_kev(
    classifier: &dyn ClassifierPort,
    pair: &Pair,
    unreachable: &mut bool,
) -> Result<Screen, ()> {
    let part =
        |c: &Candidate| json!({"id": c.id.to_string(), "kind": c.kind.as_str(), "text": c.text});
    let state = json!({"new": part(pair.new), "neighbour": part(pair.other)});
    let choice = Choice {
        state: &state,
        question: QUESTION,
        labels: &LABELS,
    };
    let chosen = match classifier.choose(&choice) {
        Ok(chosen) => chosen,
        Err(e) => {
            *unreachable |= matches!(
                e.code(),
                ErrorCode::ClassifierUnreachable
                    | ErrorCode::ClassifierRefused
                    | ErrorCode::ClassifierBadReply
            );
            return Err(());
        }
    };
    tally.add_kev(chosen.usage);
    let verdict = match chosen.label.as_str() {
        "duplicate" => Verdict::Duplicate,
        "supersedes" => Verdict::Supersedes,
        "contradicts" => Verdict::Contradicts,
        "independent" => Verdict::Independent,
        _ => return Err(()),
    };
    Ok(Screen {
        verdict,
        confidence: chosen.confidence.unwrap_or(0.0),
    })
}

/// The question `kev-4b` answers for a record's provisional topic (decision pro-d11).
const TOPIC_QUESTION: &str = "Which topic does the Context record belong to?";
/// A provisional topic is written only at this confidence or above.
const TOPIC_CONFIDENCE: f32 = 0.80;

/// The provisional topic of `new` (an accepted-independent record with no curation and no row yet), chosen among the
/// Context's curated topics in force. Fail-open: an error or a weak or unknown label records nothing.
fn provisional_topic(
    models: &Models,
    store: &mut Store,
    context: &ContextId,
    new: &Candidate,
    state: &State,
    tally: &mut Tally,
) -> Result<(), CpdError> {
    if state.is_curated(&new.id) {
        return Ok(());
    }
    let (topics, has_row) = store.read(|conn| {
        let has_row: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM record_topics WHERE context = ?1 AND record = ?2)",
            [context.as_str(), new.id.to_string().as_str()],
            |r| r.get(0),
        )?;
        Ok((
            super::curated::curated_topic_counts(conn, context)?,
            has_row,
        ))
    })?;
    if has_row || topics.is_empty() {
        return Ok(());
    }
    let definitions: Vec<String> = topics
        .iter()
        .map(|(topic, _)| format!("The record is about {topic}."))
        .collect();
    let labels: Vec<Label> = topics
        .iter()
        .zip(&definitions)
        .map(|((topic, _), definition)| Label {
            name: topic,
            definition,
        })
        .collect();
    let state_json =
        json!({"record": {"id": new.id.to_string(), "kind": new.kind.as_str(), "text": new.text}});
    let choice = Choice {
        state: &state_json,
        question: TOPIC_QUESTION,
        labels: &labels,
    };
    let Ok(chosen) = models.classifier.choose(&choice) else {
        return Ok(());
    };
    tally.add_kev(chosen.usage);
    if chosen.confidence.unwrap_or(0.0) >= TOPIC_CONFIDENCE
        && topics.iter().any(|(topic, _)| *topic == chosen.label)
    {
        tally
            .provisional
            .push((new.id.clone(), chosen.label, chosen.model));
    }
    Ok(())
}

/// A pair checked by both models.
pub(super) struct Verified {
    pub outcome: Outcome,
    /// kev-4b could not answer: the judge was not asked and `outcome` means nothing.
    pub kev_unreachable: bool,
}

/// kev-4b, the screen, the judge and the confirmation of one pair (`new` the later record, `other` the earlier).
/// `Err` only from the judge.
pub(super) fn verify_pair(
    classifier: &dyn ClassifierPort,
    judge: &dyn JudgePort,
    new: &Candidate,
    other: &Candidate,
    new_topic: Option<&str>,
    other_topic: Option<&str>,
) -> Result<Verified, CpdError> {
    let pair = Pair {
        new,
        other,
        new_topic,
        other_topic,
    };
    let mut kev_unreachable = false;
    let kev = ask_kev(classifier, &pair, &mut kev_unreachable);
    let outcome = match screen(kev) {
        _ if kev_unreachable => Outcome::Independent { why: None },
        Screened::AcceptIndependent => Outcome::Independent { why: None },
        Screened::Escalate(kev) => {
            let request = JudgeRequest {
                new: view(new, new_topic),
                neighbour: Some(view(other, other_topic)),
            };
            let judged = judge.judge(&request)?;
            confirm(side(new), side(other), kev, judged)
        }
    };
    Ok(Verified {
        outcome,
        kev_unreachable,
    })
}

/// Decides the oldest pending duplicate/superseded rows (at most [`SWEEP`]) as the models; stops at the first kev-4b
/// failure or judge error. Returns the (approved, rejected) counts decided.
fn sweep(
    store: &mut Store,
    context: &ContextId,
    models: &Models,
) -> Result<(usize, usize), CpdError> {
    let pairs = pending_pairs(store, context, SWEEP)?;
    if pairs.is_empty() {
        return Ok((0, 0));
    }
    let state = store.read(|conn| State::read(conn, context))?;
    let all = candidates(&state);
    let topics = topics(store, context)?;
    let mut decisions = Vec::new();
    for row in pairs {
        let (Some(a), Some(b)) = (
            all.iter().find(|c| c.id == row.record),
            all.iter().find(|c| c.id == row.target),
        ) else {
            continue;
        };
        let (new, other) = if a.seq > b.seq { (a, b) } else { (b, a) };
        let topic = |c: &Candidate| topics.get(&c.id).map(String::as_str);
        let Ok(verified) = verify_pair(
            models.classifier,
            models.judge,
            new,
            other,
            topic(new),
            topic(other),
        ) else {
            break;
        };
        if verified.kev_unreachable {
            break;
        }
        let approve = match verified.outcome {
            Outcome::Contradicts { .. } => continue,
            Outcome::Independent { .. } => false,
            Outcome::Retire {
                record,
                target,
                reason,
                approval,
                ..
            } => {
                let same = match reason {
                    Reason::Duplicate => super::curation::Reason::Duplicate,
                    Reason::Superseded => super::curation::Reason::Superseded,
                } == row.reason;
                if record == row.record && target == row.target && same {
                    if approval != Approval::Models {
                        continue;
                    }
                    true
                } else {
                    false
                }
            }
        };
        decisions.push(Sweep {
            record: row.record,
            approve,
        });
    }
    decide_pending(store, context, &decisions)
}

fn judge_pair(
    models: &Models,
    pair: &Pair,
    kev: Option<Screen>,
    tally: &mut Tally,
) -> Result<(), CpdError> {
    let request = JudgeRequest {
        new: view(pair.new, pair.new_topic),
        neighbour: Some(view(pair.other, pair.other_topic)),
    };
    let judged = models.judge.judge(&request)?;
    tally.judged += 1;
    tally.add_judge(&judged.usage);
    match confirm(side(pair.new), side(pair.other), kev, judged) {
        Outcome::Independent { why } => match why {
            Some(Why::Disagreed) => tally.disagreed += 1,
            Some(Why::QuoteNotFound) => tally.quote_failed += 1,
            Some(Why::Rejected(_)) => tally.rejected += 1,
            None => {}
        },
        Outcome::Retire {
            record,
            target,
            reason,
            topic,
            short,
            approval,
        } => {
            if label_ok(&topic, &short) {
                if approval == Approval::Models {
                    tally.approved.insert(record.clone());
                }
                if !tally.labels.iter().any(|(r, _, _)| *r == record) {
                    tally.labels.push((record.clone(), topic, short));
                }
                if !tally.retire.iter().any(|(r, _, _)| *r == record) {
                    tally.retire.push((record, target, reason));
                }
            } else {
                tally.rejected += 1;
            }
        }
        Outcome::Contradicts {
            neighbour,
            quote,
            reason,
        } => tally
            .contradicts
            .push(json!({"neighbour": neighbour.to_string(), "quote": quote, "reason": reason})),
    }
    Ok(())
}

fn check_pairs(
    models: &Models,
    new: &Candidate,
    found: &[Neighbour],
    all: &[Candidate],
    topics: &HashMap<RecordId, String>,
    tally: &mut Tally,
) -> Result<(), CpdError> {
    for n in found {
        let Some(other) = all.iter().find(|c| c.id == n.id) else {
            continue;
        };
        let pair = Pair {
            new,
            other,
            new_topic: topics.get(&new.id).map(String::as_str),
            other_topic: topics.get(&other.id).map(String::as_str),
        };
        let kev = ask_kev(models.classifier, &pair, &mut tally.kev_unreachable);
        match screen(kev) {
            Screened::AcceptIndependent => tally.free += 1,
            Screened::Escalate(kev) => judge_pair(models, &pair, kev, tally)?,
        }
    }
    Ok(())
}

/// The semantic list, or `None` (and the run goes FTS-only) when the embedder or `vectors.db` fails.
fn semantic(
    models: &Models,
    vectors: &VectorStore,
    context: &ContextId,
    new: &Candidate,
    all: &[Candidate],
    tokens: &mut u64,
) -> Result<Vec<RecordId>, CpdError> {
    let query = models.embedding.embed(&new.text, DIMS)?;
    *tokens += query.tokens.unwrap_or(0);
    let model = query.model.as_str();
    vectors.put_hashed(
        context,
        model,
        &new.id,
        &query.vector,
        &content_hash(&new.text),
    )?;
    let have = vectors.embedded(context, model)?;
    let missing = all
        .iter()
        .rev()
        .filter(|c| c.id != new.id && !have.contains(&c.id))
        .take(FILL);
    for c in missing {
        let e = models.embedding.embed(&c.text, DIMS)?;
        *tokens += e.tokens.unwrap_or(0);
        vectors.put_hashed(context, &e.model, &c.id, &e.vector, &content_hash(&c.text))?;
    }
    let ids: Vec<RecordId> = all.iter().map(|c| c.id.clone()).collect();
    let found = vectors.get(context, model, &ids)?;
    Ok(semantic_rank(&query.vector, &found, TOP))
}

/// The neighbours of `new` and whether the run is FTS-only.
fn find_neighbours(
    models: &Models,
    vectors: &VectorStore,
    context: &ContextId,
    new: &Candidate,
    supersedes: &[RecordId],
    all: &[Candidate],
    tokens: &mut u64,
) -> Result<(Vec<Neighbour>, bool), CpdError> {
    let forced = forced_ids(&new.text, supersedes, &new.id, all);
    let others: Vec<Candidate> = all.iter().filter(|c| c.id != new.id).cloned().collect();
    let lexical = lexical_rank(&new.text, &others, TOP)?;
    let semantic = semantic(models, vectors, context, new, all, tokens).ok();
    let fts_only = semantic.is_none();
    Ok((neighbours(&new.id, forced, lexical, semantic), fts_only))
}

/// The one write of a run (A8): only a confirmed retirement or a label writes anything.
fn write(
    store: &mut Store,
    context: &ContextId,
    state: &State,
    tally: &Tally,
    at: &str,
) -> Result<(), CpdError> {
    if tally.retire.is_empty() && tally.labels.is_empty() && tally.provisional.is_empty() {
        return Ok(());
    }
    let curations: Vec<Curation> = tally
        .labels
        .iter()
        .filter(|(record, _, _)| !state.is_curated(record))
        .map(|(record, topic, short)| Curation {
            record: record.clone(),
            topic: topic.clone(),
            short: short.clone(),
        })
        .collect();
    let proposals: Vec<Proposal> = tally
        .retire
        .iter()
        .map(|(record, target, reason)| Proposal {
            record: record.clone(),
            reason: match reason {
                Reason::Duplicate => super::curation::Reason::Duplicate,
                Reason::Superseded => super::curation::Reason::Superseded,
            },
            target: target.to_string(),
            approval: if tally.approved.contains(record) {
                Approval::Models
            } else {
                Approval::Pending
            },
        })
        .collect();
    if !curations.is_empty() || !proposals.is_empty() {
        propose_rejecting(store, context, &curations, &proposals, tally.rejected)?;
    }
    if !tally.provisional.is_empty() {
        store.write("context curate", |tx| {
            for (record, topic, model) in &tally.provisional {
                tx.execute(
                    "INSERT INTO record_topics (context, record, topic, model, at) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT DO NOTHING",
                    [context.as_str(), &record.to_string(), topic, model, at],
                )?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn report(
    new: &Candidate,
    count: usize,
    fts_only: bool,
    tally: &Tally,
    remaining: usize,
    context: &ContextId,
    ms: u64,
) -> Value {
    let retire: Vec<Value> = tally
        .retire
        .iter()
        .map(|(record, target, reason)| {
            let reason = match reason {
                Reason::Duplicate => "duplicate",
                Reason::Superseded => "superseded",
            };
            json!({"record": record.to_string(), "target": target.to_string(), "reason": reason})
        })
        .collect();
    json!({
        "context": context.as_str(),
        "record": new.id.to_string(),
        "kind": new.kind.as_str(),
        "neighbours": count,
        "fts_only": fts_only,
        "kev_unreachable": tally.kev_unreachable,
        "free": tally.free,
        "judged": tally.judged,
        "retire": retire,
        "contradicts": tally.contradicts,
        "disagreed": tally.disagreed,
        "quote_failed": tally.quote_failed,
        "rejected": tally.rejected,
        "remaining": remaining,
        "ms": ms,
        "embed_tokens": tally.embed_tokens,
        "kev_tokens_in": tally.kev_tokens_in,
        "kev_tokens_out": tally.kev_tokens_out,
        "judge_tokens_in": tally.judge_tokens_in,
        "judge_tokens_out": tally.judge_tokens_out,
        "cost": tally.cost,
    })
}

fn not_due(context: &ContextId, id: &RecordId) -> CpdError {
    CpdError::new(
        ErrorCode::RecordNotFound,
        format!("{id} is not a decision or note in force of Context {context}."),
    )
    .with("record", id.to_string())
}

#[allow(clippy::too_many_arguments)] // the injected ports and clock of one run
pub fn curate_with(
    store: &mut Store,
    home: &CpdHome,
    config: &Config,
    context: &ContextId,
    models: &Models,
    now: OffsetDateTime,
    clock: &dyn Clock,
    only: Option<&RecordId>,
) -> Result<Value, CpdError> {
    let started = clock.now();
    if !config.experimental.write_curation {
        return Ok(json!({"context": context.as_str(), "skipped": "off"}));
    }
    let swept = sweep(store, context, models)?;
    let mut out = curate_record(store, home, context, models, now, only)?;
    if swept != (0, 0) {
        out["swept"] = json!({"approved": swept.0, "rejected": swept.1});
    }
    Ok(out)
}

fn curate_record(
    store: &mut Store,
    home: &CpdHome,
    context: &ContextId,
    models: &Models,
    now: OffsetDateTime,
    only: Option<&RecordId>,
) -> Result<Value, CpdError> {
    let state = store.read(|conn| State::read(conn, context))?;
    let vectors = VectorStore::at(home);
    let (mut checked, mut fts_only) = match vectors.checked(context) {
        Ok(checked) => (checked, false),
        Err(_) => (HashSet::new(), true),
    };
    let all = candidates(&state);
    let picked = match only {
        Some(id) => Some(id.clone()).filter(|id| all.iter().any(|c| c.id == *id)),
        None => next_record(&state, &checked, now),
    };
    let Some(picked) = picked else {
        if let Some(id) = only {
            return Err(not_due(context, id));
        }
        return Ok(json!({"context": context.as_str(), "already": true, "remaining": 0}));
    };
    let new = all
        .iter()
        .find(|c| c.id == picked)
        .expect("picked from all");
    let kind_gate = gate(new.kind);
    if kind_gate == Gate::Skip {
        return Err(not_due(context, &picked));
    }
    let topics = topics(store, context)?;
    let at = clock::timestamp(now);
    let mut tally = Tally::default();
    let supersedes: Vec<RecordId> = state
        .in_force_records()
        .into_iter()
        .find(|(_, r)| r.id == picked)
        .map(|(_, r)| r.supersedes().to_vec())
        .unwrap_or_default();
    let (found, fts) = find_neighbours(
        models,
        &vectors,
        context,
        new,
        &supersedes,
        &all,
        &mut tally.embed_tokens,
    )?;
    fts_only |= fts;
    let count = found.len().min(K);
    check_pairs(models, new, &found, &all, &topics, &mut tally)?;
    let retired_or_labelled = tally.retire.iter().any(|(r, _, _)| *r == picked)
        || tally.labels.iter().any(|(r, _, _)| *r == picked);
    if tally.free > 0 && !retired_or_labelled {
        provisional_topic(models, store, context, new, &state, &mut tally)?;
    }
    write(store, context, &state, &tally, &at)?;
    vectors.mark_checked(context, &picked, &at)?;
    checked.insert(picked);
    let remaining = due(&state, &checked, now).len();
    let ms = clock.now().saturating_duration_since(started).as_millis() as u64;
    append_run(
        home,
        context,
        json!({
            "kind": "curate",
            "at": clock::timestamp(clock.utc()),
            "context": context.as_str(),
            "ms": ms,
            "embed_tokens": tally.embed_tokens,
            "kev_tokens_in": tally.kev_tokens_in,
            "kev_tokens_out": tally.kev_tokens_out,
            "judge_tokens_in": tally.judge_tokens_in,
            "judge_tokens_out": tally.judge_tokens_out,
            "cost": tally.cost,
            "free": tally.free,
            "judged": tally.judged,
            "retire": tally.retire.len(),
            "contradicts": tally.contradicts.len(),
            "disagreed": tally.disagreed,
            "quote_failed": tally.quote_failed,
            "kev_unreachable": tally.kev_unreachable,
        }),
    );
    Ok(report(new, count, fts_only, &tally, remaining, context, ms))
}

/// Appends one line to the Context's `curate-runs.jsonl`. Best effort, as distill's `log_run`: a failure is ignored.
pub(super) fn append_run(home: &CpdHome, context: &ContextId, line: Value) {
    use std::io::Write;
    let dir = home.context_digest_dir(context);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("curate-runs.jsonl"))
        .and_then(|mut file| writeln!(file, "{line}"));
}

#[derive(Args, Debug)]
pub struct CurateArgs {
    #[command(flatten)]
    select: Select,
    /// Curate this record, whatever its age
    #[arg(long, value_name = "id")]
    record: Option<RecordId>,
}

/// The real embedder: `pi/lib/embed.ts` run by `runner` in pi's agent directory. Shared by curate, embed and search.
pub(super) fn bifrost_embedder(
    config: &Config,
    runner: &std::sync::Arc<dyn crate::host::runner::Runner>,
) -> Result<BifrostEmbedding, CpdError> {
    let agent_dir = locate_pi()?.agent_dir;
    let lib = config.pi_adapter_dir()?.join("lib");
    Ok(BifrostEmbedding::new(
        runner.clone(),
        lib.join("embed.ts"),
        agent_dir,
    ))
}

/// `context curate` (§3.13a).
pub(super) fn curate(args: CurateArgs) -> Result<Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ports,
        ..
    } = Env::open()?;
    let context = args.select.resolve(&mut store)?;
    store.read(|conn| ContextRow::require(conn, &context))?;
    let floor = config.context_curation_min_records;
    let json =
        |skipped: &str| Output::success(json!({"context": context.as_str(), "skipped": skipped}));
    match store.read(|conn| not_curable(conn, &context, floor))? {
        Some(NotCurable::Archived) => return Ok(json("archived")),
        Some(NotCurable::Young { records }) => {
            return Ok(Output::success(
                json!({"context": context.as_str(), "skipped": "young", "records": records, "floor": floor}),
            ));
        }
        Some(NotCurable::Deleted) | None => {}
    }
    if !config.experimental.write_curation {
        return Ok(json("off"));
    }
    let _lock = Lock::take(&home, &context)?;
    let agent_dir = locate_pi()?.agent_dir;
    let lib = config.pi_adapter_dir()?.join("lib");
    let classifier = PiClassifier::new(
        ports.runner.clone(),
        lib.join("classify.ts"),
        agent_dir,
        &home,
    );
    let embedding = bifrost_embedder(&config, &ports.runner)?;
    let judge = BifrostJudge::new(&home, &config, ports.runner.clone());
    let models = Models {
        classifier: &classifier,
        embedding: &embedding,
        judge: &judge,
    };
    let now = ports.clock.utc();
    curate_with(
        &mut store,
        &home,
        &config,
        &context,
        &models,
        now,
        ports.clock.as_ref(),
        args.record.as_ref(),
    )
    .map(Output::success)
}
