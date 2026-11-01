//! The rendering of a Context's records that Leads read (contract §2.2
//! "Context record", commands.md §3.3). Markdown, on demand; nothing is stored.

use std::collections::HashMap;
use std::fmt::Write;

use super::record::{PlanRef, Record};
use crate::ids::RecordKind;

/// A record and its index position.
pub type Indexed = (i64, Record);

/// The repos of each Plan record's PRs, by record id.
pub type Repos = HashMap<String, Vec<String>>;

/// The records an approved retirement took out of force (contract §4.6 "Retirement"), by record id: `<reason> <target>`.
pub type Retired = HashMap<String, String>;

/// The Context as markdown: title and Jira key; Plans; active decisions;
/// superseded decisions, each naming its successor; open questions; answered
/// questions with their answers; notes (those in force, then the superseded ones, each naming its successor). Each item shows its id and date.
///
/// A Plan record lists `repos[<record id>]`: the repos of its PRs, from the
/// database (commands.md §3.3; the record itself has none).
///
/// Only the records after `since` are listed, but which decision is active and
/// which question is open is decided from all of `records`. An answer after
/// `since` is listed with its question, whatever the question's position.
#[cfg(test)]
pub fn markdown(title: &str, jira: Option<&str>, records: &[Indexed], since: i64, repos: &Repos) -> String {
    markdown_retired(title, jira, records, since, repos, &Retired::new())
}

/// [`markdown`] with the retired records labelled `retired (<reason> <target>)`: a retired decision is listed with the
/// superseded ones, a retired note after the superseded ones, a retired question or answer where it is.
pub fn markdown_retired(title: &str, jira: Option<&str>, records: &[Indexed], since: i64, repos: &Repos, retired: &Retired) -> String {
    let retired_label = |r: &Record| retired.get(&r.id.to_string()).map(|why| format!(" [retired ({why})]")).unwrap_or_default();
    let is_retired = |r: &Record| retired.contains_key(&r.id.to_string());
    let shown = |seq: i64| seq > since;
    let mut successor: HashMap<String, &Record> = HashMap::new();
    let mut answers: HashMap<String, Vec<&Indexed>> = HashMap::new();
    for entry in records {
        let (_, record) = entry;
        if let Some(old) = record.supersedes() {
            successor.insert(old.to_string(), record);
        }
        if let Some(question) = record.question() {
            answers.entry(question.to_string()).or_default().push(entry);
        }
    }

    let mut out = format!("# {title}");
    if let Some(jira) = jira {
        let _ = write!(out, " ({jira})");
    }
    out.push('\n');

    let of_kind = |kind: RecordKind| records.iter().filter(move |(seq, r)| shown(*seq) && r.kind() == kind);

    // A brief is listed short under its Plan; a Plan is listed when it or one of its briefs is shown.
    let briefs: Vec<&Indexed> = records.iter().filter(|(_, r)| r.kind() == RecordKind::Brief).collect();
    let of_plan = |plan: &PlanRef| -> Vec<&Indexed> {
        briefs.iter().copied().filter(|(_, b)| b.plan().is_some_and(|p| p.id == plan.id && p.machine == plan.machine)).collect()
    };
    let mut plans: Vec<String> = Vec::new();
    for (seq, r) in records.iter().filter(|(_, r)| r.kind() == RecordKind::Plan) {
        let plan = r.plan().expect("a plan record has its plan");
        if !(shown(*seq) || of_plan(plan).iter().any(|(s, _)| shown(*s))) {
            continue;
        }
        let jira = plan.jira.as_deref().map(|j| format!(", {j}")).unwrap_or_default();
        let repos = match repos.get(&r.id.to_string()) {
            Some(repos) if !repos.is_empty() => format!(", repos {}", repos.join(", ")),
            _ => String::new(),
        };
        let mut entry = item(r, &format!("{} (on {}{repos}{jira}): {}", plan.id, plan.machine, r.text));
        for (_, brief) in of_plan(plan) {
            entry.push_str(&nested_brief(brief));
        }
        plans.push(entry);
    }
    // A brief whose Plan record is not in the index (it should not happen: `plan new` writes both) is still shown.
    for (seq, brief) in &briefs {
        let plan = brief.plan().expect("a brief has its plan");
        let listed = records.iter().any(|(_, r)| r.kind() == RecordKind::Plan && r.plan().is_some_and(|p| p.id == plan.id && p.machine == plan.machine));
        if shown(*seq) && !listed {
            plans.push(item(brief, &format!("brief of {} (on {}): {}", plan.id, plan.machine, brief.first_line())));
        }
    }
    section(&mut out, "Plans", plans.into_iter());
    section(&mut out, "Decisions", of_kind(RecordKind::Decision).filter(|(_, r)| !successor.contains_key(&r.id.to_string()) && !is_retired(r)).map(|(_, r)| {
        let replaces = r.supersedes().map(|s| format!(" (supersedes {s})")).unwrap_or_default();
        item(r, &format!("{}{replaces}", r.text))
    }));
    section(&mut out, "Superseded decisions", of_kind(RecordKind::Decision).filter_map(|(_, r)| {
        match successor.get(&r.id.to_string()) {
            Some(next) => Some(item(r, &format!("{} (superseded by {}){}", r.text, next.id, retired_label(r)))),
            None => is_retired(r).then(|| item(r, &format!("{}{}", r.text, retired_label(r)))),
        }
    }));
    section(&mut out, "Open questions", of_kind(RecordKind::Question).filter(|(_, r)| !answers.contains_key(&r.id.to_string())).map(|(_, r)| item(r, &format!("{}{}", r.text, retired_label(r)))));

    // A question is listed when it or one of its answers is after `since`.
    let mut answered: Vec<String> = Vec::new();
    for (seq, question) in records.iter().filter(|(_, r)| r.kind() == RecordKind::Question) {
        let key = question.id.to_string();
        let Some(given) = answers.get(&key) else { continue };
        if !(shown(*seq) || given.iter().any(|(s, _)| shown(*s))) {
            continue;
        }
        let mut entry = item(question, &format!("{}{}", question.text, retired_label(question)));
        for (_, answer) in given {
            entry.push_str(&nested(answer, &retired_label(answer)));
        }
        answered.push(entry);
    }
    // An answer whose question is not in the index (it should not happen: a
    // write checks it) is still shown.
    for (key, given) in &answers {
        if records.iter().any(|(_, r)| r.id.to_string() == *key) || !given.iter().any(|(s, _)| shown(*s)) {
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
    let active = of_kind(RecordKind::Note).filter(|(_, r)| !successor.contains_key(&r.id.to_string()) && !is_retired(r)).map(|(_, r)| {
        let replaces = r.supersedes().map(|s| format!(" (supersedes {s})")).unwrap_or_default();
        item(r, &format!("{}{replaces}", r.text))
    });
    let superseded = of_kind(RecordKind::Note).filter_map(|(_, r)| {
        successor.get(&r.id.to_string()).map(|next| item(r, &format!("{} (superseded by {}){}", r.text, next.id, retired_label(r))))
    });
    let retired_notes = of_kind(RecordKind::Note)
        .filter(|(_, r)| is_retired(r) && !successor.contains_key(&r.id.to_string()))
        .map(|(_, r)| item(r, &format!("{}{}", r.text, retired_label(r))));
    section(&mut out, "Notes", active.chain(superseded).chain(retired_notes));
    out
}

/// One record in full (`context show --record`, commands.md §3.3): the Context's title, then the record's id, kind
/// (a `plan` or `brief` with its Plan) and date, then its whole text.
pub fn single(title: &str, jira: Option<&str>, record: &Record) -> String {
    let mut out = format!("# {title}");
    if let Some(jira) = jira {
        let _ = write!(out, " ({jira})");
    }
    let of = record.plan().map(|p| format!(" of {}", p.id)).unwrap_or_default();
    let _ = write!(out, "\n\n## {} ({}{of}, {})\n\n{}\n", record.id, record.kind().as_str(), date(record), record.text.trim_end());
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
    format!("- {} ({}): {}\n", record.id, date(record), text.replace('\n', "\n  "))
}

fn nested(answer: &Record, label: &str) -> String {
    format!("  - answer {} ({}): {}{label}\n", answer.id, date(answer), answer.text.replace('\n', "\n    "))
}

/// A brief under its Plan: its id and first line only (commands.md §3.3).
fn nested_brief(brief: &Record) -> String {
    format!("  - brief {} ({}): {}\n", brief.id, date(brief), brief.first_line())
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
            by: By { session: None, plan: None },
            text: text.into(),
            draft,
        }
    }

    fn all() -> Vec<Indexed> {
        let d = |s: &str| Some(RecordId::parse(s).unwrap());
        vec![
            (1, rec("hs-d1", "Old.", Draft::Decision { supersedes: None })),
            (2, rec("hs-q1", "Why?", Draft::Question)),
            (3, rec("hs-q2", "When?", Draft::Question)),
            (4, rec("hs-d2", "New.", Draft::Decision { supersedes: d("hs-d1") })),
            (5, rec("hs-a1", "Because.", Draft::Answer { question: RecordId::parse("hs-q1").unwrap() })),
            (6, rec("hs-n1", "Line one\nline two", Draft::Note)),
        ]
    }

    #[test]
    fn sections_in_order() {
        let md = markdown("Building", Some("ABC-1"), &all(), 0, &Repos::new());
        let at = |needle: &str| md.find(needle).unwrap_or_else(|| panic!("{needle} in\n{md}"));
        assert!(md.starts_with("# Building (ABC-1)\n"));
        let order = ["## Decisions", "## Superseded decisions", "## Open questions", "## Answered questions", "## Notes"];
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{md}");
        assert!(!md.contains("## Plans"));
        assert!(md.contains("- hs-d1 (2026-10-14 UTC): Old. (superseded by hs-d2)"));
        assert!(md.contains("- hs-d2 (2026-10-14 UTC): New. (supersedes hs-d1)"));
        assert!(md.contains("- hs-q2 (2026-10-14 UTC): When?"));
        assert!(md.contains("- hs-q1 (2026-10-14 UTC): Why?\n  - answer hs-a1 (2026-10-14 UTC): Because."));
        assert!(md.contains("Line one\n  line two"));
    }

    #[test]
    fn a_superseded_note_follows_the_notes_in_force_naming_its_successor() {
        let records = vec![
            (1, rec("hs-n1", "Old fact.", Draft::Note)),
            (2, rec("hs-n2", "New fact.", Draft::NoteSupersedes { supersedes: RecordId::parse("hs-n1").unwrap() })),
            (3, rec("hs-n3", "Other fact.", Draft::Note)),
        ];
        let md = markdown("Building", None, &records, 0, &Repos::new());
        let notes = md.split("## Notes").nth(1).unwrap();
        let at = |needle: &str| notes.find(needle).unwrap_or_else(|| panic!("{needle} in\n{notes}"));
        assert!(at("- hs-n2 (2026-10-14 UTC): New fact. (supersedes hs-n1)") < at("- hs-n3"));
        assert!(at("- hs-n3") < at("- hs-n1 (2026-10-14 UTC): Old fact. (superseded by hs-n2)"));
    }

    #[test]
    fn a_brief_is_listed_short_under_its_plan() {
        use crate::context::record::PlanRef;
        use crate::ids::{MachineName, PlanId};
        let plan = |id: &str| PlanRef { id: PlanId::parse(id).unwrap(), machine: MachineName::parse("hs").unwrap(), jira: None };
        let records = vec![
            (1, rec("hs-p1", "The pay plan", Draft::Plan(plan("pay")))),
            (2, rec("hs-b1", "\nFirst line.\nSecond line.", Draft::Brief(plan("pay")))),
            (3, rec("hs-b2", "Orphan.\nMore.", Draft::Brief(plan("gone")))),
        ];
        let md = markdown("Building", None, &records, 0, &Repos::new());
        assert!(md.contains("- hs-p1 (2026-10-14 UTC): pay (on hs): The pay plan\n  - brief hs-b1 (2026-10-14 UTC): First line.\n"), "{md}");
        assert!(md.contains("- hs-b2 (2026-10-14 UTC): brief of gone (on hs): Orphan.\n"), "{md}");
        assert!(!md.contains("Second line") && !md.contains("More."), "{md}");
        // A new brief brings its Plan; an older Plan alone is not listed again.
        let md = markdown("Building", None, &records, 1, &Repos::new());
        assert!(md.contains("hs-p1") && md.contains("brief hs-b1"), "{md}");
        let md = markdown("Building", None, &records, 2, &Repos::new());
        assert!(!md.contains("hs-p1") && !md.contains("hs-b1") && md.contains("hs-b2"), "{md}");
        let one = single("Building", None, &records[1].1);
        assert!(one.contains("## hs-b1 (brief of pay, 2026-10-14 UTC)\n\n\nFirst line.\nSecond line.\n"), "{one}");
    }

    #[test]
    fn since_lists_later_records_but_judges_from_all() {
        let md = markdown("Building", None, &all(), 3, &Repos::new());
        assert!(md.contains("hs-d2"), "{md}");
        assert!(!md.contains("- hs-d1 ("), "{md}");
        assert!(!md.contains("hs-q2"), "{md}");
        // The answer is new, so its older question comes with it.
        assert!(md.contains("- hs-q1 (2026-10-14 UTC): Why?\n  - answer hs-a1"), "{md}");
        assert!(md.contains("hs-n1"));
    }
}
