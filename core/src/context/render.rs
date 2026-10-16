//! The rendering of a Context's records that Leads read (contract §2.2
//! "Context record", commands.md §3.3). Markdown, on demand; nothing is stored.

use std::collections::HashMap;
use std::fmt::Write;

use super::record::Record;
use crate::ids::RecordKind;

/// A record and its index position.
pub type Indexed = (i64, Record);

/// The Context as markdown: title and Jira key; Plans; active decisions;
/// superseded decisions, each naming its successor; open questions; answered
/// questions with their answers; notes. Each item shows its id and date.
///
/// Only the records after `since` are listed, but which decision is active and
/// which question is open is decided from all of `records`. An answer after
/// `since` is listed with its question, whatever the question's position.
pub fn markdown(title: &str, jira: Option<&str>, records: &[Indexed], since: i64) -> String {
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

    section(&mut out, "Plans", of_kind(RecordKind::Plan).map(|(_, r)| {
        let plan = r.plan().expect("a plan record has its plan");
        let jira = plan.jira.as_deref().map(|j| format!(", {j}")).unwrap_or_default();
        let repo = plan.repo.as_deref().map(|repo| format!(", repo {repo}")).unwrap_or_default();
        item(r, &format!("{} (on {}{repo}{jira}): {}", plan.id, plan.machine, r.text))
    }));
    section(&mut out, "Decisions", of_kind(RecordKind::Decision).filter(|(_, r)| !successor.contains_key(&r.id.to_string())).map(|(_, r)| {
        let replaces = r.supersedes().map(|s| format!(" (supersedes {s})")).unwrap_or_default();
        item(r, &format!("{}{replaces}", r.text))
    }));
    section(&mut out, "Superseded decisions", of_kind(RecordKind::Decision).filter_map(|(_, r)| {
        successor.get(&r.id.to_string()).map(|next| item(r, &format!("{} (superseded by {})", r.text, next.id)))
    }));
    section(&mut out, "Open questions", of_kind(RecordKind::Question).filter(|(_, r)| !answers.contains_key(&r.id.to_string())).map(|(_, r)| item(r, &r.text)));

    // A question is listed when it or one of its answers is after `since`.
    let mut answered: Vec<String> = Vec::new();
    for (seq, question) in records.iter().filter(|(_, r)| r.kind() == RecordKind::Question) {
        let key = question.id.to_string();
        let Some(given) = answers.get(&key) else { continue };
        if !(shown(*seq) || given.iter().any(|(s, _)| shown(*s))) {
            continue;
        }
        let mut entry = item(question, &question.text);
        for (_, answer) in given {
            entry.push_str(&nested(answer));
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
            entry.push_str(&nested(answer));
        }
        answered.push(entry);
    }
    section(&mut out, "Answered questions", answered.into_iter());
    section(&mut out, "Notes", of_kind(RecordKind::Note).map(|(_, r)| item(r, &r.text)));
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

fn nested(answer: &Record) -> String {
    format!("  - answer {} ({}): {}\n", answer.id, date(answer), answer.text.replace('\n', "\n    "))
}

fn date(record: &Record) -> &str {
    record.at.get(..10).unwrap_or(&record.at)
}

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
        let md = markdown("Building", Some("ABC-1"), &all(), 0);
        let at = |needle: &str| md.find(needle).unwrap_or_else(|| panic!("{needle} in\n{md}"));
        assert!(md.starts_with("# Building (ABC-1)\n"));
        let order = ["## Decisions", "## Superseded decisions", "## Open questions", "## Answered questions", "## Notes"];
        assert!(order.windows(2).all(|w| at(w[0]) < at(w[1])), "{md}");
        assert!(!md.contains("## Plans"));
        assert!(md.contains("- hs-d1 (2026-10-14): Old. (superseded by hs-d2)"));
        assert!(md.contains("- hs-d2 (2026-10-14): New. (supersedes hs-d1)"));
        assert!(md.contains("- hs-q2 (2026-10-14): When?"));
        assert!(md.contains("- hs-q1 (2026-10-14): Why?\n  - answer hs-a1 (2026-10-14): Because."));
        assert!(md.contains("Line one\n  line two"));
    }

    #[test]
    fn since_lists_later_records_but_judges_from_all() {
        let md = markdown("Building", None, &all(), 3);
        assert!(md.contains("hs-d2"), "{md}");
        assert!(!md.contains("- hs-d1 ("), "{md}");
        assert!(!md.contains("hs-q2"), "{md}");
        // The answer is new, so its older question comes with it.
        assert!(md.contains("- hs-q1 (2026-10-14): Why?\n  - answer hs-a1"), "{md}");
        assert!(md.contains("hs-n1"));
    }
}
