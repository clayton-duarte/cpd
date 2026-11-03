//! The rendering of a Context's records that Leads read (contract §2.2
//! "Context record", commands.md §3.3). Markdown, on demand; nothing is stored.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use super::record::{PlanRef, Record};
use crate::ids::RecordKind;

/// A record and its index position.
pub type Indexed = (i64, Record);

/// The repos of each Plan record's Workflows, by record id.
pub type Repos = HashMap<String, Vec<String>>;

/// The records an approved retirement took out of force (contract §4.6 "Retirement"), by record id: `<reason> <target>`.
pub type Retired = HashMap<String, String>;

/// The Context as markdown: title and Jira key; Plans; active decisions;
/// superseded decisions, each naming its successor; open questions; answered
/// questions with their answers; notes (those in force, then the superseded ones, each naming its successor). Each item shows its id and date.
///
/// A Plan record lists `repos[<record id>]`: the repos of its Workflows, from the
/// database (commands.md §3.3; the record itself has none).
///
/// Only the records after `since` are listed, but which decision is active and
/// which question is open is decided from all of `records`. An answer after
/// `since` is listed with its question, whatever the question's position.
#[cfg(test)]
pub fn markdown(
    title: &str,
    jira: Option<&str>,
    records: &[Indexed],
    since: i64,
    repos: &Repos,
) -> String {
    markdown_retired(title, jira, records, since, repos, &Retired::new())
}

/// [`markdown`] with the retired records labelled `retired (<reason> <target>)`: a retired decision is listed with the
/// superseded ones, a retired note after the superseded ones, a retired question or answer where it is.
pub fn markdown_retired(
    title: &str,
    jira: Option<&str>,
    records: &[Indexed],
    since: i64,
    repos: &Repos,
    retired: &Retired,
) -> String {
    let retired_label = |r: &Record| {
        retired
            .get(&r.id.to_string())
            .map(|why| format!(" [retired ({why})]"))
            .unwrap_or_default()
    };
    let is_retired = |r: &Record| retired.contains_key(&r.id.to_string());
    let shown = |seq: i64| seq > since;
    let (successor, answers) = links(records);

    let mut out = header(title, jira);

    let of_kind = |kind: RecordKind| {
        records
            .iter()
            .filter(move |(seq, r)| shown(*seq) && r.kind() == kind)
    };

    let plans = plan_items(records, repos, &shown, &HashSet::new())
        .into_iter()
        .map(|(entry, _)| entry);
    section(&mut out, "Plans", plans);
    section(
        &mut out,
        "Decisions",
        of_kind(RecordKind::Decision)
            .filter(|(_, r)| !successor.contains_key(&r.id.to_string()) && !is_retired(r))
            .map(|(_, r)| {
                let replaces = r
                    .supersedes()
                    .map(|s| format!(" (supersedes {s})"))
                    .unwrap_or_default();
                item(r, &format!("{}{replaces}", r.text))
            }),
    );
    section(
        &mut out,
        "Superseded decisions",
        of_kind(RecordKind::Decision).filter_map(|(_, r)| match successor.get(&r.id.to_string()) {
            Some(next) => Some(item(
                r,
                &format!("{} (superseded by {}){}", r.text, next.id, retired_label(r)),
            )),
            None => is_retired(r).then(|| item(r, &format!("{}{}", r.text, retired_label(r)))),
        }),
    );
    section(
        &mut out,
        "Open questions",
        of_kind(RecordKind::Question)
            .filter(|(_, r)| !answers.contains_key(&r.id.to_string()))
            .map(|(_, r)| item(r, &format!("{}{}", r.text, retired_label(r)))),
    );

    // A question is listed when it or one of its answers is after `since`.
    let mut answered: Vec<String> = Vec::new();
    for (seq, question) in records
        .iter()
        .filter(|(_, r)| r.kind() == RecordKind::Question)
    {
        let key = question.id.to_string();
        let Some(given) = answers.get(&key) else {
            continue;
        };
        if !(shown(*seq) || given.iter().any(|(s, _)| shown(*s))) {
            continue;
        }
        let mut entry = item(
            question,
            &format!("{}{}", question.text, retired_label(question)),
        );
        for (_, answer) in given {
            entry.push_str(&nested(answer, &retired_label(answer)));
        }
        answered.push(entry);
    }
    // An answer whose question is not in the index (it should not happen: a
    // write checks it) is still shown.
    for (key, given) in &answers {
        if records.iter().any(|(_, r)| r.id.to_string() == *key)
            || !given.iter().any(|(s, _)| shown(*s))
        {
            continue;
        }
        let mut entry = format!("- {key} (question not in this index)\n");
        for (_, answer) in given {
            entry.push_str(&nested(answer, &retired_label(answer)));
        }
        answered.push(entry);
    }
    section(&mut out, "Answered questions", answered.into_iter());
    // Notes in force first, then the superseded ones, each naming its successor (a note or a decision).
    let active = of_kind(RecordKind::Note)
        .filter(|(_, r)| !successor.contains_key(&r.id.to_string()) && !is_retired(r))
        .map(|(_, r)| {
            let replaces = r
                .supersedes()
                .map(|s| format!(" (supersedes {s})"))
                .unwrap_or_default();
            item(r, &format!("{}{replaces}", r.text))
        });
    let superseded = of_kind(RecordKind::Note).filter_map(|(_, r)| {
        successor.get(&r.id.to_string()).map(|next| {
            item(
                r,
                &format!("{} (superseded by {}){}", r.text, next.id, retired_label(r)),
            )
        })
    });
    let retired_notes = of_kind(RecordKind::Note)
        .filter(|(_, r)| is_retired(r) && !successor.contains_key(&r.id.to_string()))
        .map(|(_, r)| item(r, &format!("{}{}", r.text, retired_label(r))));
    section(
        &mut out,
        "Notes",
        active.chain(superseded).chain(retired_notes),
    );
    out
}

type Successors<'a> = HashMap<String, &'a Record>;
type Answers<'a> = HashMap<String, Vec<&'a Indexed>>;

/// The successor of each superseded record and the answers of each question, by record id.
fn links(records: &[Indexed]) -> (Successors<'_>, Answers<'_>) {
    let mut successor: Successors = HashMap::new();
    let mut answers: Answers = HashMap::new();
    for entry in records {
        let (_, record) = entry;
        if let Some(old) = record.supersedes() {
            successor.insert(old.to_string(), record);
        }
        if let Some(question) = record.question() {
            answers.entry(question.to_string()).or_default().push(entry);
        }
    }
    (successor, answers)
}

fn header(title: &str, jira: Option<&str>) -> String {
    let mut out = format!("# {title}");
    if let Some(jira) = jira {
        let _ = write!(out, " ({jira})");
    }
    out.push('\n');
    out
}

/// The key of a Plan in the set of done Plans handed to [`bounded`]: its id and machine.
pub fn plan_key(plan: &PlanRef) -> String {
    format!("{}@{}", plan.id, plan.machine)
}

/// The Plans' entries (each with the index positions of the records it shows): a Plan is listed when it or one of
/// its briefs is `shown`; a Plan of `hidden` (by [`plan_key`]) and its briefs are not.
fn plan_items(
    records: &[Indexed],
    repos: &Repos,
    shown: &dyn Fn(i64) -> bool,
    hidden: &HashSet<String>,
) -> Vec<(String, Vec<i64>)> {
    // A brief is listed short under its Plan; a Plan is listed when it or one of its briefs is shown.
    let briefs: Vec<&Indexed> = records
        .iter()
        .filter(|(_, r)| r.kind() == RecordKind::Brief)
        .collect();
    let of_plan = |plan: &PlanRef| -> Vec<&Indexed> {
        briefs
            .iter()
            .copied()
            .filter(|(_, b)| {
                b.plan()
                    .is_some_and(|p| p.id == plan.id && p.machine == plan.machine)
            })
            .collect()
    };
    let mut plans: Vec<(String, Vec<i64>)> = Vec::new();
    for (seq, r) in records.iter().filter(|(_, r)| r.kind() == RecordKind::Plan) {
        let plan = r.plan().expect("a plan record has its plan");
        if hidden.contains(&plan_key(plan))
            || !(shown(*seq) || of_plan(plan).iter().any(|(s, _)| shown(*s)))
        {
            continue;
        }
        let jira = plan
            .jira
            .as_deref()
            .map(|j| format!(", {j}"))
            .unwrap_or_default();
        let repos = match repos.get(&r.id.to_string()) {
            Some(repos) if !repos.is_empty() => format!(", repos {}", repos.join(", ")),
            _ => String::new(),
        };
        let mut entry = item(
            r,
            &format!("{} (on {}{repos}{jira}): {}", plan.id, plan.machine, r.text),
        );
        let mut seqs = vec![*seq];
        for (s, brief) in of_plan(plan) {
            entry.push_str(&nested_brief(brief));
            seqs.push(*s);
        }
        plans.push((entry, seqs));
    }
    // A brief whose Plan record is not in the index (it should not happen: `plan new` writes both) is still shown.
    for (seq, brief) in &briefs {
        let plan = brief.plan().expect("a brief has its plan");
        let listed = records.iter().any(|(_, r)| {
            r.kind() == RecordKind::Plan
                && r.plan()
                    .is_some_and(|p| p.id == plan.id && p.machine == plan.machine)
        });
        if shown(*seq) && !listed && !hidden.contains(&plan_key(plan)) {
            plans.push((
                item(
                    brief,
                    &format!(
                        "brief of {} (on {}): {}",
                        plan.id,
                        plan.machine,
                        brief.first_line()
                    ),
                ),
                vec![*seq],
            ));
        }
    }
    plans
}

/// How many notes bare `context show` lists.
const NOTE_CAP: usize = 40;

/// What bare `context show` leaves out, by kind (`--all` lists it).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Omitted {
    pub superseded_decisions: usize,
    pub answered_questions: usize,
    pub superseded_notes: usize,
    pub older_notes: usize,
    pub done_plans: usize,
}

/// The bounded Context render: its markdown, the index positions of the records it lists and what it omits.
pub struct Bounded {
    pub markdown: String,
    pub shown: HashSet<i64>,
    pub omitted: Omitted,
}

/// The Context bounded to what is active (commands.md §3.3): open questions, decisions in force, the curated
/// `topics` with their counts, the newest [`NOTE_CAP`] notes in force, the Plans not in `done` (by [`plan_key`]),
/// then a count line per kind of history left out.
pub fn bounded(
    title: &str,
    jira: Option<&str>,
    records: &[Indexed],
    repos: &Repos,
    retired: &Retired,
    done: &HashSet<String>,
    topics: &[(String, usize)],
) -> Bounded {
    let (successor, answers) = links(records);
    let in_force = |r: &Record| {
        !successor.contains_key(&r.id.to_string()) && !retired.contains_key(&r.id.to_string())
    };
    let retired_label = |r: &Record| {
        retired
            .get(&r.id.to_string())
            .map(|why| format!(" [retired ({why})]"))
            .unwrap_or_default()
    };
    let replaces = |r: &Record| {
        r.supersedes()
            .map(|s| format!(" (supersedes {s})"))
            .unwrap_or_default()
    };
    let of_kind = |kind: RecordKind| records.iter().filter(move |(_, r)| r.kind() == kind);
    let mut shown = HashSet::new();
    let mut omitted = Omitted::default();
    let mut out = header(title, jira);

    section(
        &mut out,
        "Open questions",
        of_kind(RecordKind::Question)
            .filter(|(_, r)| !answers.contains_key(&r.id.to_string()))
            .map(|(seq, r)| {
                shown.insert(*seq);
                item(r, &format!("{}{}", r.text, retired_label(r)))
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    section(
        &mut out,
        "Decisions",
        of_kind(RecordKind::Decision)
            .filter(|(_, r)| in_force(r))
            .map(|(seq, r)| {
                shown.insert(*seq);
                item(r, &format!("{}{}", r.text, replaces(r)))
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    omitted.superseded_decisions = of_kind(RecordKind::Decision)
        .filter(|(_, r)| !in_force(r))
        .count();
    if !topics.is_empty() {
        let line: Vec<String> = topics.iter().map(|(t, n)| format!("{t} ({n})")).collect();
        let _ = write!(out, "\n## Topics\n{}\n", line.join(", "));
    }
    let notes: Vec<&Indexed> = of_kind(RecordKind::Note)
        .filter(|(_, r)| in_force(r))
        .collect();
    omitted.superseded_notes = of_kind(RecordKind::Note).count() - notes.len();
    omitted.older_notes = notes.len().saturating_sub(NOTE_CAP);
    section(
        &mut out,
        "Notes",
        notes[omitted.older_notes..]
            .iter()
            .map(|(seq, r)| {
                shown.insert(*seq);
                item(r, &format!("{}{}", r.text, replaces(r)))
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    if omitted.older_notes > 0 {
        let _ = write!(
            out,
            "... {} older notes omitted: cpd context show --topic <t> | cpd context search <query> | cpd context show --all\n",
            omitted.older_notes
        );
    }
    let plans = plan_items(records, repos, &|_| true, done);
    section(
        &mut out,
        "Plans",
        plans
            .into_iter()
            .map(|(entry, seqs)| {
                shown.extend(seqs);
                entry
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    omitted.answered_questions = answers.len();
    omitted.done_plans = of_kind(RecordKind::Plan)
        .filter(|(_, r)| r.plan().is_some_and(|p| done.contains(&plan_key(p))))
        .count();
    let counts = [
        (omitted.superseded_decisions, "superseded decisions"),
        (omitted.answered_questions, "answered questions"),
        (omitted.superseded_notes, "superseded notes"),
        (omitted.done_plans, "done plans"),
    ];
    if counts.iter().any(|(n, _)| *n > 0) {
        out.push_str("\n## History (omitted; cpd context show --all renders it)\n\n");
        for (n, what) in counts.iter().filter(|(n, _)| *n > 0) {
            let _ = writeln!(out, "- {n} {what}");
        }
    }
    Bounded {
        markdown: out,
        shown,
        omitted,
    }
}

/// One record in full (`context show --record`, commands.md §3.3): the Context's title, then the record's id, kind
/// (a `plan` or `brief` with its Plan) and date, then its whole text.
pub fn single(title: &str, jira: Option<&str>, record: &Record) -> String {
    let mut out = format!("# {title}");
    if let Some(jira) = jira {
        let _ = write!(out, " ({jira})");
    }
    let of = record
        .plan()
        .map(|p| format!(" of {}", p.id))
        .unwrap_or_default();
    let _ = write!(
        out,
        "\n\n## {} ({}{of}, {})\n\n{}\n",
        record.id,
        record.kind().as_str(),
        date(record),
        record.text.trim_end()
    );
    out
}

fn section(out: &mut String, heading: &str, items: impl Iterator<Item = String>) {
    let items: Vec<String> = items.collect();
    if items.is_empty() {
        return;
    }
    let _ = write!(out, "\n## {heading}\n\n");
    for item in items {
        out.push_str(&item);
    }
}

/// `- <id> (<date>): <text>`, a multi-line text indented under it.
fn item(record: &Record, text: &str) -> String {
    format!(
        "- {} ({}): {}\n",
        record.id,
        date(record),
        text.replace('\n', "\n  ")
    )
}

fn nested(answer: &Record, label: &str) -> String {
    format!(
        "  - answer {} ({}): {}{label}\n",
        answer.id,
        date(answer),
        answer.text.replace('\n', "\n    ")
    )
}

/// A brief under its Plan: its id and first line only (commands.md §3.3).
fn nested_brief(brief: &Record) -> String {
    format!(
        "  - brief {} ({}): {}\n",
        brief.id,
        date(brief),
        brief.first_line()
    )
}

fn date(record: &Record) -> String {
    format!("{} UTC", record.at.get(..10).unwrap_or(&record.at))
}

#[cfg(test)]
#[path = "render_time_tests.rs"]
mod render_time_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::record::{By, Draft};
    use crate::ids::RecordId;

    fn rec(id: &str, text: &str, draft: Draft) -> Record {
        Record {
            id: RecordId::parse(id).unwrap(),
            at: "2026-10-14T21:12:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: text.into(),
            draft,
        }
    }

    fn all() -> Vec<Indexed> {
        let d = |s: &str| Some(RecordId::parse(s).unwrap());
        vec![
            (
                1,
                rec("hs-d1", "Old.", Draft::Decision { supersedes: None }),
            ),
            (2, rec("hs-q1", "Why?", Draft::Question)),
            (3, rec("hs-q2", "When?", Draft::Question)),
            (
                4,
                rec(
                    "hs-d2",
                    "New.",
                    Draft::Decision {
                        supersedes: d("hs-d1"),
                    },
                ),
            ),
            (
                5,
                rec(
                    "hs-a1",
                    "Because.",
                    Draft::Answer {
                        question: RecordId::parse("hs-q1").unwrap(),
                    },
                ),
            ),
            (6, rec("hs-n1", "Line one\nline two", Draft::Note)),
        ]
    }

    #[test]
    fn sections_in_order() {
        let md = markdown("Building", Some("ABC-1"), &all(), 0, &Repos::new());
        let at = |needle: &str| {
            md.find(needle)
                .unwrap_or_else(|| panic!("{needle} in\n{md}"))
        };
        assert!(md.starts_with("# Building (ABC-1)\n"));
        let order = [
            "## Decisions",
            "## Superseded decisions",
            "## Open questions",
            "## Answered questions",
            "## Notes",
        ];
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{md}");
        assert!(!md.contains("## Plans"));
        assert!(md.contains("- hs-d1 (2026-10-14 UTC): Old. (superseded by hs-d2)"));
        assert!(md.contains("- hs-d2 (2026-10-14 UTC): New. (supersedes hs-d1)"));
        assert!(md.contains("- hs-q2 (2026-10-14 UTC): When?"));
        assert!(md.contains(
            "- hs-q1 (2026-10-14 UTC): Why?\n  - answer hs-a1 (2026-10-14 UTC): Because."
        ));
        assert!(md.contains("Line one\n  line two"));
    }

    #[test]
    fn a_superseded_note_follows_the_notes_in_force_naming_its_successor() {
        let records = vec![
            (1, rec("hs-n1", "Old fact.", Draft::Note)),
            (
                2,
                rec(
                    "hs-n2",
                    "New fact.",
                    Draft::NoteSupersedes {
                        supersedes: RecordId::parse("hs-n1").unwrap(),
                    },
                ),
            ),
            (3, rec("hs-n3", "Other fact.", Draft::Note)),
        ];
        let md = markdown("Building", None, &records, 0, &Repos::new());
        let notes = md.split("## Notes").nth(1).unwrap();
        let at = |needle: &str| {
            notes
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} in\n{notes}"))
        };
        assert!(at("- hs-n2 (2026-10-14 UTC): New fact. (supersedes hs-n1)") < at("- hs-n3"));
        assert!(at("- hs-n3") < at("- hs-n1 (2026-10-14 UTC): Old fact. (superseded by hs-n2)"));
    }

    #[test]
    fn a_brief_is_listed_short_under_its_plan() {
        use crate::context::record::PlanRef;
        use crate::ids::{MachineName, PlanId};
        let plan = |id: &str| PlanRef {
            id: PlanId::parse(id).unwrap(),
            machine: MachineName::parse("hs").unwrap(),
            jira: None,
        };
        let records = vec![
            (1, rec("hs-p1", "The pay plan", Draft::Plan(plan("pay")))),
            (
                2,
                rec(
                    "hs-b1",
                    "\nFirst line.\nSecond line.",
                    Draft::Brief(plan("pay")),
                ),
            ),
            (
                3,
                rec("hs-b2", "Orphan.\nMore.", Draft::Brief(plan("gone"))),
            ),
        ];
        let md = markdown("Building", None, &records, 0, &Repos::new());
        assert!(md.contains("- hs-p1 (2026-10-14 UTC): pay (on hs): The pay plan\n  - brief hs-b1 (2026-10-14 UTC): First line.\n"), "{md}");
        assert!(
            md.contains("- hs-b2 (2026-10-14 UTC): brief of gone (on hs): Orphan.\n"),
            "{md}"
        );
        assert!(!md.contains("Second line") && !md.contains("More."), "{md}");
        // A new brief brings its Plan; an older Plan alone is not listed again.
        let md = markdown("Building", None, &records, 1, &Repos::new());
        assert!(md.contains("hs-p1") && md.contains("brief hs-b1"), "{md}");
        let md = markdown("Building", None, &records, 2, &Repos::new());
        assert!(
            !md.contains("hs-p1") && !md.contains("hs-b1") && md.contains("hs-b2"),
            "{md}"
        );
        let one = single("Building", None, &records[1].1);
        assert!(
            one.contains(
                "## hs-b1 (brief of pay, 2026-10-14 UTC)\n\n\nFirst line.\nSecond line.\n"
            ),
            "{one}"
        );
    }

    #[test]
    fn since_lists_later_records_but_judges_from_all() {
        let md = markdown("Building", None, &all(), 3, &Repos::new());
        assert!(md.contains("hs-d2"), "{md}");
        assert!(!md.contains("- hs-d1 ("), "{md}");
        assert!(!md.contains("hs-q2"), "{md}");
        // The answer is new, so its older question comes with it.
        assert!(
            md.contains("- hs-q1 (2026-10-14 UTC): Why?\n  - answer hs-a1"),
            "{md}"
        );
        assert!(md.contains("hs-n1"));
    }
}
