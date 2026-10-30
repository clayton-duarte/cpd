//! `cpd policy show` (commands.md §22.1): the command tree, in this process (`Fx`), against a temporary
//! `$CPD_HOME`, with Contexts, Plans, PRs, Jobs, Attempts and Sessions written
//! straight into the database and policy files written by hand.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::{refused, Run};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{json, Value};

const OWNER_FILE: &str = "[gate]\nmerge = \"local\"\npr = \"local\"\n\n[pr]\ntext = \"Owner pr text.\"\n\n[code]\ntext = \"Owner code text.\"\n";
const REPO_FILE: &str = "[code]\ntext = \"Repo code text.\"\n";

/// Owner `acme` (a file with `[gate]`, `[pr]` and `[code]`), the repo file
/// `acme/cpd`, Context `ctx` (owner `acme`) with Plan `pl`, PR `named` (name `cpd`) and
/// PR `anon` (no name), a Manager, a Lead and one Worker per PR; Context `bare` has no owner.
fn world() -> Fx {
    let fx = Fx::new();
    let env = &fx.env;
    let home = env.cpd_home();
    fs::write(home.join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fs::write(home.join("policy/acme/owner.toml"), OWNER_FILE).unwrap();
    fs::write(home.join("policy/acme/cpd.toml"), REPO_FILE).unwrap();
    let fp = "0".repeat(64);
    let job = |key: &str, pr: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pl', '{pr}', '{key}', 'J', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = '{key}';
             UPDATE jobs SET status = 'running' WHERE key = '{key}';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
               VALUES ('pl', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/pl--{key}-a1', '/w', 'w-{key}', 't');
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('w-{key}', 'worker', 'ctx', 'pl', '{key}-a1', '{key}-a1', '/w', 'S2', 'working', 't', 't');"
        )
    };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', 'acme', 't', 'm1'), ('bare', 'B', NULL, 't', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', 'M', '/', 'O3', 'idle', 't', 't'),
                  ('mgr2', 'manager', 'bare', 'M', '/', 'O3', 'idle', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, name, created_txn, created_at)
           VALUES ('pl', 'named', '/r', 'main', 'waiting', 'cpd/pl/named', 'cpd', cpd_txn(), 't'),
                  ('pl', 'anon', '/r', 'main', 'waiting', 'cpd/pl/anon', NULL, cpd_txn(), 't');
         {} {}",
        job("a", "named"),
        job("b", "anon")
    ));
    fx
}

fn real(fx: &Fx, rel: &str) -> String {
    fs::canonicalize(fx.env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
}

fn show(fx: &Fx, args: &[&str]) -> Run {
    let mut all = vec!["policy", "show"];
    all.extend(args);
    fx.cpd(&all)
}

/// `policy show` as `session` (and `attempt`, as in a Worker's pane).
fn show_as(fx: &Fx, session: &str, attempt: Option<&str>) -> Run {
    let mut vars = vec![("CPD_SESSION", session)];
    if let Some(attempt) = attempt {
        vars.push(("CPD_ATTEMPT", attempt));
    }
    fx.cpd_with(fx.env.root(), &["policy", "show"], &vars)
}

fn key(out: &Value) -> (&str, Option<&str>) {
    (out["key"]["owner"].as_str().unwrap(), out["key"]["repo"].as_str())
}

fn files(fx: &Fx, rels: &[&str]) -> Value {
    json!(rels.iter().map(|r| real(fx, r)).collect::<Vec<_>>())
}

#[test]
fn the_output_has_exactly_the_documented_fields() {
    let fx = world();
    let out = show(&fx, &["--owner", "acme"]).ok();
    let mut top: Vec<&str> = out.as_object().unwrap().keys().map(String::as_str).collect();
    top.sort_unstable();
    assert_eq!(top, ["files", "key", "machine", "markdown", "ok", "policy", "role"]);
    assert_eq!(out["role"], Value::Null);
    let mut domains: Vec<&str> = out["policy"].as_object().unwrap().keys().map(String::as_str).collect();
    domains.sort_unstable();
    assert_eq!(domains, ["code", "deploy", "deploys", "gate", "hooks", "instructions", "model", "monitor", "pr", "protected", "resources", "test-commands", "tests"]);
    for (name, domain) in out["policy"].as_object().unwrap() {
        let mut fields: Vec<&str> = domain.as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["audience", "keys", "source", "text"], "{name}");
    }
    assert_eq!(out["policy"]["pr"]["audience"], json!(["lead", "worker"]));
    assert_eq!(out["policy"]["deploy"]["audience"], json!(["lead"]));
    assert_eq!(out["policy"]["instructions"]["audience"], json!(["manager", "lead", "worker"]));
    assert_eq!(out["policy"]["deploy"]["keys"], json!({}));
    let mut keys: Vec<&str> = out["policy"]["pr"]["keys"].as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["feedback.checks", "pr.human_gated_contexts", "pr.jira.done", "pr.jira.in_progress", "pr.jira.in_review", "pr.jira_required", "pr.post.channel", "pr.post.enabled", "pr.post.merged_reaction", "pr.post.template", "pr.required_checks"]);
    let mut gate: Vec<&str> = out["policy"]["gate"]["keys"].as_object().unwrap().keys().map(String::as_str).collect();
    gate.sort_unstable();
    assert_eq!(gate, ["merge", "pr"]);
    assert_eq!(out["policy"]["gate"]["audience"], json!(["lead", "worker"]));
    for k in out["policy"]["pr"]["keys"].as_object().unwrap().values() {
        let mut fields: Vec<&str> = k.as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["source", "value"]);
    }
}

#[test]
fn owner_key_reads_the_owner_file_and_marks_the_key_default() {
    let fx = world();
    let out = show(&fx, &["--owner", "Acme"]).ok();
    assert_eq!(key(&out), ("acme", None));
    assert_eq!(out["files"], files(&fx, &["acme/owner.toml"]));
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    let pr = &out["policy"]["pr"];
    assert_eq!(pr["source"], owner);
    assert_eq!(pr["text"], "Owner pr text.");
    // `feedback.checks` is left out of the set domain: cpd's default, marked.
    assert_eq!(pr["keys"]["feedback.checks"], json!({"value": "any", "source": format!("{owner} (key default)")}));
    // The `gate` domain is its own domain: the owner sets both keys.
    let gate = &out["policy"]["gate"];
    assert_eq!(gate["source"], owner);
    assert_eq!(gate["keys"]["merge"], json!({"value": "local", "source": owner}));
    assert_eq!(gate["keys"]["pr"], json!({"value": "local", "source": owner}));
    assert_eq!(out["policy"]["code"]["source"], owner);
    assert_eq!(out["policy"]["deploy"], json!({"source": "default", "audience": ["lead"], "keys": {}, "text": ""}));
}

#[test]
fn a_repo_file_overrides_one_domain() {
    let fx = world();
    let out = show(&fx, &["--owner", "acme", "--repo", "cpd"]).ok();
    assert_eq!(key(&out), ("acme", Some("cpd")));
    assert_eq!(out["files"], files(&fx, &["acme/owner.toml", "acme/cpd.toml"]));
    assert_eq!(out["policy"]["code"]["source"], format!("repo:{}", real(&fx, "acme/cpd.toml")));
    assert_eq!(out["policy"]["code"]["text"], "Repo code text.");
    assert_eq!(out["policy"]["pr"]["source"], format!("owner:{}", real(&fx, "acme/owner.toml")));
    // A repo with an empty file reads the owner's.
    fx.env.repo_policy("acme", "other", "");
    let other = show(&fx, &["--owner", "acme", "--repo", "other"]).ok();
    assert_eq!(other["files"], files(&fx, &["acme/owner.toml", "acme/other.toml"]));
    assert_eq!(other["policy"]["code"]["text"], "Owner code text.");
}

#[test]
fn an_empty_owner_file_is_cpds_default_without_key_default_marks() {
    let fx = world();
    fs::write(fx.env.cpd_home().join("policy/acme/owner.toml"), "").unwrap();
    let out = show(&fx, &["--owner", "acme"]).ok();
    assert_eq!(out["policy"]["gate"]["source"], "default");
    assert_eq!(out["policy"]["gate"]["keys"]["merge"], json!({"value": "none", "source": "default"}));
    assert_eq!(out["policy"]["gate"]["keys"]["pr"], json!({"value": "none", "source": "default"}));
    assert_eq!(out["policy"]["pr"]["source"], "default");
    assert_eq!(out["policy"]["pr"]["keys"]["feedback.checks"], json!({"value": "any", "source": "default"}));
    // A bare `[pr]` table sets the whole `pr` domain to cpd's default: every key is defaulted.
    fs::write(fx.env.cpd_home().join("policy/acme/owner.toml"), "[pr]\n").unwrap();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    let out = show(&fx, &["--owner", "acme"]).ok();
    assert_eq!(out["policy"]["pr"]["keys"]["feedback.checks"], json!({"value": "any", "source": format!("{owner} (key default)")}));
    // `[pr]` sets only the `pr` domain: `gate` stays cpd's default.
    assert_eq!(out["policy"]["gate"]["source"], "default");
}

#[test]
fn context_selects_its_owner() {
    let fx = world();
    let out = show(&fx, &["--context", "ctx"]).ok();
    assert_eq!(key(&out), ("acme", None));
    assert_eq!(out["role"], Value::Null);
}

#[test]
fn pr_selects_its_repo_or_the_owner_when_unnamed() {
    let fx = world();
    let named = show(&fx, &["--pr", "named", "--plan", "pl"]).ok();
    assert_eq!(key(&named), ("acme", Some("cpd")));
    assert_eq!(named["policy"]["code"]["text"], "Repo code text.");
    let anon = show(&fx, &["--pr", "anon", "--plan", "pl"]).ok();
    assert_eq!(key(&anon), ("acme", None));
    // The Plan may come from CPD_PLAN.
    let out = fx.cpd_with(fx.env.root(), &["policy", "show", "--pr", "named"], &[("CPD_PLAN", "pl")]);
    assert_eq!(serde_json::from_str::<Value>(&out.stdout).unwrap()["key"]["repo"], "cpd");
}

#[test]
fn no_flag_resolves_the_callers_session() {
    let fx = world();
    for (session, role) in [("mgr1", "manager"), ("lead1", "lead")] {
        let out = show_as(&fx, session, None).ok();
        assert_eq!((key(&out), out["role"].as_str()), (("acme", None), Some(role)), "{session}");
    }
    let worker = show_as(&fx, "w-a", Some("a-a1")).ok();
    assert_eq!((key(&worker), worker["role"].as_str()), (("acme", Some("cpd")), Some("worker")));
    assert_eq!(worker["policy"]["code"]["text"], "Repo code text.");
    let anon = show_as(&fx, "w-b", Some("b-a1")).ok();
    assert_eq!(key(&anon), ("acme", None));
}

#[test]
fn usage_refusals() {
    let fx = world();
    refused(&show(&fx, &[]), "session_required", 2);
    refused(&show(&fx, &["--pr", "named", "--context", "ctx"]), "usage", 2);
    refused(&show(&fx, &["--context", "ctx", "--owner", "acme"]), "usage", 2);
    refused(&show(&fx, &["--pr", "named", "--owner", "acme"]), "usage", 2);
    refused(&show(&fx, &["--repo", "cpd"]), "usage", 2);
    refused(&show(&fx, &["--context", "ctx", "--repo", "cpd"]), "usage", 2);
    refused(&show(&fx, &["--owner", "-bad"]), "usage", 2);
    refused(&show(&fx, &["--owner", "acme", "--repo", "a b"]), "usage", 2);
    refused(&show(&fx, &["--pr", "named"]), "plan_required", 2);
}

#[test]
fn not_found_refusals() {
    let fx = world();
    refused(&show_as(&fx, "nope", None), "session_not_found", 4);
    refused(&show(&fx, &["--pr", "named", "--plan", "zz"]), "plan_not_found", 4);
    refused(&show(&fx, &["--pr", "zz", "--plan", "pl"]), "pr_not_found", 4);
    refused(&show(&fx, &["--context", "zz"]), "context_not_found", 4);
}

#[test]
fn owner_unset_names_the_next_command() {
    let fx = world();
    let error = refused(&show(&fx, &["--context", "bare"]), "owner_unset", 3);
    assert!(error["details"].get("next").is_none() && error["message"].as_str().unwrap().contains("cpd context owner <login> --context bare"), "{error}");
    refused(&show_as(&fx, "mgr2", None), "owner_unset", 3);
}

#[test]
fn a_repo_file_without_an_owner_file_is_policy_invalid() {
    let fx = world();
    let home = fx.env.cpd_home();
    fs::create_dir_all(home.join("policy/ghost")).unwrap();
    fs::write(home.join("policy/ghost/x.toml"), REPO_FILE).unwrap();
    // A directory of repo files with no `owner.toml` is refused whole (`policy_invalid`, the entry rules of §8.3), before the owner check.
    for args in [&["--owner", "ghost", "--repo", "x"][..], &["--owner", "ghost"][..]] {
        let error = refused(&show(&fx, args), "policy_invalid", 1);
        assert_eq!(error["details"]["file"], home.join("policy/ghost").display().to_string());
    }
}

#[test]
fn policy_invalid_names_the_file_and_key() {
    let fx = world();
    let owner: PathBuf = fx.env.cpd_home().join("policy/acme/owner.toml");
    let repo: PathBuf = fx.env.cpd_home().join("policy/acme/cpd.toml");
    fs::write(&owner, "[pr]\nfeedback.checks = \"sometimes\"\n").unwrap();
    let error = refused(&show(&fx, &["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], real(&fx, "acme/owner.toml"));
    assert_eq!(error["details"]["key"], "pr.feedback.checks");
    fs::write(&owner, "[nonsense]\n").unwrap();
    assert_eq!(refused(&show(&fx, &["--owner", "acme"]), "policy_invalid", 1)["details"]["key"], "nonsense");
    fs::write(&owner, "not toml = = =").unwrap();
    assert_eq!(refused(&show(&fx, &["--owner", "acme"]), "policy_invalid", 1)["details"]["key"], Value::Null);
    // The repo file of a key is read only for that key.
    fs::write(&owner, "").unwrap();
    fs::write(&repo, "[code]\ntext = 1\n").unwrap();
    show(&fx, &["--owner", "acme"]).ok();
    let error = refused(&show(&fx, &["--owner", "acme", "--repo", "cpd"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], real(&fx, "acme/cpd.toml"));
}

#[test]
fn a_misnamed_policy_file_is_policy_invalid_with_its_path() {
    for (rel, invalid) in [("Clayton-Duarte/owner.toml", "Clayton-Duarte"), ("clayton_duarte/owner.toml", "clayton_duarte"), ("o/Bad Name.toml", "o/Bad Name.toml")] {
        let fx = world();
        let path = fx.env.cpd_home().join("policy").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        let error = refused(&show(&fx, &["--owner", "acme"]), "policy_invalid", 1);
        assert_eq!(error["details"]["file"], fx.env.cpd_home().join("policy").join(invalid).display().to_string(), "{rel}");
        assert_eq!(error["details"]["key"], Value::Null);
    }
}

#[test]
fn policy_show_lists_gate_merge_and_pr_with_sources() {
    let fx = world();
    let owner = format!("owner:{}", real(&fx, "acme/owner.toml"));
    let out = show(&fx, &["--owner", "acme"]).ok();
    let gate = &out["policy"]["gate"];
    assert_eq!(gate["audience"], json!(["lead", "worker"]), "{out}");
    assert_eq!(gate["keys"]["merge"], json!({"value": "local", "source": owner}), "{out}");
    assert_eq!(gate["keys"]["pr"], json!({"value": "local", "source": owner}), "{out}");
    // A repo file that sets `pr` only replaces the whole gate domain: `merge` is cpd's `none`, marked.
    let repo = format!("repo:{}", real(&fx, "acme/cpd.toml"));
    fs::write(fx.env.cpd_home().join("policy/acme/cpd.toml"), "[gate]\npr = \"local\"\n\n[code]\ntext = \"Repo code text.\"\n").unwrap();
    let out = show(&fx, &["--owner", "acme", "--repo", "cpd"]).ok();
    let gate = &out["policy"]["gate"];
    assert_eq!(gate["source"], repo, "{out}");
    assert_eq!(gate["keys"]["merge"], json!({"value": "none", "source": format!("{repo} (key default)")}), "{out}");
    assert_eq!(gate["keys"]["pr"], json!({"value": "local", "source": repo}), "{out}");
}

#[test]
fn it_writes_nothing() {
    let fx = world();
    let count = |sql: &str| -> i64 {
        let mut store = Store::open(&CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(), 5000).unwrap();
        store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    };
    let before = (count("SELECT count(*) FROM txns"), count("SELECT count(*) FROM events"));
    show(&fx, &["--owner", "acme"]).ok();
    show(&fx, &["--context", "ctx"]).ok();
    assert_eq!(before, (count("SELECT count(*) FROM txns"), count("SELECT count(*) FROM events")));
}
