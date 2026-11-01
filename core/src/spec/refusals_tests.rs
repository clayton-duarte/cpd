//! The Worker Spec lists only refusals `attempt submit` returns (commands.md §6.1 precondition table).

use std::collections::BTreeSet;
use std::path::Path;

use super::{Input, JobKind, render};
use crate::config::Tier;
use crate::ids::{AttemptId, PlanId};

fn text(kind: JobKind) -> String {
    let (plan, attempt) = (
        PlanId::parse("pay").unwrap(),
        AttemptId::parse("api-schema-a1").unwrap(),
    );
    let one = ["a".to_owned()];
    render(&Input {
        plan: &plan,
        attempt: &attempt,
        title: "API schema",
        tier: Tier::parse("S3").unwrap(),
        worktree: Path::new("/w/pay--api-schema-a1"),
        kind,
        goal: "Define the schema.",
        context: "None.",
        touches: &one,
        acceptance: &one,
        out_of_scope: &[],
        resources: &[],
        quick_check: &[],
        protected: &[],
        generated: &[],
        approved_touches: &[],
        previous: &[],
        answers: &[],
        addenda: &[],
    })
    .unwrap()
}

/// Backticked codes in `s`, in order.
fn ticked(s: &str) -> Vec<String> {
    s.split('`').skip(1).step_by(2).map(str::to_owned).collect()
}

/// The codes that start a bullet of the Finish refusal list: `- `a`:` or `- `a`, `b`:`.
fn listed(spec: &str) -> BTreeSet<String> {
    let finish = spec.split("\n## Finish").nth(1).expect("a Finish section");
    let mut codes = BTreeSet::new();
    for line in finish.lines() {
        let Some(rest) = line.strip_prefix("- `") else {
            continue;
        };
        let Some(head) = rest.find("`:").map(|end| &line[2..3 + end + 1]) else {
            continue;
        };
        if head
            .split(", ")
            .all(|c| c.len() > 2 && c.starts_with('`') && c.ends_with('`'))
        {
            codes.extend(ticked(head));
        }
    }
    codes
}

/// The codes of the Code column of the §6.1 precondition table.
fn table_codes() -> BTreeSet<String> {
    let md =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/commands.md"))
            .unwrap();
    let mut section = Vec::new();
    let mut inside = false;
    for line in md.lines() {
        if line.starts_with("### 6.1 ") {
            inside = true;
        } else if inside && line.starts_with("### ") {
            break;
        } else if inside && line.starts_with('|') {
            section.push(line);
        }
    }
    assert!(!section.is_empty(), "§6.1 has a table");
    let mut codes = BTreeSet::new();
    for row in section {
        let cells: Vec<&str> = row.trim_end().trim_end_matches('|').split('|').collect();
        codes.extend(ticked(cells[cells.len() - 2]));
    }
    codes
}

#[test]
fn worker_text_lists_only_refusals_submit_returns() {
    let (tests, files, none) = (["t".to_owned()], ["f".to_owned()], Path::new("/w/out.md"));
    let kinds = [
        JobKind::Work,
        JobKind::Docs,
        JobKind::Test {
            tests: &tests,
            test_files: &files,
        },
        JobKind::Impl {
            test_jobs: &files,
            tests: &tests,
            guarded: &files,
            approved: &[],
        },
        JobKind::Verify,
        JobKind::Open,
        JobKind::Update,
        JobKind::Explore {
            kind: "scout",
            artefact: none,
        },
    ];
    let table = table_codes();
    let mut outside = BTreeSet::new();
    for kind in kinds {
        outside.extend(listed(&text(kind)).difference(&table).cloned());
    }
    assert!(
        outside.is_empty(),
        "the Worker Spec lists refusals outside the §6.1 table: {outside:?}"
    );
}
