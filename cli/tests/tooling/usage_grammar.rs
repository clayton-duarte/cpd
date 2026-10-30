//! pi/test/cpd-usage.json is generated from clap's command tree and pinned here (docs/implementing.md, "The fake cpd
//! refuses what cpd refuses").

use clap::{Arg, Command, CommandFactory};
use cpd::cli::Cli;
use serde_json::{json, Map, Value};

const REGENERATE: &str = "CPD_BLESS=1 scripts/cargo.sh test -p cpd --test tooling usage_grammar";

fn is_required_flag(cmd: &Command, arg: &Arg) -> bool {
    arg.is_required_set() && !cmd.get_groups().any(|g| g.get_args().any(|id| id == arg.get_id()))
}

fn usage(path: &str, cmd: &Command) -> Value {
    let mut flags = Map::new();
    let mut required = Vec::new();
    let (mut min, mut count, mut unbounded) = (0usize, 0usize, false);
    for arg in cmd.get_arguments() {
        let range = arg.get_num_args().expect("built command has num_args");
        if let Some(long) = arg.get_long() {
            let takes = match (range.min_values(), range.max_values()) {
                (0, 0) => false,
                (1, 1) => true,
                (lo, hi) => panic!("`{path}` argument '--{long}' takes {lo}..{hi} values; the grammar file cannot express it"),
            };
            flags.insert(format!("--{long}"), Value::Bool(takes));
            if is_required_flag(cmd, arg) {
                required.push(format!("--{long}"));
            }
        } else if arg.is_positional() {
            count += 1;
            if arg.is_required_set() {
                min += 1;
            }
            if range.max_values() > 1 {
                unbounded = true;
            }
        }
    }
    required.sort();
    json!({
        "flags": flags,
        "positionals": { "min": min, "max": if unbounded { Value::Null } else { json!(count) } },
        "required": required,
    })
}

fn walk(cmd: &Command, path: &str, out: &mut Map<String, Value>) {
    let subs: Vec<&Command> = cmd.get_subcommands().filter(|c| c.get_name() != "help").collect();
    if path.is_empty() || subs.is_empty() || !cmd.is_subcommand_required_set() {
        out.insert(path.to_owned(), usage(path, cmd));
    }
    for c in subs {
        let p = if path.is_empty() { c.get_name().to_owned() } else { format!("{path} {}", c.get_name()) };
        walk(c, &p, out);
    }
}

fn render() -> String {
    let mut cmd = Cli::command();
    cmd.build();
    let mut out = Map::new();
    walk(&cmd, "", &mut out);
    let mut text = serde_json::to_string_pretty(&Value::Object(out)).unwrap();
    text.push('\n');
    text
}

#[test]
fn usage_file_is_generated_from_the_binary() {
    let page = render();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../pi/test/cpd-usage.json");
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
                panic!("pi/test/cpd-usage.json differs from the clap tree at line {n}:\n  file:   {x:?}\n  binary: {y:?}\nRegenerate: {REGENERATE}");
            }
            if x.is_none() {
                break;
            }
        }
        panic!("pi/test/cpd-usage.json differs from the clap tree. Regenerate: {REGENERATE}");
    }
}
