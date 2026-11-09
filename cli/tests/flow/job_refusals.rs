//! The refusals an agent meets when writing a Job's Spec or adding a Job (commands.md §5, §16.1;
//! Plan `agent-refusals`): the `outOfScope` unknown-field refusal names the field cpd reads
//! (`out_of_scope`), and the `--tier`/`--spec` help matches the code, which requires both even
//! with `--draft` (pro-d404).

use crate::common;

use common::fx::Fx;
use common::refused;

use super::job;

/// The help line of `--tier` or `--spec` in `help`.
fn flag_line<'a>(help: &'a str, flag: &str) -> &'a str {
    help.lines()
        .find(|line| line.trim_start().starts_with(&format!("--{flag} ")))
        .unwrap_or_else(|| panic!("`--{flag}` is in the help:\n{help}"))
}

#[test]
fn out_of_scope_field_is_refused_with_a_hint() {
    let fx = Fx::new();
    job::setup(&fx.env);
    job::plan(&fx.env, "pl", "plan");

    // cpd 1's `outOfScope`, in place of the field cpd reads.
    let mut spec = job::spec_json();
    spec.as_object_mut().unwrap().remove("out_of_scope");
    spec["outOfScope"] = serde_json::json!([]);
    let file = job::write_spec(&fx.env, "outOfScope.json", &spec.to_string());

    let run = fx.cpd(&[
        "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &file, "--plan", "pl",
    ]);
    let error = refused(&run, "spec_invalid", 2);
    assert_eq!(error["details"]["path"], "$.outOfScope");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("out_of_scope"),
        "the unknown-field refusal hints at the field cpd reads: {message}"
    );
}

#[test]
fn draft_help_matches_behaviour() {
    let fx = Fx::new();
    let helped = fx.cpd(&["job", "add", "--help"]).ok();
    let help = helped["help"].as_str().unwrap();
    for flag in ["tier", "spec"] {
        let line = flag_line(help, flag);
        assert!(
            line.contains("required"),
            "`--{flag}` is required for every Job: {line}"
        );
        assert!(
            !line.contains("except with --draft"),
            "`--{flag}` is required even with --draft: {line}"
        );
    }

    // The code the help must match: `--draft` without `--tier` and `--spec` is `usage`.
    job::setup(&fx.env);
    job::plan(&fx.env, "pl", "plan");
    let run = fx.cpd(&["job", "add", "k", "--title", "T", "--draft", "--plan", "pl"]);
    let error = refused(&run, "usage", 2);
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("--tier") && message.contains("--spec"),
        "the refusal names both flags: {message}"
    );
}
