//! `cpd policy show` lists the `resources` domain (contract §8.3 "The `resources` domain").

use crate::common;

use std::fs;

use common::fx::Fx;
use serde_json::json;

/// Owner `acme` (an empty file), Context `ctx`, Plan `pl` and PR `named` of repo `acme/cpd`.
fn world(repo_file: &str) -> Fx {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    fs::write(home.join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fs::write(home.join("policy/acme/owner.toml"), "").unwrap();
    fs::write(home.join("policy/acme/cpd.toml"), repo_file).unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', 'acme', 't', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, name, created_txn, created_at)
           VALUES ('pl', 'named', '/r', 'main', 'waiting', 'cpd/pl/named', 'cpd', cpd_txn(), 't');",
    );
    fx
}

#[test]
fn resource_decl_policy_show_lists_the_declared_names_sorted() {
    let fx = world("[resources.e2e-port]\n\n[resources.db]\n");
    let repo_file = fs::canonicalize(fx.env.cpd_home().join("policy/acme/cpd.toml"))
        .unwrap()
        .display()
        .to_string();
    let out = fx
        .cpd(&["policy", "show", "--pr", "named", "--plan", "pl"])
        .ok();
    let domain = &out["policy"]["resources"];
    assert_eq!(
        domain["keys"]["names"],
        json!({"value": ["db", "e2e-port"], "source": format!("repo:{repo_file}")})
    );
    assert_eq!(domain["source"], format!("repo:{repo_file}"));
    assert_eq!(domain["text"], "");
    assert_eq!(domain["audience"], json!([]));

    fs::write(fx.env.cpd_home().join("policy/acme/cpd.toml"), "").unwrap();
    let out = fx
        .cpd(&["policy", "show", "--pr", "named", "--plan", "pl"])
        .ok();
    assert_eq!(
        out["policy"]["resources"]["keys"]["names"],
        json!({"value": [], "source": "default"})
    );
}

#[test]
fn policy_show_resources_lists_names_with_scope_slots_and_source() {
    let fx = world("[resources.staging]\nslots = 3\n\n[resources.okta-port]\n");
    let home = fx.env.cpd_home();
    fs::write(
        home.join("policy/host.toml"),
        "[resources.shared-port]\nslots = 2\n",
    )
    .unwrap();
    fs::write(home.join("policy/acme/owner.toml"), "[resources.staging]\n").unwrap();
    let real = |rel: &str| {
        fs::canonicalize(home.join(rel))
            .unwrap()
            .display()
            .to_string()
    };
    let (host, repo) = (real("policy/host.toml"), real("policy/acme/cpd.toml"));

    let out = fx
        .cpd(&["policy", "show", "--pr", "named", "--plan", "pl"])
        .ok();

    let domain = &out["policy"]["resources"];
    assert_eq!(
        domain["keys"]["names"],
        json!({"value": ["okta-port", "shared-port", "staging"], "source": format!("repo:{repo}")})
    );
    assert_eq!(
        domain["keys"]["declared"],
        json!({
            "value": [
                {"name": "okta-port", "scope": "repo", "slots": 1, "source": repo},
                {"name": "shared-port", "scope": "host", "slots": 2, "source": host},
                {"name": "staging", "scope": "repo", "slots": 3, "source": repo},
            ],
            "source": format!("repo:{repo}")
        })
    );
    assert_eq!(domain["source"], format!("repo:{repo}"));
}
