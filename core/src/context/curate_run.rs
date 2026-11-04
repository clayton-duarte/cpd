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
use super::curation::{Curation, NotCurable, Proposal, not_curable, propose_rejecting};
use super::distill::{Lock, State};
use super::{ContextRow, Select};
use crate::classifier::{Choice, ClassifierPort, Label, PiClassifier};
use crate::clock;
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
    labels: Vec<(RecordId, String, String)>,
    contradicts: Vec<Value>,
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
fn ask_kev(models: &Models, pair: &Pair, tally: &mut Tally) -> Result<Screen, ()> {
    let part =
        |c: &Candidate| json!({"id": c.id.to_string(), "kind": c.kind.as_str(), "text": c.text});
    let state = json!({"new": part(pair.new), "neighbour": part(pair.other)});
    let choice = Choice {
        state: &state,
        question: QUESTION,
        labels: &LABELS,
    };
    let chosen = match models.classifier.choose(&choice) {
        Ok(chosen) => chosen,
        Err(e) => {
            tally.kev_unreachable |= matches!(
                e.code(),
                ErrorCode::ClassifierUnreachable
                    | ErrorCode::ClassifierRefused
                    | ErrorCode::ClassifierBadReply
            );
            return Err(());
        }
    };
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
        } => {
            if label_ok(&topic, &short) {
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
        let kev = ask_kev(models, &pair, tally);
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
) -> Result<Vec<RecordId>, CpdError> {
    let query = models.embedding.embed(&new.text, DIMS)?;
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
) -> Result<(Vec<Neighbour>, bool), CpdError> {
    let forced = forced_ids(&new.text, supersedes, &new.id, all);
    let others: Vec<Candidate> = all.iter().filter(|c| c.id != new.id).cloned().collect();
    let lexical = lexical_rank(&new.text, &others, TOP)?;
    let semantic = semantic(models, vectors, context, new, all).ok();
    let fts_only = semantic.is_none();
    Ok((neighbours(&new.id, forced, lexical, semantic), fts_only))
}

/// The one write of a run (A8): only a confirmed retirement or a label writes anything.
fn write(
    store: &mut Store,
    context: &ContextId,
    state: &State,
    tally: &Tally,
) -> Result<(), CpdError> {
    if tally.retire.is_empty() && tally.labels.is_empty() {
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
        })
        .collect();
    if curations.is_empty() && proposals.is_empty() {
        return Ok(());
    }
    propose_rejecting(store, context, &curations, &proposals, tally.rejected)?;
    Ok(())
}

fn report(
    new: &Candidate,
    count: usize,
    fts_only: bool,
    tally: &Tally,
    remaining: usize,
    context: &ContextId,
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
    })
}

fn not_due(context: &ContextId, id: &RecordId) -> CpdError {
    CpdError::new(
        ErrorCode::RecordNotFound,
        format!("{id} is not a decision or note in force of Context {context}."),
    )
    .with("record", id.to_string())
}

pub fn curate_with(
    store: &mut Store,
    home: &CpdHome,
    config: &Config,
    context: &ContextId,
    models: &Models,
    now: OffsetDateTime,
    only: Option<&RecordId>,
) -> Result<Value, CpdError> {
    if !config.experimental.write_curation {
        return Ok(json!({"context": context.as_str(), "skipped": "off"}));
    }
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
    let (found, fts) = find_neighbours(models, &vectors, context, new, &supersedes, &all)?;
    fts_only |= fts;
    let count = found.len().min(K);
    check_pairs(models, new, &found, &all, &topics, &mut tally)?;
    write(store, context, &state, &tally)?;
    vectors.mark_checked(context, &picked, &at)?;
    checked.insert(picked);
    let remaining = due(&state, &checked, now).len();
    Ok(report(new, count, fts_only, &tally, remaining, context))
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
        args.record.as_ref(),
    )
    .map(Output::success)
}
