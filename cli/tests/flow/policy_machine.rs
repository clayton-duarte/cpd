//! `cpd policy show` and the machine scope (commands.md §22.1 "The machine object", contract §8.3
//! "The machine scope"): the built binary against a temporary `$CPD_HOME` whose `config.toml`
//! names the machine `pro`, with policy files written by hand.

use crate::common;

use std::fs;

use common::{refused, Run, TestEnv, CPD};
use serde_json::{json, Value};

const SLACK: &str = r#"
[mcp.servers.slack]
url = "https://mcp.slack.com/mcp"
description = "Slack: read channels and threads, post messages, and manage reactions."
exposure = "hidden"

[mcp.servers.slack.oauth]
clientId = "1601185624273.8899143856786"
callbackUrl = "http://localhost:3118/callback"
scope = "channels:history chat:write reactions:write"

[mcp.servers.slack.toolExposure]
slack_read_channel = "direct"
slack_read_thread = "direct"
slack_add_reaction = "direct"
slack_get_reactions = "direct"
slack_search_emojis = "direct"
slack_read_user_profile = "direct"
slack_send_message = "direct"
"#;

struct Fx {
    env: TestEnv,
}

impl Fx {
    /// Owner `acme` (an empty file), Context `ctx` (owner `acme`) with Plan `pl`, a Manager `mgr1`
    /// and a Lead `lead1`; the machine is `pro`.
    fn new() -> Fx {
        let env = TestEnv::new();
        fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"pro\"\n").unwrap();
        env.sql(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', 'acme', 't', 'pro');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr1', 'manager', 'ctx', 'M', '/', 'O3', 'idle', 't', 't');",
        );
        // A Repo key needs its repo file (`repo_unconfigured`); an empty one is valid.
        env.repo_policy("acme", "web", "");
        Fx { env }
    }

    fn machine_file(&self, text: &str) {
        self.machine_file_of("pro", text);
    }

    fn machine_file_of(&self, host: &str, text: &str) {
        let dir = self.env.cpd_home().join("policy/machines");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{host}.toml")), text).unwrap();
    }

    fn real(&self, rel: &str) -> String {
        fs::canonicalize(self.env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
    }

    fn show(&self, args: &[&str]) -> Run {
        let mut all = vec!["policy", "show"];
        all.extend(args);
        self.env.cpd(&all)
    }

    fn show_as(&self, session: &str) -> Run {
        let out = self.env.command(CPD).args(["policy", "show"]).env("CPD_SESSION", session).output().unwrap();
        Run {
            exit: out.status.code().unwrap(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

fn slack_json() -> Value {
    json!({
        "url": "https://mcp.slack.com/mcp",
        "description": "Slack: read channels and threads, post messages, and manage reactions.",
        "exposure": "hidden",
        "oauth": {"clientId": "1601185624273.8899143856786", "callbackUrl": "http://localhost:3118/callback",
                  "scope": "channels:history chat:write reactions:write"},
        "toolExposure": {"slack_read_channel": "direct", "slack_read_thread": "direct", "slack_add_reaction": "direct",
                         "slack_get_reactions": "direct", "slack_search_emojis": "direct",
                         "slack_read_user_profile": "direct", "slack_send_message": "direct"}
    })
}

// i.
#[test]
fn the_machine_object_carries_the_slack_entry_with_pis_key_names() {
    let fx = Fx::new();
    fx.machine_file(SLACK);
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["machine"]["name"], "pro");
    assert_eq!(out["machine"]["mcp"]["source"], format!("machine:{}", fx.real("machines/pro.toml")));
    assert_eq!(out["machine"]["mcp"]["servers"], json!({"slack": slack_json()}));
}

#[test]
fn the_markdown_has_a_machine_section_naming_the_server() {
    let fx = Fx::new();
    fx.machine_file(SLACK);
    let out = fx.show(&["--owner", "acme"]).ok();
    let md = out["markdown"].as_str().unwrap();
    let section = md.split("## Machine pro").nth(1).unwrap_or_else(|| panic!("no `## Machine pro` in {md}"));
    assert!(section.contains("slack"), "{md}");
    assert!(section.contains("https://mcp.slack.com/mcp"), "{md}");
    assert!(section.contains("hidden"), "{md}");
}

// j.
#[test]
fn no_machine_file_means_no_servers_from_default() {
    let fx = Fx::new();
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["machine"], json!({"name": "pro", "mcp": {"source": "default", "servers": {}}}));
}

#[test]
fn no_machine_file_says_no_mcp_servers_in_the_markdown() {
    let fx = Fx::new();
    let out = fx.show(&["--owner", "acme"]).ok();
    let md = out["markdown"].as_str().unwrap();
    assert!(md.contains("## Machine pro"), "{md}");
    assert!(md.contains("No MCP servers."), "{md}");
}

// k.
#[test]
fn every_form_carries_the_same_machine_object() {
    let fx = Fx::new();
    fx.machine_file(SLACK);
    let expected = fx.show(&["--owner", "acme"]).ok()["machine"].clone();
    assert_eq!(expected["mcp"]["servers"]["slack"], slack_json());
    assert_eq!(fx.show(&["--owner", "acme", "--repo", "web"]).ok()["machine"], expected, "--owner --repo");
    assert_eq!(fx.show(&["--context", "ctx"]).ok()["machine"], expected, "--context");
    assert_eq!(fx.show_as("mgr1").ok()["machine"], expected, "manager, no flag");
    assert_eq!(fx.show_as("lead1").ok()["machine"], expected, "lead, no flag");
}

// l.
#[test]
fn an_invalid_machine_file_fails_every_form_as_policy_invalid() {
    let fx = Fx::new();
    fx.machine_file("[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\nheaders = { A = \"b\" }\n");
    let file = fx.real("machines/pro.toml");
    let runs = [
        ("--owner", fx.show(&["--owner", "acme"])),
        ("--owner --repo", fx.show(&["--owner", "acme", "--repo", "web"])),
        ("--context", fx.show(&["--context", "ctx"])),
        ("no flag", fx.show_as("mgr1")),
    ];
    for (form, run) in &runs {
        let error = refused(run, "policy_invalid", 1);
        assert_eq!(error["details"]["file"], file, "{form}");
        assert!(error["details"]["key"].as_str().is_some_and(|k| k.contains("headers")), "{form}: {error}");
    }
}

// m.
#[test]
fn an_owner_file_with_an_mcp_key_is_policy_invalid() {
    let fx = Fx::new();
    fx.env.owner_policy("acme", "[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\n");
    let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("owners/acme.toml"));
    assert!(error["message"].as_str().unwrap().contains("machine"), "{error}");
}

// n. The machine file is keyed by the host (config key `host`), not by the machine name.
#[test]
fn policy_show_reads_the_file_of_the_host_not_of_the_machine_name() {
    let fx = Fx::new();
    fs::write(fx.env.cpd_home().join("config.toml"), "version = 1\nmachine = \"pro\"\nhost = \"mac-pro\"\n").unwrap();
    fx.machine_file_of("pro", "[mcp.servers.by-name]\nurl = \"https://by-name.example.com/mcp\"\n");
    fx.machine_file_of("mac-pro", "[mcp.servers.by-host]\nurl = \"https://by-host.example.com/mcp\"\n");
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["machine"]["name"], "pro");
    assert_eq!(out["machine"]["mcp"]["source"], format!("machine:{}", fx.real("machines/mac-pro.toml")));
    assert_eq!(out["machine"]["mcp"]["servers"], json!({"by-host": {"url": "https://by-host.example.com/mcp"}}));
}

// o. One machine file is both the machine scope and the `[mcp]` table.
#[test]
fn a_machine_file_with_pr_and_mcp_sets_the_pr_domain_and_the_servers() {
    let fx = Fx::new();
    fx.machine_file("[pr]\ngate = \"local\"\n\n[mcp.servers.x]\nurl = \"https://x.example.com/mcp\"\n");
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["policy"]["pr"]["source"], format!("machine:{}", fx.real("machines/pro.toml")));
    assert_eq!(out["machine"]["mcp"]["servers"], json!({"x": {"url": "https://x.example.com/mcp"}}));
}

// p. `[mcp]` is the machine file's alone.
#[test]
fn an_mcp_table_in_an_owner_file_is_policy_invalid_with_key_mcp() {
    let fx = Fx::new();
    fx.env.owner_policy("acme", "[mcp.servers.x]\nurl = \"https://x.example.com/mcp\"\n");
    let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("owners/acme.toml"));
    assert_eq!(error["details"]["key"], "mcp");
    assert!(error["message"].as_str().unwrap().contains("is set in a machine file"), "{error}");
}

#[test]
fn an_mcp_table_in_a_repo_file_is_policy_invalid_with_key_mcp() {
    let fx = Fx::new();
    fx.env.repo_policy("acme", "web", "[mcp.servers.x]\nurl = \"https://x.example.com/mcp\"\n");
    let error = refused(&fx.show(&["--owner", "acme", "--repo", "web"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("repos/acme/web.toml"));
    assert_eq!(error["details"]["key"], "mcp");
    assert!(error["message"].as_str().unwrap().contains("is set in a machine file"), "{error}");
}

// q. A domain the machine scope does not expose stays refused there, with or without `[mcp]`.
#[test]
fn hooks_in_a_machine_file_are_policy_invalid_by_exposure() {
    let fx = Fx::new();
    fx.machine_file("[hooks]\npre-commit = \"true\"\n");
    let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("machines/pro.toml"));
    assert_eq!(error["details"]["key"], "hooks");
}

#[test]
fn hooks_beside_mcp_in_a_machine_file_are_policy_invalid_by_exposure() {
    let fx = Fx::new();
    fx.machine_file("[mcp.servers.x]\nurl = \"https://x.example.com/mcp\"\n\n[hooks]\npre-commit = \"true\"\n");
    let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("machines/pro.toml"));
    assert_eq!(error["details"]["key"], "hooks");
}
