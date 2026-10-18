//! The command index of commands.md §2: every documented command exists.

use clap::CommandFactory;
use cpd::cli::Cli;

/// The commands of commands.md §2's table, read from the document itself,
/// each as its words (`["context", "decide"]`, `["plan", "phase", "plan"]`).
fn documented_commands() -> Vec<Vec<String>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/commands.md");
    let doc = std::fs::read_to_string(path).expect("read docs/commands.md");
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
            "cpd {} is not a command",
            words.join(" ")
        );
    }
}

/// `session close` is documented in §2 (added with the command, parity amendment 14) and exists.
#[test]
fn session_close_is_indexed_and_exists() {
    assert!(documented_commands().contains(&vec!["session".to_owned(), "close".to_owned()]));
    assert!(Cli::command().find_subcommand("session").unwrap().find_subcommand("close").is_some());
}

/// `pr add`, `pr edit` and `pr cancel` are documented in §2 (R34) and exist.
#[test]
fn pr_crud_is_indexed_and_exists() {
    let documented = documented_commands();
    let pr = Cli::command().find_subcommand("pr").unwrap().clone();
    for word in ["add", "edit", "cancel"] {
        assert!(documented.contains(&vec!["pr".to_owned(), word.to_owned()]), "pr {word} is not in §2");
        assert!(pr.find_subcommand(word).is_some(), "pr {word} does not exist");
    }
}

/// `context sync` went with the git store (Plan manager B1, hs-a1): neither documented nor a command.
#[test]
fn context_sync_is_neither_indexed_nor_a_command() {
    assert!(!documented_commands().contains(&vec!["context".to_owned(), "sync".to_owned()]));
    assert!(Cli::command().find_subcommand("context").unwrap().find_subcommand("sync").is_none());
}

/// `pr start` is documented in §2 (R34, §9.7) and exists.
#[test]
fn pr_start_is_indexed_and_exists() {
    assert!(documented_commands().contains(&vec!["pr".to_owned(), "start".to_owned()]));
    assert!(Cli::command().find_subcommand("pr").unwrap().find_subcommand("start").is_some());
}

/// `context pending` is documented in §2 (hs-d159) and exists.
#[test]
fn context_pending_is_indexed_and_exists() {
    assert!(documented_commands().contains(&vec!["context".to_owned(), "pending".to_owned()]));
    assert!(Cli::command().find_subcommand("context").unwrap().find_subcommand("pending").is_some());
}

/// `pr push` is documented in §2 (hs-d139) and exists.
#[test]
fn pr_push_is_indexed_and_exists() {
    assert!(documented_commands().contains(&vec!["pr".to_owned(), "push".to_owned()]));
    assert!(Cli::command().find_subcommand("pr").unwrap().find_subcommand("push").is_some());
}

/// `plan cancel` is documented in §2 (E1) and exists.
#[test]
fn plan_cancel_is_indexed_and_exists() {
    assert!(documented_commands().contains(&vec!["plan".to_owned(), "cancel".to_owned()]));
    assert!(Cli::command().find_subcommand("plan").unwrap().find_subcommand("cancel").is_some());
}

/// The code `internal` is in the error table of commands.md §1.3 and of contract §1.2 (store-W7).
#[test]
fn internal_is_listed_in_both_error_tables() {
    let docs = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs");
    for file in ["commands.md", "contract.md"] {
        let doc = std::fs::read_to_string(format!("{docs}/{file}")).unwrap();
        assert!(doc.lines().any(|l| l.starts_with('|') && l.contains("`internal`")), "{file} does not list `internal`");
    }
    assert_eq!(cpd_core::error::ErrorCode::Internal.as_str(), "internal");
}
