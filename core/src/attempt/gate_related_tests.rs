//! Red tests for the `related` key of `[test-commands]` (bug b68): the Gate runs it on the changed files and unions
//! its tests with the changed tests; non-zero is `gate_related_failed`; an absent key is today's behaviour.
//! The command runner is a closure: no test spawns anything.

use super::kind::{RelatedRun, gate_tests};
use crate::policy::Scope;

const JEST: &str = "jest --findRelatedTests {{ files }}";

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
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
    gate_tests(Some(JEST), &files, &[], &mut |c| {
        commands.push(c.to_owned());
        ok("")
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
        &mut |_| ok("b.test.ts\nc.test.ts\n"),
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
        &mut |_| {
            ran += 1;
            ok("x.test.ts\n")
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
    gate_tests(Some("related {{ files }}"), &files, &[], &mut |c| {
        commands.push(c.to_owned());
        ok("")
    })
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
        &mut |_| RelatedRun {
            exit: 2,
            stdout: "b.test.ts\n".to_owned(),
        },
    )
    .unwrap_err();
    assert_eq!(err.code().as_str(), "gate_related_failed");
    assert!(err.message().contains('2'), "{}", err.message());
}
