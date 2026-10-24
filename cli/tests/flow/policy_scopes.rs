//! The three policy scopes (contract §8.2, §8.3; commands.md §14.2, §22.1): the machine is a full
//! scope (`host.toml`), the more specific scope wins per domain, and each domain is exposed to some
//! scopes only. The built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{refused, Run, TestEnv};
use serde_json::{json, Value};

const OWNER_EMPTY: &str = "";
const MCP_ENTRY: &str = "[mcp.servers.x]\nurl = \"https://example.com/mcp\"\n";

struct Fx {
    env: TestEnv,
}

impl Fx {
    /// Machine `pro`; owner `acme` and repo `acme/r` with an empty file each (a repo file is required).
    fn new() -> Fx {
        Fx::with_config("version = 1\nmachine = \"pro\"\n")
    }

    fn with_config(config: &str) -> Fx {
        let env = TestEnv::new();
        env.write_config(config);
        let fx = Fx { env };
        fx.write("acme/owner.toml", OWNER_EMPTY);
        fx.write("acme/r.toml", "");
        fx
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.env.cpd_home().join("policy").join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn real(&self, rel: &str) -> String {
        fs::canonicalize(self.path(rel)).unwrap().display().to_string()
    }

    fn show(&self, args: &[&str]) -> Run {
        let mut all = vec!["policy", "show"];
        all.extend(args);
        self.env.cpd(&all)
    }

    /// `policy show --owner acme`, ok.
    fn owner(&self) -> Value {
        self.show(&["--owner", "acme"]).ok()
    }

    fn repo(&self) -> Value {
        self.show(&["--owner", "acme", "--repo", "r"]).ok()
    }
}

// ------------------------------------------- i2, i3: the machine is a full scope

#[test]
fn the_machine_file_sets_a_domain_with_source_machine() {
    let fx = Fx::new();
    fx.write("host.toml", "[tests]\ntext = \"Machine tests.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], format!("machine:{}", fx.real("host.toml")));
    assert_eq!(out["policy"]["tests"]["text"], "Machine tests.");
}

#[test]
fn with_no_machine_file_the_domain_is_the_default() {
    let fx = Fx::new();
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], "default");
    assert_eq!(out["policy"]["tests"]["text"], "");
}

#[test]
fn the_owner_wins_over_the_machine() {
    let fx = Fx::new();
    fx.write("host.toml", "[tests]\ntext = \"Machine tests.\"\n");
    fx.write("acme/owner.toml", "[tests]\ntext = \"Owner tests.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], format!("owner:{}", fx.real("acme/owner.toml")));
    assert_eq!(out["policy"]["tests"]["text"], "Owner tests.");
}

#[test]
fn the_repo_wins_over_the_owner_and_the_machine() {
    let fx = Fx::new();
    fx.write("host.toml", "[tests]\ntext = \"Machine tests.\"\n");
    fx.write("acme/owner.toml", "[tests]\ntext = \"Owner tests.\"\n");
    fx.write("acme/r.toml", "[tests]\ntext = \"Repo tests.\"\n");
    let out = fx.repo();
    assert_eq!(out["policy"]["tests"]["source"], format!("repo:{}", fx.real("acme/r.toml")));
    assert_eq!(out["policy"]["tests"]["text"], "Repo tests.");
}

// ------------------------------------------------ i4: whole-domain replacement

#[test]
fn a_more_specific_domain_replaces_the_machines_whole_domain() {
    let fx = Fx::new();
    fx.write("host.toml", "[pr]\ngate = \"local\"\ntext = \"Machine pr.\"\n");
    fx.write("acme/owner.toml", "[pr]\ntext = \"Owner pr.\"\n");
    let out = fx.owner();
    let owner = format!("owner:{}", fx.real("acme/owner.toml"));
    let pr = &out["policy"]["pr"];
    assert_eq!(pr["text"], "Owner pr.");
    // The built-in default gate is `ci`; the machine's `local` never fills in.
    assert_eq!(pr["keys"]["gate"], json!({"value": "ci", "source": format!("{owner} (key default)")}));
}

#[test]
fn the_machines_gate_is_never_inherited_by_an_owner_domain_that_leaves_it_out() {
    // The same rule with the machine's value `ci` and the default taken from the built-in default.
    let fx = Fx::new();
    fx.write("host.toml", "[pr]\ngate = \"ci\"\ntext = \"Machine pr.\"\n");
    fx.write("acme/owner.toml", "[pr]\ntext = \"Owner pr.\"\n");
    let gate = &fx.owner()["policy"]["pr"]["keys"]["gate"];
    assert_eq!(gate["value"], "ci");
    assert!(gate["source"].as_str().unwrap().ends_with(" (key default)"), "{gate}");
    assert!(gate["source"].as_str().unwrap().starts_with("owner:"), "{gate}");
}

// --------------------------------------------------------------- i6: `files`

#[test]
fn files_lists_machine_then_owner_then_repo() {
    let fx = Fx::new();
    fx.write("host.toml", "");
    fx.write("acme/r.toml", "");
    let out = fx.repo();
    assert_eq!(
        out["files"],
        json!([fx.real("host.toml"), fx.real("acme/owner.toml"), fx.real("acme/r.toml")])
    );
}

#[test]
fn files_leaves_out_absent_files() {
    let fx = Fx::new();
    assert_eq!(fx.repo()["files"], json!([fx.real("acme/owner.toml"), fx.real("acme/r.toml")]));
    fx.write("host.toml", "");
    assert_eq!(
        fx.repo()["files"],
        json!([fx.real("host.toml"), fx.real("acme/owner.toml"), fx.real("acme/r.toml")])
    );
}

// ------------------------------------------------------------ i7: exposure

#[test]
fn the_text_domains_are_accepted_in_all_three_scopes() {
    let text = "[pr]\ngate = \"local\"\n\n[tests]\ntext = \"t\"\n\n[code]\ntext = \"c\"\n";
    for rel in ["host.toml", "acme/owner.toml", "acme/r.toml"] {
        let fx = Fx::new();
        fx.write(rel, text);
        fx.repo();
    }
}

// ------------------------------------------------ i8: the machine's provider is a default

fn provider(out: &Value) -> (&str, &str) {
    let key = &out["policy"]["model"]["keys"]["provider"];
    (key["value"].as_str().unwrap(), key["source"].as_str().unwrap())
}

// ------------------------------------------- i9: a machine file is validated like any scope file

const SHOWS: [&[&str]; 2] = [&["--owner", "acme"], &["--owner", "acme", "--repo", "r"]];

fn show_forms(fx: &Fx) -> Vec<Run> {
    let mut runs: Vec<Run> = SHOWS.iter().map(|args| fx.show(args)).collect();
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', 'acme', 't', 'pro');");
    runs.push(fx.show(&["--context", "ctx"]));
    runs
}

#[test]
fn a_machine_file_with_an_unknown_domain_a_wrong_type_or_a_bad_value_is_policy_invalid() {
    for (text, key) in [
        ("[nonsense]\n", "nonsense"),
        ("[tests]\ntext = 1\n", "tests.text"),
        ("[pr]\ngate = \"sometimes\"\n", "pr.gate"),
    ] {
        let fx = Fx::new();
        fx.write("host.toml", text);
        for run in show_forms(&fx) {
            let error = refused(&run, "policy_invalid", 1);
            assert_eq!(error["details"]["file"], fx.real("host.toml"), "{text}");
            assert_eq!(error["details"]["key"], key, "{text}");
        }
    }
}
