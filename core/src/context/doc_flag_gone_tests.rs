//! The distiller's line has no `doc` field, the prompt does not name it, and `context-curated` carries no
//! `docs` count (docs/commands.md §3.13, contract §4.6 and §5.2). No process, file or sleep.

use std::collections::HashMap;

use super::distill::Batch;
use super::render::Indexed;
use super::{By, Draft, Record};
use crate::ids::RecordId;

fn decision(seq: i64, text_id: &str) -> Indexed {
    (
        seq,
        Record {
            id: RecordId::parse(text_id).unwrap(),
            at: "2026-10-01T10:00:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: "Use it.".into(),
            draft: Draft::Decision { supersedes: vec![] },
        },
    )
}

fn counts(line: &str) -> (usize, usize) {
    let d1 = decision(1, "hs-d1");
    let records = [&d1];
    let batch = Batch {
        records: &records,
        in_force: HashMap::new(),
        hits: HashMap::new(),
    };
    let checked = batch.check(line);
    (checked.rejected.len(), checked.curations.len())
}

#[test]
fn a_line_without_doc_is_accepted() {
    let line = r#"{"id":"hs-d1","topic":"storage","short":"sqlite","retire":null}"#;
    assert_eq!(counts(line), (0, 1));
}

#[test]
fn a_line_with_a_doc_key_is_rejected() {
    let line = r#"{"id":"hs-d1","topic":"storage","short":"sqlite","retire":null,"doc":false}"#;
    assert_eq!(counts(line), (1, 0));
}

#[test]
fn the_prompt_names_no_doc_field_and_keeps_documented() {
    let prompt = include_str!("distill_prompt.md");
    assert!(!prompt.contains("\"doc\""), "the prompt names the doc key");
    assert!(!prompt.contains("- `doc`:"), "the prompt has a doc bullet");
    assert!(prompt.contains("documented"), "documented is gone");
    assert!(prompt.contains("docs.md"), "docs.md is gone");
}

#[test]
fn context_curated_carries_no_docs_count() {
    let spec = crate::events::Kind::ContextCurated.spec();
    assert_eq!(spec.required, ["context", "curated", "retire"]);
    assert_eq!(spec.optional, ["rejected"]);
}
