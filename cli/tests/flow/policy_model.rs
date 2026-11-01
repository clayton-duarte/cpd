//! The `model` policy domain (contract §8.3 "Models", §9; commands.md §14.3, §22.1): the four Tier models
//! `O`, `S`, `H`, `G`, one provider per resolved table, and `cpd tiers`'s key selectors. The built binary's
//! command tree, in this process (`Fx`), against a temporary `$CPD_HOME`.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, refused, write_executable};
use serde_json::{Value, json};

struct Models {
    fx: Fx,
}

impl Models {
    /// Machine `pro`; owner `acme` and repos `acme/r`, `acme/r1`, `acme/r2` with an empty file each.
    fn new() -> Models {
        let fx = Fx::with_ports();
        fx.env.write_config("version = 1\nmachine = \"pro\"\n");
        fs::remove_dir_all(fx.env.cpd_home().join("policy")).unwrap();
        fs::create_dir_all(fx.env.cpd_home().join("policy")).unwrap();
        let m = Models { fx };
        for rel in [
            "acme/owner.toml",
            "acme/r.toml",
            "acme/r1.toml",
            "acme/r2.toml",
        ] {
            m.write(rel, "");
        }
        m
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.fx.env.cpd_home().join("policy").join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn real(&self, rel: &str) -> String {
        fs::canonicalize(self.path(rel))
            .unwrap()
            .display()
            .to_string()
    }

    fn show(&self, args: &[&str]) -> Run {
        let mut all = vec!["policy", "show"];
        all.extend(args);
        self.fx.cpd(&all)
    }

    fn owner(&self) -> Run {
        self.show(&["--owner", "acme"])
    }

    fn repo(&self) -> Run {
        self.show(&["--owner", "acme", "--repo", "r"])
    }
}

const LETTERS: [&str; 4] = ["O", "S", "H", "G"];

/// A `[model]` table whose four refs are `github-copilot/<prefix>-<letter>`.
fn table(prefix: &str) -> String {
    let lines: Vec<String> = LETTERS
        .iter()
        .map(|l| format!("{l} = \"github-copilot/{prefix}-{}\"", l.to_lowercase()))
        .collect();
    format!("[model]\n{}\n", lines.join("\n"))
}

fn key<'a>(out: &'a Value, letter: &str) -> &'a Value {
    &out["policy"]["model"]["keys"][letter]
}

fn assert_all(out: &Value, prefix: &str, source: &str) {
    for l in LETTERS {
        let k = key(out, l);
        assert_eq!(
            k["value"],
            format!("github-copilot/{prefix}-{}", l.to_lowercase()),
            "{l}: {out}"
        );
        assert_eq!(k["source"], source, "{l}: {out}");
    }
}

// ------------------------------------------------------------------ m1: the built-in default

#[test]
fn m1_default_models_are_todays_tiers() {
    let fx = Models::new();
    for out in [fx.owner().ok(), fx.repo().ok()] {
        for (letter, model) in [
            ("O", "github-copilot/claude-opus-5.5"),
            ("S", "github-copilot/claude-sonnet-5.5"),
            ("H", "github-copilot/claude-haiku-4.5"),
            ("G", "github-copilot/gemini-3.8-flash"),
        ] {
            assert_eq!(
                key(&out, letter),
                &json!({"value": model, "source": "default"}),
                "{out}"
            );
        }
    }
}

#[test]
fn m1_the_model_domain_has_no_text() {
    let fx = Models::new();
    let out = fx.owner().ok();
    assert!(out["policy"]["model"].is_object(), "{out}");
    // Like the repo-only domains (commands.md §22.1): `text` is `""` (or not printed), never a text.
    let text = out["policy"]["model"]
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert_eq!(text, "", "{out}");
    assert_eq!(out["policy"]["model"]["audience"], json!([]), "{out}");
}

// ------------------------------------------------------------------ m2: precedence

#[test]
fn m2_the_machine_sets_the_models() {
    let fx = Models::new();
    fx.write("host.toml", &table("machine"));
    assert_all(
        &fx.owner().ok(),
        "machine",
        &format!("machine:{}", fx.real("host.toml")),
    );
}

#[test]
fn m2_the_owner_wins_over_the_machine() {
    let fx = Models::new();
    fx.write("host.toml", &table("machine"));
    fx.write("acme/owner.toml", &table("owner"));
    assert_all(
        &fx.owner().ok(),
        "owner",
        &format!("owner:{}", fx.real("acme/owner.toml")),
    );
}

#[test]
fn m2_the_repo_wins_over_the_owner_and_the_machine() {
    let fx = Models::new();
    fx.write("host.toml", &table("machine"));
    fx.write("acme/owner.toml", &table("owner"));
    fx.write("acme/r.toml", &table("repo"));
    assert_all(
        &fx.repo().ok(),
        "repo",
        &format!("repo:{}", fx.real("acme/r.toml")),
    );
    // The owner's own key is untouched by the repo file.
    assert_all(
        &fx.owner().ok(),
        "owner",
        &format!("owner:{}", fx.real("acme/owner.toml")),
    );
}

// ------------------------------------------------------------------ m3: whole-domain replacement

#[test]
fn m3_the_owner_replaces_the_machine_whole_and_the_rest_is_the_default() {
    let fx = Models::new();
    fx.write("host.toml", &table("mach"));
    fx.write("acme/owner.toml", "[model]\nO = \"github-copilot/own\"\n");
    let out = fx.owner().ok();
    let source = format!("owner:{}", fx.real("acme/owner.toml"));
    assert_eq!(
        key(&out, "O"),
        &json!({"value": "github-copilot/own", "source": source}),
        "{out}"
    );
    for (letter, model) in [
        ("S", "github-copilot/claude-sonnet-5.5"),
        ("H", "github-copilot/claude-haiku-4.5"),
        ("G", "github-copilot/gemini-3.8-flash"),
    ] {
        assert_eq!(
            key(&out, letter),
            &json!({"value": model, "source": format!("{source} (key default)")}),
            "{out}"
        );
    }
}

// ------------------------------------------------------------------ m4: one provider per table

const BIFROST: &str = "[model]\nO = \"bifrost/vertex/o\"\nS = \"bifrost/vertex/s\"\nH = \"bifrost/vertex/h\"\nG = \"bifrost/vertex/g\"\n";

fn assert_mixed(error: &Value, file: &str) {
    assert_eq!(error["details"]["file"], file, "{error}");
    assert_eq!(error["details"]["key"], "model", "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("bifrost") && message.contains("github-copilot"),
        "the message names the providers: {error}"
    );
}

#[test]
fn m4_an_owner_file_mixing_providers_with_the_default_is_policy_invalid() {
    let fx = Models::new();
    fx.write("acme/owner.toml", "[model]\nS = \"bifrost/vertex/s\"\n");
    let error = refused(&fx.owner(), "policy_invalid", 1);
    assert_mixed(&error, &fx.real("acme/owner.toml"));
    let message = error["message"].as_str().unwrap();
    for word in ["S", "O", "bifrost", "github-copilot"] {
        assert!(message.contains(word), "{error}");
    }
}

#[test]
fn m4_four_refs_of_one_other_provider_are_accepted() {
    let fx = Models::new();
    fx.write("acme/owner.toml", BIFROST);
    let out = fx.owner().ok();
    for (letter, model) in [("O", "o"), ("S", "s"), ("H", "h"), ("G", "g")] {
        assert_eq!(
            key(&out, letter)["value"],
            format!("bifrost/vertex/{model}"),
            "{out}"
        );
    }
}

#[test]
fn m4_a_machine_file_with_two_providers_is_refused_naming_that_file() {
    let fx = Models::new();
    fx.write("host.toml", "[model]\nO = \"github-copilot/o\"\nS = \"github-copilot/s\"\nH = \"bifrost/h\"\nG = \"github-copilot/g\"\n");
    let error = refused(&fx.owner(), "policy_invalid", 1);
    assert_mixed(&error, &fx.real("host.toml"));
}

#[test]
fn m4_a_repo_file_with_two_providers_is_refused_naming_that_file() {
    let fx = Models::new();
    fx.write("acme/r.toml", "[model]\nG = \"bifrost/g\"\n");
    let error = refused(&fx.repo(), "policy_invalid", 1);
    assert_mixed(&error, &fx.real("acme/r.toml"));
}

// ------------------------------------------------------------------ m5: shape

#[test]
fn m5_a_badly_shaped_model_is_policy_invalid_naming_its_key() {
    for (line, key) in [
        ("O = \"no-slash\"", "model.O"),
        ("S = \"/model\"", "model.S"),
        ("H = \"provider/\"", "model.H"),
        ("O = \"pro vider/model\"", "model.O"),
        ("G = 3", "model.G"),
        ("o = \"p/m\"", "model.o"),
        ("X = \"p/m\"", "model.X"),
    ] {
        let fx = Models::new();
        fx.write("acme/owner.toml", &format!("[model]\n{line}\n"));
        let error = refused(&fx.owner(), "policy_invalid", 1);
        assert_eq!(
            error["details"]["file"],
            fx.real("acme/owner.toml"),
            "{line}: {error}"
        );
        assert_eq!(error["details"]["key"], key, "{line}: {error}");
    }
}

// ------------------------------------------------------------------ m6: exposure and keys

#[test]
fn m6_model_is_accepted_in_a_machine_an_owner_and_a_repo_file() {
    for rel in ["host.toml", "acme/owner.toml", "acme/r.toml"] {
        let fx = Models::new();
        fx.write(rel, &table("x"));
        fx.repo().ok();
    }
}

#[test]
fn m6_text_and_provider_are_not_model_keys() {
    for (line, key) in [
        ("text = \"x\"", "model.text"),
        ("provider = \"github-copilot\"", "model.provider"),
    ] {
        let fx = Models::new();
        fx.write("acme/owner.toml", &format!("[model]\n{line}\n"));
        let error = refused(&fx.owner(), "policy_invalid", 1);
        assert_eq!(
            error["details"]["file"],
            fx.real("acme/owner.toml"),
            "{line}: {error}"
        );
        assert_eq!(error["details"]["key"], key, "{line}: {error}");
    }
}

// ------------------------------------------------------------------ m7: `cpd tiers` selectors

const VERSION: &str = "1.0.0";

/// A fake pi install (`bin/pi`, `install/current-version`) and `tier_catalog` rows for every ref of the tables in
/// `prefixes` (efforts by position: O all, S `[0,1]`, H `[0,1,2]`), so no `node` runs.
fn cache(fx: &Fx, prefixes: &[&str]) {
    write_executable(&fx.env.bin().join("pi"), "");
    fs::create_dir_all(fx.env.root().join("install")).unwrap();
    fs::write(
        fx.env.root().join("install/current-version"),
        format!("{VERSION}\n"),
    )
    .unwrap();
    let scope = fx
        .env
        .root()
        .join("install/releases")
        .join(VERSION)
        .join("node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(
            scope.join(package).join("package.json"),
            "{\"type\": \"module\"}",
        )
        .unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows: String = prefixes
        .iter()
        .flat_map(|p| [("O", "[0,1,2,3,4,5]"), ("S", "[0,1]"), ("H", "[0,1,2]")].map(|(l, e)| (p, l, e)))
        .map(|(p, letter, efforts)| {
            let model = format!("github-copilot/{p}-{}", letter.to_lowercase());
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('{VERSION}', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.env.sql(&rows);
}

/// Machine, owner, `r1` and `r2` each set a table; Context `ctx` (owner `acme`), Plan `pl`, PRs `p1` (`r1`), `p2` (`r2`).
fn tiers_world() -> Models {
    let fx = Models::new();
    fx.write("host.toml", &table("mach"));
    fx.write("acme/owner.toml", &table("own"));
    fx.write("acme/r1.toml", &table("one"));
    fx.write("acme/r2.toml", &table("two"));
    cache(&fx.fx, &["mach", "own", "one", "two"]);
    fx.fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', 'acme', 't', 'pro');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, name, created_txn, created_at)
           VALUES ('pl', 'p1', '/r', 'main', 'waiting', 'cpd/pl/p1', 'r1', cpd_txn(), 't'),
                  ('pl', 'p2', '/r', 'main', 'waiting', 'cpd/pl/p2', 'r2', cpd_txn(), 't');",
    );
    fx
}

fn tiers(fx: &Models, args: &[&str]) -> Run {
    let mut all = vec!["tiers"];
    all.extend(args);
    fx.fx.cpd(&all)
}

/// `out` prints the table `prefix` with the cached efforts, and its `key` is `owner`/`repo`.
fn assert_tiers(out: &Value, prefix: &str, owner: Option<&str>, repo: Option<&str>) {
    assert_eq!(out["key"]["owner"].as_str(), owner, "{out}");
    assert_eq!(out["key"]["repo"].as_str(), repo, "{out}");
    assert_tiers_models(out, prefix);
}

fn assert_tiers_models(out: &Value, prefix: &str) {
    for (letter, efforts) in [
        ("O", json!([0, 1, 2, 3, 4, 5])),
        ("S", json!([0, 1])),
        ("H", json!([0, 1, 2])),
        ("G", json!([])),
    ] {
        let model = format!("github-copilot/{prefix}-{}", letter.to_lowercase());
        assert_eq!(
            out["tiers"][letter],
            json!({"model": model, "efforts": efforts}),
            "{letter}: {out}"
        );
    }
}

#[test]
fn m7_owner_and_repo_select_the_repos_models() {
    let fx = tiers_world();
    assert_tiers(
        &tiers(&fx, &["--owner", "acme", "--repo", "r1"]).ok(),
        "one",
        Some("acme"),
        Some("r1"),
    );
    assert_tiers(
        &tiers(&fx, &["--owner", "acme", "--repo", "r2"]).ok(),
        "two",
        Some("acme"),
        Some("r2"),
    );
}

#[test]
fn m7_pr_selects_its_repo() {
    let fx = tiers_world();
    assert_tiers(
        &tiers(&fx, &["--workflow", "p1", "--plan", "pl"]).ok(),
        "one",
        Some("acme"),
        Some("r1"),
    );
    assert_tiers(
        &tiers(&fx, &["--workflow", "p2", "--plan", "pl"]).ok(),
        "two",
        Some("acme"),
        Some("r2"),
    );
}

#[test]
fn m7_context_selects_its_owner() {
    let fx = tiers_world();
    assert_tiers(
        &tiers(&fx, &["--context", "ctx"]).ok(),
        "own",
        Some("acme"),
        None,
    );
}

#[test]
fn m7_owner_selects_the_owners_models() {
    let fx = tiers_world();
    assert_tiers(
        &tiers(&fx, &["--owner", "acme"]).ok(),
        "own",
        Some("acme"),
        None,
    );
}

#[test]
fn m7_no_selector_is_the_machine_layer() {
    let fx = tiers_world();
    let out = tiers(&fx, &[]).ok();
    // The documents state no `key` for the machine layer: only the models are pinned.
    assert_tiers_models(&out, "mach");
}

#[test]
fn m7_repo_without_owner_is_usage() {
    let fx = tiers_world();
    tiers(&fx, &["--owner", "acme", "--repo", "r1"]).ok();
    refused(&tiers(&fx, &["--repo", "r1"]), "usage", 2);
}
