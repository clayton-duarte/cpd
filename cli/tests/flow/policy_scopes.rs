//! The three policy scopes (contract §8.2 `host`, §8.3; commands.md §14.2, §22.1): the machine is a full
//! scope named by the host, the more specific scope wins per domain, and each domain is exposed to some
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
    /// Machine `pro`, host `mac-pro`; owner `acme` and repo `acme/r` with an empty file each (a repo file is required).
    fn new() -> Fx {
        Fx::with_config("version = 1\nmachine = \"pro\"\nhost = \"mac-pro\"\n")
    }

    fn with_config(config: &str) -> Fx {
        let env = TestEnv::new();
        env.write_config(config);
        let fx = Fx { env };
        fx.write("owners/acme.toml", OWNER_EMPTY);
        fx.write("repos/acme/r.toml", "");
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

// ------------------------------------------------------------------ i1: `host`

fn config_of(text: &str) -> Run {
    let env = TestEnv::new();
    env.write_config(text);
    env.cpd(&["config"])
}

#[test]
fn host_defaults_to_machine_with_source_default() {
    let out = config_of("version = 1\nmachine = \"pro\"\n").ok();
    assert_eq!(out["config"]["host"], "pro", "{out}");
    assert_eq!(out["sources"]["host"], "default", "{out}");
}

#[test]
fn host_set_in_the_file_has_source_file() {
    let out = config_of("version = 1\nmachine = \"pro\"\nhost = \"mac-pro\"\n").ok();
    assert_eq!(out["config"]["host"], "mac-pro", "{out}");
    assert_eq!(out["sources"]["host"], "file", "{out}");
    assert_eq!(out["config"]["machine"], "pro", "{out}");
}

#[test]
fn a_bad_host_is_config_invalid_naming_host() {
    // The key is known: a valid host is read, so a refusal below is about the value.
    config_of("version = 1\nmachine = \"pro\"\nhost = \"mac-pro\"\n").ok();
    let long = "a".repeat(33);
    for bad in ["Mac Pro", "1mac", "MAC", long.as_str(), ""] {
        let run = config_of(&format!("version = 1\nmachine = \"pro\"\nhost = \"{bad}\"\n"));
        let error = refused(&run, "config_invalid", 1);
        assert_eq!(error["details"]["key"], "host", "{bad:?}: {error}");
    }
}

#[test]
fn a_valid_host_is_accepted() {
    let long = "a".repeat(32);
    for good in ["a", "mac-pro", "homeserver", long.as_str()] {
        let out = config_of(&format!("version = 1\nmachine = \"pro\"\nhost = \"{good}\"\n")).ok();
        assert_eq!(out["config"]["host"], good, "{out}");
    }
}

// ------------------------------------------- i2, i3: the machine is a full scope

#[test]
fn the_machine_file_sets_a_domain_with_source_machine() {
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "[tests]\ntext = \"Machine tests.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], format!("machine:{}", fx.real("machines/mac-pro.toml")));
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
    fx.write("machines/mac-pro.toml", "[tests]\ntext = \"Machine tests.\"\n");
    fx.write("owners/acme.toml", "[tests]\ntext = \"Owner tests.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], format!("owner:{}", fx.real("owners/acme.toml")));
    assert_eq!(out["policy"]["tests"]["text"], "Owner tests.");
}

#[test]
fn the_repo_wins_over_the_owner_and_the_machine() {
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "[tests]\ntext = \"Machine tests.\"\n");
    fx.write("owners/acme.toml", "[tests]\ntext = \"Owner tests.\"\n");
    fx.write("repos/acme/r.toml", "[tests]\ntext = \"Repo tests.\"\n");
    let out = fx.repo();
    assert_eq!(out["policy"]["tests"]["source"], format!("repo:{}", fx.real("repos/acme/r.toml")));
    assert_eq!(out["policy"]["tests"]["text"], "Repo tests.");
}

// ------------------------------------------------ i4: whole-domain replacement

#[test]
fn a_more_specific_domain_replaces_the_machines_whole_domain() {
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "[pr]\ngate = \"local\"\ntext = \"Machine pr.\"\n");
    fx.write("owners/acme.toml", "[pr]\ntext = \"Owner pr.\"\n");
    let out = fx.owner();
    let owner = format!("owner:{}", fx.real("owners/acme.toml"));
    let pr = &out["policy"]["pr"];
    assert_eq!(pr["text"], "Owner pr.");
    // The built-in default gate is `ci`; the machine's `local` never fills in.
    assert_eq!(pr["keys"]["gate"], json!({"value": "ci", "source": format!("{owner} (key default)")}));
}

#[test]
fn the_machines_gate_is_never_inherited_by_an_owner_domain_that_leaves_it_out() {
    // The same rule with the machine's value `ci` and the default taken from the built-in default.
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "[pr]\ngate = \"ci\"\ntext = \"Machine pr.\"\n");
    fx.write("owners/acme.toml", "[pr]\ntext = \"Owner pr.\"\n");
    let gate = &fx.owner()["policy"]["pr"]["keys"]["gate"];
    assert_eq!(gate["value"], "ci");
    assert!(gate["source"].as_str().unwrap().ends_with(" (key default)"), "{gate}");
    assert!(gate["source"].as_str().unwrap().starts_with("owner:"), "{gate}");
}

// ---------------------------------------------------------- i5: named by the host

#[test]
fn the_machine_file_is_named_by_the_host_not_the_machine() {
    let fx = Fx::new();
    fx.write("machines/pro.toml", "[tests]\ntext = \"Named by machine.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], "default");
    assert_eq!(out["files"], json!([fx.real("owners/acme.toml")]));
}

#[test]
fn with_no_host_key_the_machine_file_is_named_by_the_machine() {
    let fx = Fx::with_config("version = 1\nmachine = \"pro\"\n");
    fx.write("machines/pro.toml", "[tests]\ntext = \"Named by machine.\"\n");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["source"], format!("machine:{}", fx.real("machines/pro.toml")));
}

// --------------------------------------------------------------- i6: `files`

#[test]
fn files_lists_machine_then_owner_then_repo() {
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "");
    fx.write("repos/acme/r.toml", "");
    let out = fx.repo();
    assert_eq!(
        out["files"],
        json!([fx.real("machines/mac-pro.toml"), fx.real("owners/acme.toml"), fx.real("repos/acme/r.toml")])
    );
}

#[test]
fn files_leaves_out_absent_files() {
    let fx = Fx::new();
    assert_eq!(fx.repo()["files"], json!([fx.real("owners/acme.toml"), fx.real("repos/acme/r.toml")]));
    fx.write("machines/mac-pro.toml", "");
    assert_eq!(
        fx.repo()["files"],
        json!([fx.real("machines/mac-pro.toml"), fx.real("owners/acme.toml"), fx.real("repos/acme/r.toml")])
    );
}

// ------------------------------------------------------------ i7: exposure

#[test]
fn the_text_domains_are_accepted_in_all_three_scopes() {
    let text = "[pr]\ngate = \"local\"\n\n[tests]\ntext = \"t\"\n\n[code]\ntext = \"c\"\n";
    for rel in ["machines/mac-pro.toml", "owners/acme.toml", "repos/acme/r.toml"] {
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
        fx.write("machines/mac-pro.toml", text);
        for run in show_forms(&fx) {
            let error = refused(&run, "policy_invalid", 1);
            assert_eq!(error["details"]["file"], fx.real("machines/mac-pro.toml"), "{text}");
            assert_eq!(error["details"]["key"], key, "{text}");
        }
    }
}

#[test]
fn an_entry_of_machines_that_is_not_a_valid_host_toml_is_policy_invalid() {
    for name in ["Mac Pro.toml", "x.txt"] {
        let fx = Fx::new();
        fx.write(&format!("machines/{name}"), "");
        fx.write("machines/mac-pro.toml", "");
        for run in show_forms(&fx) {
            let error = refused(&run, "policy_invalid", 1);
            assert_eq!(error["details"]["file"], fx.path(&format!("machines/{name}")).display().to_string(), "{name}");
        }
    }
}

#[test]
fn only_this_hosts_machine_file_is_read() {
    let fx = Fx::new();
    fx.write("machines/mac-pro.toml", "[tests]\ntext = \"Mine.\"\n");
    fx.write("machines/other.toml", "not toml = = =");
    let out = fx.owner();
    assert_eq!(out["policy"]["tests"]["text"], "Mine.");
    assert_eq!(out["files"], json!([fx.real("machines/mac-pro.toml"), fx.real("owners/acme.toml")]));
}
