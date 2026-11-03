//! A Context record (contract §2.2) as it is held in memory, stored in the
//! database (§4.6), and parsed from a JSONL line of an old store (`legacy`).

use rusqlite::types::Type;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::ids::{ContextId, MachineName, PlanId, RecordId, RecordKind, SessionId};

/// The `v` of every record line and of `context.json`.
pub const VERSION: u32 = 1;

/// Who wrote a record: its Session and Plan, when it had them.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct By {
    pub session: Option<SessionId>,
    pub plan: Option<PlanId>,
}

/// What a `plan` record says about its Plan: a reference by id and machine.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRef {
    pub id: PlanId,
    pub machine: MachineName,
    #[serde(default)]
    pub jira: Option<String>,
}

/// What a new record is, beyond its text. Each kind carries exactly the
/// links it may have, so a decision cannot name a question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Draft {
    Decision {
        supersedes: Vec<RecordId>,
    },
    Note,
    /// A note that replaces an earlier note (recency wins, §2.2).
    NoteSupersedes {
        supersedes: Vec<RecordId>,
    },
    Question,
    Answer {
        question: RecordId,
    },
    Plan(PlanRef),
    /// A Plan's brief (B3-A4): `plan` is `{id, machine}`.
    Brief(PlanRef),
}

impl Draft {
    pub fn supersedes(&self) -> &[RecordId] {
        match self {
            Draft::Decision { supersedes } | Draft::NoteSupersedes { supersedes } => supersedes,
            _ => &[],
        }
    }

    pub fn kind(&self) -> RecordKind {
        match self {
            Draft::Decision { .. } => RecordKind::Decision,
            Draft::Note | Draft::NoteSupersedes { .. } => RecordKind::Note,
            Draft::Question => RecordKind::Question,
            Draft::Answer { .. } => RecordKind::Answer,
            Draft::Plan(_) => RecordKind::Plan,
            Draft::Brief(_) => RecordKind::Brief,
        }
    }
}

/// One record: the JSON of §2.2 "Context record", checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: RecordId,
    pub at: String,
    pub by: By,
    pub text: String,
    pub draft: Draft,
}

impl Record {
    pub fn kind(&self) -> RecordKind {
        self.draft.kind()
    }

    pub fn machine(&self) -> &MachineName {
        self.id.machine()
    }

    pub fn supersedes(&self) -> &[RecordId] {
        self.draft.supersedes()
    }

    pub fn question(&self) -> Option<&RecordId> {
        match &self.draft {
            Draft::Answer { question } => Some(question),
            _ => None,
        }
    }

    /// The Plan a `plan` or `brief` record references.
    pub fn plan(&self) -> Option<&PlanRef> {
        match &self.draft {
            Draft::Plan(plan) | Draft::Brief(plan) => Some(plan),
            _ => None,
        }
    }

    /// The first line of a `brief`'s text: how `context show` lists it (commands.md §3.3).
    pub fn first_line(&self) -> &str {
        self.text
            .trim_start()
            .lines()
            .next()
            .unwrap_or_default()
            .trim_end()
    }

    /// [`Record::to_json`] for a listing: a `brief` is short, its `text` its first line, with `"short": true`.
    pub fn to_json_listed(&self) -> Value {
        let mut v = self.to_json();
        if self.kind() == RecordKind::Brief {
            v["text"] = json!(self.first_line());
            v["short"] = json!(true);
        }
        v
    }

    /// The record as commands print it: `id`, `kind`, `at`, `machine`, `by`,
    /// `text`, and `supersedes`, `question` or `plan` when the kind has one.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("id".into(), json!(self.id));
        map.insert("kind".into(), json!(self.kind().as_str()));
        map.insert("at".into(), json!(self.at));
        map.insert("machine".into(), json!(self.machine()));
        map.insert(
            "by".into(),
            json!({"session": self.by.session, "plan": self.by.plan}),
        );
        map.insert("text".into(), json!(self.text));
        if !self.supersedes().is_empty() {
            map.insert("supersedes".into(), json!(self.supersedes()));
        }
        if let Some(question) = self.question() {
            map.insert("question".into(), json!(question));
        }
        if let Some(plan) = self.plan() {
            let mut p = Map::new();
            p.insert("id".into(), json!(plan.id));
            p.insert("machine".into(), json!(plan.machine));
            if let Some(jira) = &plan.jira {
                p.insert("jira".into(), json!(jira));
            }
            map.insert("plan".into(), Value::Object(p));
        }
        Value::Object(map)
    }

    /// Parses one line of a record file. The error says what is wrong with it.
    /// `machine` is the file's: a machine appends only to its own file.
    pub fn parse_line(line: &str, machine: &MachineName) -> Result<Record, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            v: u32,
            id: RecordId,
            kind: String,
            at: String,
            machine: MachineName,
            by: By,
            text: String,
            #[serde(default)]
            supersedes: Vec<RecordId>,
            #[serde(default)]
            question: Option<RecordId>,
            #[serde(default)]
            plan: Option<RawPlan>,
        }
        /// A `plan` record written before R34 carries `repo`: read and dropped (§4.6).
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPlan {
            id: PlanId,
            machine: MachineName,
            #[serde(default)]
            #[allow(dead_code)]
            // Accepted so `deny_unknown_fields` takes a pre-R34 record; never read.
            repo: Option<String>,
            #[serde(default)]
            jira: Option<String>,
        }
        let raw: Raw = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if raw.v != VERSION {
            return Err(format!("unsupported record version {}", raw.v));
        }
        let kind = kind_named(&raw.kind).ok_or_else(|| format!("unknown kind {:?}", raw.kind))?;
        if raw.id.kind() != kind {
            return Err(format!("id {} does not match kind {}", raw.id, raw.kind));
        }
        if raw.machine != *machine || raw.id.machine() != machine {
            return Err(format!(
                "record {} is not this file's machine, {machine}",
                raw.id
            ));
        }
        OffsetDateTime::parse(&raw.at, &Rfc3339).map_err(|e| format!("at is not RFC 3339: {e}"))?;
        if raw.text.is_empty() {
            return Err("text is empty".into());
        }
        let plan = raw.plan.map(|p| PlanRef {
            id: p.id,
            machine: p.machine,
            jira: p.jira,
        });
        let draft = Draft::from_parts(kind, raw.supersedes, raw.question, plan)?;
        Ok(Record {
            id: raw.id,
            at: raw.at,
            by: raw.by,
            text: raw.text,
            draft,
        })
    }

    /// The columns of `context_records` in [`Record::from_row`]'s order, after `seq`.
    pub const COLUMNS: &'static str = "seq, id, kind, at, machine, by_session, by_plan, text, supersedes, question, \
                                       plan_id, plan_machine, plan_jira";

    /// Reads a row selected with [`Record::COLUMNS`]: its `seq` and the record.
    pub fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, Record)> {
        fn bad(
            column: usize,
            e: impl std::error::Error + Send + Sync + 'static,
        ) -> rusqlite::Error {
            rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(e))
        }
        fn invalid(column: usize, message: String) -> rusqlite::Error {
            bad(
                column,
                std::io::Error::new(std::io::ErrorKind::InvalidData, message),
            )
        }
        let optional = |column: usize| row.get::<_, Option<String>>(column);
        let id = RecordId::parse(&row.get::<_, String>(1)?).map_err(|e| bad(1, e))?;
        let kind = row.get::<_, String>(2)?;
        let kind = kind_named(&kind).ok_or_else(|| invalid(2, format!("unknown kind {kind:?}")))?;
        let machine = MachineName::parse(&row.get::<_, String>(4)?).map_err(|e| bad(4, e))?;
        if id.kind() != kind || *id.machine() != machine {
            return Err(invalid(
                1,
                format!("record {id} does not match its kind and machine"),
            ));
        }
        let by = By {
            session: optional(5)?
                .map(|s| SessionId::parse(&s))
                .transpose()
                .map_err(|e| bad(5, e))?,
            plan: optional(6)?
                .map(|p| PlanId::parse(&p))
                .transpose()
                .map_err(|e| bad(6, e))?,
        };
        let record_id = |column| -> rusqlite::Result<Option<RecordId>> {
            optional(column)?
                .map(|s| RecordId::parse(&s))
                .transpose()
                .map_err(|e| bad(column, e))
        };
        let plan = match optional(10)? {
            None => None,
            Some(plan) => Some(PlanRef {
                id: PlanId::parse(&plan).map_err(|e| bad(10, e))?,
                machine: MachineName::parse(&row.get::<_, String>(11)?).map_err(|e| bad(11, e))?,
                jira: optional(12)?,
            }),
        };
        let draft = Draft::from_parts(
            kind,
            optional(8)?
                .map(|list| {
                    list.split(',')
                        .map(RecordId::parse)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
                .map_err(|e| bad(8, e))?
                .unwrap_or_default(),
            record_id(9)?,
            plan,
        )
        .map_err(|m| invalid(2, m))?;
        let record = Record {
            id,
            at: row.get(3)?,
            by,
            text: row.get(7)?,
            draft,
        };
        Ok((row.get(0)?, record))
    }

    /// Inserts the record into `context_records`; a record whose `(context, id)`
    /// is already indexed is left alone. Returns whether a row was added.
    pub fn insert(
        &self,
        conn: &rusqlite::Connection,
        context: &ContextId,
    ) -> rusqlite::Result<bool> {
        let plan = self.plan();
        let added = conn.execute(
            "INSERT INTO context_records
               (context, id, kind, at, machine, by_session, by_plan, text, supersedes, question,
                plan_id, plan_machine, plan_jira)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (context, id) DO NOTHING",
            rusqlite::params![
                context.as_str(),
                self.id.to_string(),
                self.kind().as_str(),
                self.at,
                self.machine().as_str(),
                self.by.session.as_ref().map(SessionId::as_str),
                self.by.plan.as_ref().map(PlanId::as_str),
                self.text,
                (!self.supersedes().is_empty()).then(|| {
                    self.supersedes()
                        .iter()
                        .map(RecordId::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                }),
                self.question().map(RecordId::to_string),
                plan.map(|p| p.id.as_str()),
                plan.map(|p| p.machine.as_str()),
                plan.and_then(|p| p.jira.as_deref()),
            ],
        )?;
        Ok(added == 1)
    }
}

impl Draft {
    /// The links a kind may have (§2.2): `supersedes` for a decision (naming a
    /// decision or a note) or a note (naming a note), `question` for an answer only and required
    /// (naming a question), `plan` for a plan or a brief only and required.
    fn from_parts(
        kind: RecordKind,
        supersedes: Vec<RecordId>,
        question: Option<RecordId>,
        plan: Option<PlanRef>,
    ) -> Result<Draft, String> {
        let extra =
            |name: &str, kind: RecordKind| format!("a {} record has no {name}", kind.as_str());
        if !supersedes.is_empty() && !matches!(kind, RecordKind::Decision | RecordKind::Note) {
            return Err(extra("supersedes", kind));
        }
        if question.is_some() && kind != RecordKind::Answer {
            return Err(extra("question", kind));
        }
        if plan.is_some() && !matches!(kind, RecordKind::Plan | RecordKind::Brief) {
            return Err(extra("plan", kind));
        }
        for s in &supersedes {
            if may_supersede(kind, s.kind()) {
                continue;
            }
            return Err(format!(
                "a {} record cannot supersede {s}, a {}",
                kind.as_str(),
                s.kind().as_str()
            ));
        }
        if let Some(q) = &question
            && q.kind() != RecordKind::Question
        {
            return Err(format!("question {q} is not a question"));
        }
        Ok(match kind {
            RecordKind::Decision => Draft::Decision { supersedes },
            RecordKind::Note if supersedes.is_empty() => Draft::Note,
            RecordKind::Note => Draft::NoteSupersedes { supersedes },
            RecordKind::Question => Draft::Question,
            RecordKind::Answer => Draft::Answer {
                question: question.ok_or("an answer needs its question")?,
            },
            RecordKind::Plan => Draft::Plan(plan.ok_or("a plan record needs its plan")?),
            RecordKind::Brief => Draft::Brief(plan.ok_or("a brief record needs its plan")?),
        })
    }
}

/// Recency wins (§2.2): a decision may supersede a decision or a note, a note only a note (a fact never overrules a
/// decision).
pub fn may_supersede(writer: RecordKind, target: RecordKind) -> bool {
    superseding(writer).contains(&target)
}

/// The kinds a record of kind `writer` may supersede, in the order `supersedes_invalid` lists them.
pub fn superseding(writer: RecordKind) -> &'static [RecordKind] {
    match writer {
        RecordKind::Decision => &[RecordKind::Decision, RecordKind::Note],
        RecordKind::Note => &[RecordKind::Note],
        _ => &[],
    }
}

fn kind_named(name: &str) -> Option<RecordKind> {
    RecordKind::ALL.into_iter().find(|k| k.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> MachineName {
        MachineName::parse("hs").unwrap()
    }

    fn line(extra: &str) -> String {
        format!(
            r#"{{"v":1,"id":"hs-d1","kind":"decision","at":"2026-10-14T21:12:00.000Z","machine":"hs","by":{{"session":null,"plan":null}},"text":"One engine."{extra}}}"#
        )
    }

    #[test]
    fn a_record_survives_its_line() {
        let record = Record::parse_line(&line(r#","supersedes":["hs-d9"]"#), &machine()).unwrap();
        assert_eq!(record.supersedes()[0].to_string(), "hs-d9");
        let mut value = record.to_json();
        value["v"] = json!(VERSION);
        let again = Record::parse_line(&value.to_string(), &machine()).unwrap();
        assert_eq!(again, record);
    }

    #[test]
    fn a_plan_record_from_before_r34_is_read_without_its_repo() {
        let old = line("")
            .replace("hs-d1", "hs-p1")
            .replace("decision", "plan")
            .replace(
                r#""text""#,
                r#""plan":{"id":"pay","machine":"hs","repo":"/r/pay","jira":"ABC-1"},"text""#,
            );
        let record = Record::parse_line(&old, &machine()).unwrap();
        let plan = record.plan().unwrap();
        assert_eq!(
            (plan.id.as_str(), plan.jira.as_deref()),
            ("pay", Some("ABC-1"))
        );
        assert!(
            !record.to_json()["plan"]
                .as_object()
                .unwrap()
                .contains_key("repo")
        );
        let other = old.replace(r#""repo":"/r/pay""#, r#""base":"main""#);
        assert!(
            Record::parse_line(&other, &machine()).is_err(),
            "no other unknown field"
        );
    }

    #[test]
    fn a_record_that_breaks_the_contract_is_refused() {
        let m = machine();
        for (bad, why) in [
            (line(r#","extra":1"#), "unknown field"),
            (line(r#","question":"hs-q1""#), "question on a decision"),
            (line(r#","supersedes":["hs-q1"]"#), "supersedes a question"),
            (line("").replace(r#""v":1"#, r#""v":2"#), "version"),
            (line("").replace("hs-d1", "hs-n1"), "id letter against kind"),
            (
                line("").replace(r#""machine":"hs""#, r#""machine":"zz""#),
                "machine",
            ),
            (
                line("").replace("2026-10-14T21:12:00.000Z", "yesterday"),
                "time",
            ),
            (line("").replace("One engine.", ""), "empty text"),
            (line("").replace("decision", "idea"), "kind"),
        ] {
            assert!(Record::parse_line(&bad, &m).is_err(), "{why}");
        }
        let answer = line("")
            .replace("hs-d1", "hs-a1")
            .replace("decision", "answer");
        assert!(
            Record::parse_line(&answer, &m).is_err(),
            "an answer needs its question"
        );
        let plan = line("")
            .replace("hs-d1", "hs-p1")
            .replace("decision", "plan");
        assert!(
            Record::parse_line(&plan, &m).is_err(),
            "a plan record needs its plan"
        );
    }
}
