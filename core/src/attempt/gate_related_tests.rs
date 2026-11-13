//! Red tests for the `related` key of `[test-commands]` (bug b68): the Gate runs it on the changed files and unions
//! its tests with the changed tests; non-zero is `gate_related_failed`; an absent key is today's behaviour.
//! The command runner is a closure: no test spawns anything.

use serde_json::{Map, Value, json};

use super::kind::{RelatedRun, gate_tests, split_unknown};
use crate::policy::Scope;

const JEST: &str = "jest --findRelatedTests {{ files }}";

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn none() -> Map<String, Value> {
    Map::new()
}

fn ok(stdout: &str) -> RelatedRun {
    RelatedRun {
        exit: 0,
        stdout: stdout.to_owned(),
    }
}

#[test]
fn related_command_runs_with_changed_files() {
    let mut commands = Vec::new();
    let files = names(&["src/app.ts", "test/app.test.ts"]);
    gate_tests(Some(JEST), &files, &[], &none(), &mut |c| {
        commands.push(c.to_owned());
        ok("[]")
    })
    .unwrap();
    assert_eq!(
        commands,
        vec!["jest --findRelatedTests src/app.ts test/app.test.ts".to_owned()]
    );
}

#[test]
fn related_results_union_with_changed_tests() {
    let got = gate_tests(
        Some(JEST),
        &names(&["src/app.ts"]),
        &names(&["a.test.ts", "b.test.ts"]),
        &none(),
        &mut |_| ok(r#"["b.test.ts","c.test.ts"]"#),
    )
    .unwrap();
    let mut got = got;
    got.sort();
    assert_eq!(got, names(&["a.test.ts", "b.test.ts", "c.test.ts"]));
}

#[test]
fn missing_related_key_keeps_current_behaviour() {
    // No key: nothing runs and the changed tests are the result.
    let mut ran = 0;
    let got = gate_tests(
        None,
        &names(&["src/a.ts"]),
        &names(&["a.test.ts"]),
        &none(),
        &mut |_| {
            ran += 1;
            ok(r#"["x.test.ts"]"#)
        },
    )
    .unwrap();
    assert_eq!((ran, got), (0, names(&["a.test.ts"])));
    // The key is optional and parsed: absent is None, set is kept.
    let table = |text: &str| -> Option<String> {
        let scope: Scope = toml::from_str(text).unwrap();
        scope
            .test_commands
            .unwrap()
            .commands()
            .unwrap()
            .unwrap()
            .related
    };
    assert_eq!(
        table("[test-commands]\nbuild = \"true\"\nrun = \"true\"\n"),
        None
    );
    assert_eq!(
        table(&format!(
            "[test-commands]\nbuild = \"true\"\nrun = \"true\"\nrelated = \"{JEST}\"\n"
        )),
        Some(JEST.to_owned())
    );
}

#[test]
fn related_failure_fails_the_gate() {
    let err = gate_tests(
        Some(JEST),
        &names(&["src/a.ts"]),
        &names(&["a.test.ts"]),
        &none(),
        &mut |_| RelatedRun {
            exit: 1,
            stdout: String::new(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code().as_str(), "gate_related_failed");
}

#[test]
fn files_placeholder_lists_changed_non_test_files() {
    let mut commands = Vec::new();
    let files = names(&["core/src/lib.rs", "core/src/util.rs"]);
    gate_tests(
        Some("related {{ files }}"),
        &files,
        &[],
        &none(),
        &mut |c| {
            commands.push(c.to_owned());
            ok("[]")
        },
    )
    .unwrap();
    assert_eq!(
        commands,
        vec!["related core/src/lib.rs core/src/util.rs".to_owned()]
    );
}

#[test]
fn related_nonzero_is_not_swallowed() {
    // Even with test names printed and changed tests present, a non-zero exit is an error naming the exit.
    let err = gate_tests(
        Some(JEST),
        &names(&["src/a.ts"]),
        &names(&["a.test.ts"]),
        &none(),
        &mut |_| RelatedRun {
            exit: 2,
            stdout: r#"["b.test.ts"]"#.to_owned(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code().as_str(), "gate_related_failed");
    assert!(err.message().contains('2'), "{}", err.message());
}

#[test]
fn related_output_is_a_json_array_not_one_literal_name() {
    let got = gate_tests(Some(JEST), &[], &[], &none(), &mut |_| {
        ok("[\"a.test.ts\", \"b.test.ts\"]\n")
    })
    .unwrap();
    assert_eq!(got, names(&["a.test.ts", "b.test.ts"]));
    for bad in ["a.test.ts\nb.test.ts\n", "not json", "{\"a\":1}", "[1]", ""] {
        let err = gate_tests(Some(JEST), &[], &[], &none(), &mut |_| ok(bad)).unwrap_err();
        assert_eq!(err.code().as_str(), "gate_related_failed", "{bad:?}");
    }
}

#[test]
fn a_name_only_related_found_that_list_rejects_is_gate_related_failed() {
    let (named, found) = split_unknown(
        &names(&["a.test.ts"]),
        &names(&["a.test.ts", "ghost.test.ts"]),
    );
    assert_eq!(
        (named, found),
        (names(&["a.test.ts"]), names(&["ghost.test.ts"]))
    );
}

#[test]
fn related_renders_with_the_hook_context_and_files() {
    let mut context = none();
    context.insert("repo_path".to_owned(), json!("/r"));
    context.insert("branch".to_owned(), json!("feat"));
    let mut commands = Vec::new();
    gate_tests(
        Some("rel {{ repo_path }} {{ branch }} {{ files | length }} {{ files }}"),
        &names(&["a.rs", "b.rs"]),
        &[],
        &context,
        &mut |c| {
            commands.push(c.to_owned());
            ok("[]")
        },
    )
    .unwrap();
    assert_eq!(commands, vec!["rel /r feat 2 a.rs b.rs".to_owned()]);
    let err = gate_tests(Some("{{ nope }}"), &[], &[], &context, &mut |_| ok("[]")).unwrap_err();
    assert_eq!(err.code().as_str(), "gate_related_failed");
}
