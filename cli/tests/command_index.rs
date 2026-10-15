//! The command index of commands.md §2: every command exists, and every
//! command whose area has not landed fails with `not_implemented`.

mod common;

use clap::CommandFactory;
use common::TestEnv;
use cpd2::cli::Cli;

/// The commands of commands.md §2's table, read from the document itself,
/// each as its words (`["context", "decide"]`, `["plan", "phase", "plan"]`).
fn documented_commands() -> Vec<Vec<String>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/v2/commands.md");
    let doc = std::fs::read_to_string(path).expect("read docs/v2/commands.md");
    let section = doc.split("\n## 2. Command index\n").nth(1).expect("§2 exists");
    let section = section.split("\n## ").next().unwrap();
    let mut commands = Vec::new();
    for row in section.lines().filter(|l| l.starts_with("| `")) {
        let first_cell = row.split('|').nth(1).unwrap();
        // "`job add`, `edit`, `cancel`": later bare words share the first's group;
        // "`doctor`, `config`, `tiers`": a one-word first item lists top-level commands.
        let items: Vec<Vec<String>> = first_cell
            .split('`')
            .skip(1)
            .step_by(2)
            .map(|item| item.split_whitespace().map(str::to_owned).collect())
            .collect();
        let group = if items[0].len() > 1 { Some(items[0][0].clone()) } else { None };
        let before = commands.len();
        for item in items {
            match &group {
                Some(group) if item.len() == 1 => commands.push(vec![group.clone(), item[0].clone()]),
                _ => commands.push(item),
            }
        }
        assert!(commands.len() > before, "row yields no command: {row}");
    }
    // Anchors prove the parser found the table; the contract may add or drop other commands.
    for anchor in [&["context", "new"][..], &["plan", "new"], &["job", "add"], &["watch"]] {
        assert!(
            commands.iter().any(|c| c.iter().map(String::as_str).eq(anchor.iter().copied())),
            "anchor command {anchor:?} not parsed: {commands:?}"
        );
    }
    commands
}

#[test]
fn every_documented_command_exists() {
    for words in documented_commands() {
        let mut cmd = Cli::command();
        let mut rest = words.as_slice();
        while let Some((word, tail)) = rest.split_first() {
            match cmd.find_subcommand(word) {
                Some(sub) => {
                    cmd = sub.clone();
                    rest = tail;
                }
                None => break,
            }
        }
        // What is left must be positional arguments of a leaf command (`plan phase plan`).
        assert!(
            rest.len() < words.len() && (rest.is_empty() || !cmd.has_subcommands()),
            "cpd2 {} is not a command",
            words.join(" ")
        );
    }
}

/// Commands not implemented yet, by area: `(argv, details.command)`.
/// When an area's Job lands a command, it deletes that command's line here
/// (and tests the command in its own test file). Areas are separated by a
/// comment line so that parallel Jobs never edit adjacent lines.
const PENDING: &[(&[&str], &str)] = &[
    // context
    // plan
    (&["plan", "new", "pay-4521", "--context", "payments"], "plan new"),
    (&["plan", "list"], "plan list"),
    (&["plan", "show", "--plan", "cpd2"], "plan show"),
    (&["plan", "phase", "plan"], "plan phase"),
    (&["plan", "render"], "plan render"),
    (&["plan", "review"], "plan review"),
    (&["plan", "decide", "r1", "--approve"], "plan decide"),
    (&["plan", "lock"], "plan lock"),
    // job
    (&["job", "add", "api-schema", "--tier", "S2"], "job add"),
    (&["job", "edit", "api-schema", "--no-needs"], "job edit"),
    (&["job", "cancel", "api-schema", "--reason", "Not needed."], "job cancel"),
    (&["job", "retry", "api-schema", "--tier", "O4"], "job retry"),
    (&["job", "list"], "job list"),
    (&["job", "show", "api-schema"], "job show"),
    // attempt
    (&["attempt", "submit", "api-schema-a1"], "attempt submit"),
    (&["attempt", "read", "api-schema-a1", "--lines", "10"], "attempt read"),
    (&["attempt", "show", "api-schema-a1"], "attempt show"),
    (&["attempt", "list", "--active"], "attempt list"),
    // driver
    (&["attempt", "run", "api-schema-a1", "--plan", "cpd2"], "attempt run"),
    (&["job", "dispatch", "api-schema"], "job dispatch"),
    // review
    (&["review", "decide", "api-schema-a1", "--approve", "--feedback", "Fine."], "review decide"),
    // ask
    (&["ask", "api-schema-a1", "Which field names?"], "ask"),
    (&["answer", "api-schema-a1-q1", "The v2 ones."], "answer"),
    // pr
    (&["pr", "open"], "pr open"),
    (&["pr", "rebase"], "pr rebase"),
    (&["pr", "poll"], "pr poll"),
    // session
    // events
    // watch
    (&["watch"], "watch"),
    (&["watch", "--ensure"], "watch"),
    // tui
    (&["tui"], "tui"),
    (&["tui", "--attach"], "tui"),
    (&["tui", "view", "tree"], "tui view"),
];

#[test]
fn pending_commands_fail_with_not_implemented() {
    let env = TestEnv::new();
    for (argv, command) in PENDING {
        let run = env.cpd2(argv);
        assert_eq!(run.exit, 1, "cpd2 {argv:?}: {run:?}");
        let error = run.error();
        assert_eq!(error["code"], "not_implemented", "cpd2 {argv:?}");
        assert_eq!(error["details"]["command"], *command, "cpd2 {argv:?}");
        assert!(error["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert_eq!(run.stderr, "", "cpd2 {argv:?}");
    }
}
