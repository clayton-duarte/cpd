//! docs/interface.md is generated from the `tree` of `cpd --help` (commands.md §2).

use clap::CommandFactory;
use cpd::cli::Cli;
use cpd::interface::TABLE;
use serde_json::Value;

const REGENERATE: &str = "CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface";

fn help_tree() -> Value {
    let mut stdout = Vec::new();
    cpd::main_with(["cpd".into(), "--help".into()], &mut stdout);
    let json: Value = serde_json::from_slice(&stdout).expect("one JSON value");
    json["tree"].clone()
}

/// A runnable node says so; a group is internal when all its commands are.
fn is_internal(node: &Value) -> bool {
    match node.get("internal") {
        Some(v) => v.as_bool().unwrap(),
        None => node["commands"].as_array().unwrap().iter().all(is_internal),
    }
}

fn rows(nodes: &Value, prefix: &str, out: &mut String) {
    for node in nodes.as_array().unwrap() {
        let path = format!("{prefix} {}", node["name"].as_str().unwrap());
        let about = node["about"].as_str().unwrap().replace('|', "\\|");
        let callers = if is_internal(node) {
            "internal".to_owned()
        } else {
            let roles: Vec<&str> = node.get("callers").map_or(vec![], |c| {
                c.as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_str().unwrap())
                    .collect()
            });
            roles.join(", ")
        };
        let cockpit = if node["cockpit"].as_bool() == Some(true) {
            "yes"
        } else {
            ""
        };
        let cell = |text: &str| {
            if text.is_empty() {
                " |".to_owned()
            } else {
                format!(" {text} |")
            }
        };
        out.push_str(&format!(
            "| `{path}` | {about} |{}{}\n",
            cell(&callers),
            cell(cockpit)
        ));
        if let Some(children) = node.get("commands") {
            rows(children, &path, out);
        }
    }
}

fn render(tree: &Value) -> String {
    let mut page = String::from(
        "# cpd's interface\n\nGenerated from the `tree` of `cpd --help` by `cli/tests/interface.rs`; do not edit by hand. Regenerate: `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface`. Callers, cockpit and internal: [commands.md §2](commands.md#2-command-index). A § without a file is commands.md's.\n\n| Command | Description | Callers | Cockpit |\n|---|---|---|---|\n",
    );
    rows(tree, "cpd", &mut page);
    page
}

fn find<'a>(tree: &'a Value, path: &str) -> &'a Value {
    let mut nodes = tree;
    let mut found = None;
    for word in path.split(' ') {
        let n = nodes
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == word)
            .unwrap_or_else(|| panic!("no command {path}"));
        nodes = n.get("commands").unwrap_or(&Value::Null);
        found = Some(n);
    }
    found.unwrap()
}

#[test]
fn interface_page_is_generated_from_the_binary() {
    let page = render(&help_tree());
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/interface.md");
    if std::env::var_os("CPD_BLESS").is_some_and(|v| v == "1") {
        std::fs::write(path, &page).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(path).unwrap_or_default();
    if on_disk != page {
        let (mut a, mut b) = (on_disk.lines(), page.lines());
        let mut n = 0;
        loop {
            n += 1;
            let (x, y) = (a.next(), b.next());
            if x != y {
                panic!(
                    "docs/interface.md differs from `cpd --help` at line {n}:\n  page:   {x:?}\n  binary: {y:?}\nRegenerate: {REGENERATE}"
                );
            }
            if x.is_none() {
                break;
            }
        }
        panic!("docs/interface.md differs from `cpd --help`. Regenerate: {REGENERATE}");
    }
}

fn runnable_paths(cmd: &clap::Command, prefix: &str, hidden: bool, out: &mut Vec<String>) {
    for c in cmd.get_subcommands().filter(|c| c.get_name() != "help") {
        let path = if prefix.is_empty() {
            c.get_name().to_owned()
        } else {
            format!("{prefix} {}", c.get_name())
        };
        let internal = hidden || c.is_hide_set();
        let has_children = c.get_subcommands().any(|s| s.get_name() != "help");
        if !internal && (!has_children || !c.is_subcommand_required_set()) {
            out.push(path.clone());
        }
        runnable_paths(c, &path, internal, out);
    }
}

#[test]
fn every_command_has_one_entry() {
    let mut paths = Vec::new();
    runnable_paths(&Cli::command(), "", false, &mut paths);
    let table: Vec<&str> = TABLE.iter().map(|e| e.0).collect();
    assert_eq!(
        table, paths,
        "cli/src/interface.rs's table and the runnable, non-internal commands must match, in clap's order"
    );
}

#[test]
fn anchors() {
    let tree = help_tree();
    assert_eq!(
        find(&tree, "plan show")["callers"],
        serde_json::json!(["Lead", "Manager"])
    );
    assert_eq!(find(&tree, "session open")["cockpit"], true);
    for internal in ["session run", "plan decide", "tui view tree"] {
        assert_eq!(find(&tree, internal)["internal"], true, "{internal}");
    }
}

#[test]
fn janitor_command_is_in_the_help_tree() {
    let tree = help_tree();
    let node = tree
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "janitor")
        .expect("`janitor` is a command of the help tree (commands.md §29)");
    assert!(
        !is_internal(node),
        "`janitor` is a runnable command the User calls: {node}"
    );
}
