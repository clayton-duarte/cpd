//! `cpd tui view worker` is no longer a command (commands.md §13.1): the Job pills are not clickable, so the Worker
//! pane is gone. It is an unknown command like any other; the test runs the parser only.

use serde_json::Value;

fn names(nodes: &Value) -> Vec<String> {
    nodes
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn tui_view_worker_is_an_unknown_command() {
    let fx = crate::tui_plan_cancelled::env();
    for argv in [
        &["tui", "view", "worker", "o1-a1", "--plan", "ship"][..],
        &["tui", "view", "worker", "--job", "a", "--plan", "ship"],
    ] {
        let run = fx.cpd(argv);
        assert_eq!(run.exit, 2, "cpd {argv:?}: {run:?}");
        assert_eq!(run.error()["code"], "usage", "cpd {argv:?}");
    }

    let mut stdout = Vec::new();
    cpd::main_with(
        ["cpd".into(), "tui".into(), "view".into(), "--help".into()],
        &mut stdout,
    );
    let help = String::from_utf8(stdout).expect("utf-8 help");
    let tree: Value = serde_json::from_str(&help)
        .ok()
        .map(|j: Value| j["tree"].clone())
        .unwrap_or(Value::Null);
    if let Some(view) = tree
        .as_array()
        .and_then(|t| t.iter().find(|n| n["name"] == "tui"))
        .and_then(|t| {
            t["commands"]
                .as_array()?
                .iter()
                .find(|n| n["name"] == "view")
        })
    {
        assert!(
            !names(&view["commands"]).contains(&"worker".to_owned()),
            "tui view lists worker"
        );
    }
    assert!(
        !help.contains("worker"),
        "cpd tui view --help names worker: {help}"
    );
}
