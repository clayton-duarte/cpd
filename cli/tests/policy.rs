//! `cpd policy show` (commands.md §22.1): the built binary against a temporary
//! `$CPD_HOME`, with Contexts, Plans, PRs, Jobs, Attempts and Sessions written
//! straight into the database and policy files written by hand.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{refused, Run, TestEnv, CPD};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{json, Value};

const OWNER_FILE: &str = "[pr]\ngate = \"local\"\ntext = \"Owner pr text.\"\n\n[code]\ntext = \"Owner code text.\"\n";
const REPO_FILE: &str = "[code]\ntext = \"Repo code text.\"\n";

struct Fx {
    env: TestEnv,
}

impl Fx {
    /// Owner `acme` (a file with `[pr]` setting only `gate`, and `[code]`), the repo file
    /// `acme/cpd`, Context `ctx` (owner `acme`) with Plan `pl`, PR `named` (name `cpd`) and
    /// PR `anon` (no name), a Manager, a Lead and one Worker per PR; Context `bare` has no owner.
    fn new() -> Fx {
        let env = TestEnv::new();
        let home = env.cpd_home();
        fs::write(home.join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
        fs::write(home.join("policy/owners/acme.toml"), OWNER_FILE).unwrap();
        fs::create_dir_all(home.join("policy/repos/acme")).unwrap();
        fs::write(home.join("policy/repos/acme/cpd.toml"), REPO_FILE).unwrap();
        let fx = Fx { env };
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

    fn real(&self, rel: &str) -> String {
        fs::canonicalize(self.env.cpd_home().join("policy").join(rel)).unwrap().display().to_string()
    }

    fn show(&self, args: &[&str]) -> Run {
        let mut all = vec!["policy", "show"];
        all.extend(args);
        self.env.cpd(&all)
    }

    /// `policy show` as `session` (and `attempt`, as in a Worker's pane).
    fn show_as(&self, session: &str, attempt: Option<&str>) -> Run {
        let mut cmd = self.env.command(CPD);
        cmd.args(["policy", "show"]).env("CPD_SESSION", session);
        if let Some(attempt) = attempt {
            cmd.env("CPD_ATTEMPT", attempt);
        }
        let out = cmd.output().unwrap();
        Run {
            exit: out.status.code().unwrap(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

fn key(out: &Value) -> (&str, Option<&str>) {
    (out["key"]["owner"].as_str().unwrap(), out["key"]["repo"].as_str())
}

fn files(fx: &Fx, rels: &[&str]) -> Value {
    json!(rels.iter().map(|r| fx.real(r)).collect::<Vec<_>>())
}

#[test]
fn the_output_has_exactly_the_documented_fields() {
    let fx = Fx::new();
    let out = fx.show(&["--owner", "acme"]).ok();
    let mut top: Vec<&str> = out.as_object().unwrap().keys().map(String::as_str).collect();
    top.sort_unstable();
    assert_eq!(top, ["files", "key", "ok", "policy", "role"]);
    assert_eq!(out["role"], Value::Null);
    let mut domains: Vec<&str> = out["policy"].as_object().unwrap().keys().map(String::as_str).collect();
    domains.sort_unstable();
    assert_eq!(domains, ["code", "deploy", "instructions", "monitor", "pr", "tests"]);
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
    assert_eq!(keys, ["feedback.checks", "gate"]);
    for k in out["policy"]["pr"]["keys"].as_object().unwrap().values() {
        let mut fields: Vec<&str> = k.as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["source", "value"]);
    }
}

#[test]
fn owner_key_reads_the_owner_file_and_marks_the_key_default() {
    let fx = Fx::new();
    let out = fx.show(&["--owner", "Acme"]).ok();
    assert_eq!(key(&out), ("acme", None));
    assert_eq!(out["files"], files(&fx, &["owners/acme.toml"]));
    let owner = format!("owner:{}", fx.real("owners/acme.toml"));
    let pr = &out["policy"]["pr"];
    assert_eq!(pr["source"], owner);
    assert_eq!(pr["text"], "Owner pr text.");
    assert_eq!(pr["keys"]["gate"], json!({"value": "local", "source": owner}));
    // `feedback.checks` is left out of the set domain: cpd's default, marked.
    assert_eq!(pr["keys"]["feedback.checks"], json!({"value": "any", "source": format!("{owner} (key default)")}));
    assert_eq!(out["policy"]["code"]["source"], owner);
    assert_eq!(out["policy"]["deploy"], json!({"source": "default", "audience": ["lead"], "keys": {}, "text": ""}));
}

#[test]
fn a_repo_file_overrides_one_domain() {
    let fx = Fx::new();
    let out = fx.show(&["--owner", "acme", "--repo", "cpd"]).ok();
    assert_eq!(key(&out), ("acme", Some("cpd")));
    assert_eq!(out["files"], files(&fx, &["owners/acme.toml", "repos/acme/cpd.toml"]));
    assert_eq!(out["policy"]["code"]["source"], format!("repo:{}", fx.real("repos/acme/cpd.toml")));
    assert_eq!(out["policy"]["code"]["text"], "Repo code text.");
    assert_eq!(out["policy"]["pr"]["source"], format!("owner:{}", fx.real("owners/acme.toml")));
    // A repo without a file reads the owner's.
    let other = fx.show(&["--owner", "acme", "--repo", "other"]).ok();
    assert_eq!(other["files"], files(&fx, &["owners/acme.toml"]));
    assert_eq!(other["policy"]["code"]["text"], "Owner code text.");
}

#[test]
fn an_empty_owner_file_is_cpds_default_without_key_default_marks() {
    let fx = Fx::new();
    fs::write(fx.env.cpd_home().join("policy/owners/acme.toml"), "").unwrap();
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["policy"]["pr"]["source"], "default");
    assert_eq!(out["policy"]["pr"]["keys"]["gate"], json!({"value": "ci", "source": "default"}));
    // A bare `[pr]` table sets the whole domain to cpd's default: every key is defaulted.
    fs::write(fx.env.cpd_home().join("policy/owners/acme.toml"), "[pr]\n").unwrap();
    let owner = format!("owner:{}", fx.real("owners/acme.toml"));
    let out = fx.show(&["--owner", "acme"]).ok();
    assert_eq!(out["policy"]["pr"]["keys"]["gate"], json!({"value": "ci", "source": format!("{owner} (key default)")}));
    assert_eq!(out["policy"]["pr"]["keys"]["feedback.checks"]["source"], format!("{owner} (key default)"));
}

#[test]
fn context_selects_its_owner() {
    let fx = Fx::new();
    let out = fx.show(&["--context", "ctx"]).ok();
    assert_eq!(key(&out), ("acme", None));
    assert_eq!(out["role"], Value::Null);
}

#[test]
fn pr_selects_its_repo_or_the_owner_when_unnamed() {
    let fx = Fx::new();
    let named = fx.show(&["--pr", "named", "--plan", "pl"]).ok();
    assert_eq!(key(&named), ("acme", Some("cpd")));
    assert_eq!(named["policy"]["code"]["text"], "Repo code text.");
    let anon = fx.show(&["--pr", "anon", "--plan", "pl"]).ok();
    assert_eq!(key(&anon), ("acme", None));
    // The Plan may come from CPD_PLAN.
    let out = fx.env.command(CPD).args(["policy", "show", "--pr", "named"]).env("CPD_PLAN", "pl").output().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["key"]["repo"], "cpd");
}

#[test]
fn no_flag_resolves_the_callers_session() {
    let fx = Fx::new();
    for (session, role) in [("mgr1", "manager"), ("lead1", "lead")] {
        let out = fx.show_as(session, None).ok();
        assert_eq!((key(&out), out["role"].as_str()), (("acme", None), Some(role)), "{session}");
    }
    let worker = fx.show_as("w-a", Some("a-a1")).ok();
    assert_eq!((key(&worker), worker["role"].as_str()), (("acme", Some("cpd")), Some("worker")));
    assert_eq!(worker["policy"]["code"]["text"], "Repo code text.");
    let anon = fx.show_as("w-b", Some("b-a1")).ok();
    assert_eq!(key(&anon), ("acme", None));
}

#[test]
fn usage_refusals() {
    let fx = Fx::new();
    refused(&fx.show(&[]), "session_required", 2);
    refused(&fx.show(&["--pr", "named", "--context", "ctx"]), "usage", 2);
    refused(&fx.show(&["--context", "ctx", "--owner", "acme"]), "usage", 2);
    refused(&fx.show(&["--pr", "named", "--owner", "acme"]), "usage", 2);
    refused(&fx.show(&["--repo", "cpd"]), "usage", 2);
    refused(&fx.show(&["--context", "ctx", "--repo", "cpd"]), "usage", 2);
    refused(&fx.show(&["--owner", "-bad"]), "usage", 2);
    refused(&fx.show(&["--owner", "acme", "--repo", "a b"]), "usage", 2);
    refused(&fx.show(&["--pr", "named"]), "plan_required", 2);
}

#[test]
fn not_found_refusals() {
    let fx = Fx::new();
    refused(&fx.show_as("nope", None), "session_not_found", 4);
    refused(&fx.show(&["--pr", "named", "--plan", "zz"]), "plan_not_found", 4);
    refused(&fx.show(&["--pr", "zz", "--plan", "pl"]), "pr_not_found", 4);
    refused(&fx.show(&["--context", "zz"]), "context_not_found", 4);
}

#[test]
fn owner_unset_names_the_next_command() {
    let fx = Fx::new();
    let error = refused(&fx.show(&["--context", "bare"]), "owner_unset", 3);
    assert!(error["details"].get("next").is_none() && error["message"].as_str().unwrap().contains("cpd context owner <login> --context bare"), "{error}");
    refused(&fx.show_as("mgr2", None), "owner_unset", 3);
}

#[test]
fn owner_unknown_even_with_a_repo_file() {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    fs::create_dir_all(home.join("policy/repos/ghost")).unwrap();
    fs::write(home.join("policy/repos/ghost/x.toml"), REPO_FILE).unwrap();
    let error = refused(&fx.show(&["--owner", "ghost", "--repo", "x"]), "owner_unknown", 1);
    assert_eq!(error["details"]["owner"], "ghost");
    refused(&fx.show(&["--owner", "ghost"]), "owner_unknown", 1);
}

#[test]
fn policy_invalid_names_the_file_and_key() {
    let fx = Fx::new();
    let owner: PathBuf = fx.env.cpd_home().join("policy/owners/acme.toml");
    let repo: PathBuf = fx.env.cpd_home().join("policy/repos/acme/cpd.toml");
    fs::write(&owner, "[pr]\ngate = \"sometimes\"\n").unwrap();
    let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("owners/acme.toml"));
    assert_eq!(error["details"]["key"], "pr.gate");
    fs::write(&owner, "[nonsense]\n").unwrap();
    assert_eq!(refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1)["details"]["key"], "nonsense");
    fs::write(&owner, "not toml = = =").unwrap();
    assert_eq!(refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1)["details"]["key"], Value::Null);
    // The repo file of a key is read only for that key.
    fs::write(&owner, "").unwrap();
    fs::write(&repo, "[code]\ntext = 1\n").unwrap();
    fx.show(&["--owner", "acme"]).ok();
    let error = refused(&fx.show(&["--owner", "acme", "--repo", "cpd"]), "policy_invalid", 1);
    assert_eq!(error["details"]["file"], fx.real("repos/acme/cpd.toml"));
}

#[test]
fn a_misnamed_policy_file_is_policy_invalid_with_its_path() {
    for rel in ["owners/Clayton-Duarte.toml", "owners/clayton_duarte.toml", "repos/o/Bad Name.toml"] {
        let fx = Fx::new();
        let path = fx.env.cpd_home().join("policy").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        let error = refused(&fx.show(&["--owner", "acme"]), "policy_invalid", 1);
        assert_eq!(error["details"]["file"], path.display().to_string(), "{rel}");
        assert_eq!(error["details"]["key"], Value::Null);
    }
}

#[test]
fn it_writes_nothing() {
    let fx = Fx::new();
    let count = |sql: &str| -> i64 {
        let mut store = Store::open(&CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(), 5000).unwrap();
        store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    };
    let before = (count("SELECT count(*) FROM txns"), count("SELECT count(*) FROM events"));
    fx.show(&["--owner", "acme"]).ok();
    fx.show(&["--context", "ctx"]).ok();
    assert_eq!(before, (count("SELECT count(*) FROM txns"), count("SELECT count(*) FROM events")));
}
