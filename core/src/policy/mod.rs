//! Policies (contract §8.3): typed domains with cpd's compiled default, owner
//! and repo scope files, and one entry point, [`resolve`], per policy key.
//!
//! - [`domain`]: the domains, [`Policy`], the audience table;
//! - [`load`]: parsing and validating one scope file;
//! - [`resolve`]: the default, the owner file, the repo file, per domain.

mod domain;
mod load;
mod resolve;

use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, OwnerLogin, RepoName};
use crate::session::Role;

pub use domain::{
    audience, Checks, Domain, DomainSource, Feedback, Gate, Policy, Pr, Scope, Source, Sources, Text,
};
pub use load::{load_scope_file, parse_scope, policy_invalid};
pub use resolve::{default_policy, key_of_scope_file, resolve, resolve_unowned_row};

/// What a policy is resolved for (contract §8.3 "Model"): never a machine, a path or a cwd.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PolicyKey {
    Owner(OwnerLogin),
    Repo(OwnerLogin, RepoName),
}

impl PolicyKey {
    pub fn owner(&self) -> &OwnerLogin {
        match self {
            PolicyKey::Owner(o) | PolicyKey::Repo(o, _) => o,
        }
    }
}

impl std::fmt::Display for PolicyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyKey::Owner(o) => write!(f, "{o}"),
            PolicyKey::Repo(o, n) => write!(f, "{o}/{n}"),
        }
    }
}

/// A resolved policy: every domain's value with its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub policy: Policy,
    pub sources: Sources,
}

impl Resolved {
    /// The `(domain, source, text)` of every non-empty `text` that reaches
    /// `role`, in the contract's domain order (`policy show` and the adapter).
    pub fn texts_for(&self, role: Role) -> Vec<(Domain, &DomainSource, &str)> {
        Domain::ALL
            .iter()
            .copied()
            .filter(|d| audience(*d).contains(&role))
            .map(|d| (d, self.sources.get(d), self.policy.text(d)))
            .filter(|(_, _, text)| !text.is_empty())
            .collect()
    }
}

/// `owner_unknown` (exit 1): the key's owner has no `owners/<owner>.toml`.
pub fn owner_unknown(owner: &OwnerLogin) -> CpdError {
    CpdError::new(
        ErrorCode::OwnerUnknown,
        format!("The owner {owner} has no policy file: create policy/owners/{owner}.toml (empty means cpd's default)."),
    )
    .with("owner", owner.as_str())
}

/// `repo_unkeyed` (exit 1): the repo cannot be keyed from `origin`; `url` is `None` with no `origin`.
pub fn repo_unkeyed(url: Option<&str>) -> CpdError {
    CpdError::new(
        ErrorCode::RepoUnkeyed,
        "The repo cannot be keyed: its origin must be https://github.com/<owner>/<name> or git@github.com:<owner>/<name>.",
    )
    .with("url", url.map_or(serde_json::Value::Null, |u| serde_json::Value::String(u.to_owned())))
}

/// `owner_mismatch` (exit 3): the repo's owner is not the Context's.
pub fn owner_mismatch(owner: &OwnerLogin, repo_owner: &OwnerLogin) -> CpdError {
    CpdError::new(
        ErrorCode::OwnerMismatch,
        format!("The repo belongs to {repo_owner}, but the Context's owner is {owner}."),
    )
    .with("owner", owner.as_str())
    .with("repo_owner", repo_owner.as_str())
}

/// `owner_unset` (exit 3): the Context has no owner yet (made before the policy keys).
pub fn owner_unset(context: &ContextId) -> CpdError {
    CpdError::new(ErrorCode::OwnerUnset, format!("The Context {context} has no owner."))
        .with("context", context.as_str())
        .with("next", format!("cpd context owner <login> --context {context}"))
}

/// `owner_set` (exit 3): `context owner` on a Context that has an owner.
pub fn owner_set(context: &ContextId, owner: &OwnerLogin) -> CpdError {
    CpdError::new(ErrorCode::OwnerSet, format!("The Context {context} already has the owner {owner}."))
        .with("context", context.as_str())
        .with("owner", owner.as_str())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use std::path::Path;

    fn home() -> (tempfile::TempDir, CpdHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        (dir, home)
    }

    fn write(home: &CpdHome, rel: &str, text: &str) {
        let path = home.root().join("policy").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn owner(s: &str) -> OwnerLogin {
        OwnerLogin::parse(s).unwrap()
    }

    fn repo_key(o: &str, n: &str) -> PolicyKey {
        PolicyKey::Repo(owner(o), RepoName::parse(n).unwrap())
    }

    fn real(home: &CpdHome, rel: &str) -> std::path::PathBuf {
        std::fs::canonicalize(home.root().join("policy").join(rel)).unwrap()
    }

    #[test]
    fn default_sets_every_domain_with_ci_and_any() {
        let d = default_policy();
        assert_eq!(d.pr.gate, Gate::Ci);
        assert_eq!(d.pr.feedback.checks, Checks::Any);
        for domain in Domain::ALL {
            assert_eq!(d.text(*domain), "");
        }
    }

    #[test]
    fn empty_owner_file_is_default_everywhere() {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", "");
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(&r.policy, default_policy());
        for d in Domain::ALL {
            assert_eq!(r.sources.get(*d), &DomainSource { source: Source::Default, key_default: false });
        }
    }

    #[test]
    fn precedence_is_repo_over_owner_over_default_per_domain() {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", "[code]\ntext = \"owner code\"\n[tests]\ntext = \"owner tests\"\n");
        write(&home, "repos/acme/cpd.toml", "[tests]\ntext = \"repo tests\"\n");
        let r = resolve(&home, &repo_key("Acme", "CPD")).unwrap();
        assert_eq!(r.policy.code.text, "owner code");
        assert_eq!(r.sources.get(Domain::Code).source, Source::Owner(real(&home, "owners/acme.toml")));
        assert_eq!(r.policy.tests.text, "repo tests");
        assert_eq!(r.sources.get(Domain::Tests).source, Source::Repo(real(&home, "repos/acme/cpd.toml")));
        assert_eq!(r.policy.deploy.text, "");
        assert_eq!(r.sources.get(Domain::Deploy).source, Source::Default);
        // The owner key resolves without the repo file.
        let o = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(o.policy.tests.text, "owner tests");
    }

    #[test]
    fn a_repo_without_a_file_resolves_from_its_owner() {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", "[code]\ntext = \"c\"\n");
        let r = resolve(&home, &repo_key("acme", "other")).unwrap();
        assert_eq!(r.policy.code.text, "c");
    }

    #[test]
    fn a_set_domain_replaces_the_whole_domain_and_left_out_keys_take_cpds_default() {
        let (_d, home) = home();
        write(
            &home,
            "owners/acme.toml",
            "[pr]\ngate = \"local\"\nfeedback.checks = \"none\"\ntext = \"owner pr\"\n",
        );
        write(&home, "repos/acme/cpd.toml", "[pr]\ngate = \"ci\"\n");
        let r = resolve(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.pr.gate, Gate::Ci);
        assert_eq!(r.policy.pr.feedback.checks, Checks::Any, "cpd's default, not the owner's none");
        assert_eq!(r.policy.pr.text, "", "the text is replaced, not inherited");
        let src = r.sources.get(Domain::Pr);
        assert_eq!(src.source, Source::Repo(real(&home, "repos/acme/cpd.toml")));
        assert!(src.key_default);
        assert!(src.to_string().ends_with(" (key default)"));
        // The owner file sets both keys: no `(key default)`.
        let o = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(o.policy.pr.gate, Gate::Local);
        assert_eq!(o.policy.pr.feedback.checks, Checks::None);
        assert!(!o.sources.get(Domain::Pr).key_default);
    }

    #[test]
    fn an_empty_table_is_cpds_default_for_the_whole_domain() {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", "[pr]\ngate = \"local\"\ntext = \"t\"\n");
        write(&home, "repos/acme/cpd.toml", "[pr]\n");
        let r = resolve(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.pr, default_policy().pr);
        assert!(matches!(r.sources.get(Domain::Pr).source, Source::Repo(_)));
        assert!(r.sources.get(Domain::Pr).key_default);
    }

    fn invalid(text: &str) -> CpdError {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", text);
        let err = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyInvalid);
        assert_eq!(err.exit().code(), 1);
        assert!(err.details()["file"].as_str().unwrap().ends_with("owners/acme.toml"));
        err
    }

    #[test]
    fn invalid_files_are_policy_invalid_with_file_and_key() {
        let key = |text: &str| invalid(text).details()["key"].clone();
        assert_eq!(key("[mcp]\n"), serde_json::json!("mcp"));
        assert_eq!(key("[hooks]\ntext = \"x\"\n"), serde_json::json!("hooks"));
        assert_eq!(key("[bogus]\n"), serde_json::json!("bogus"));
        assert_eq!(key("[pr]\nbogus = 1\n"), serde_json::json!("pr.bogus"));
        assert_eq!(key("[pr.feedback]\nbogus = 1\n"), serde_json::json!("pr.feedback.bogus"));
        assert_eq!(key("[pr]\ngate = \"maybe\"\n"), serde_json::json!("pr.gate"));
        assert_eq!(key("[pr]\nfeedback.checks = \"some\"\n"), serde_json::json!("pr.feedback.checks"));
        assert_eq!(key("[code]\ntext = 5\n"), serde_json::json!("code.text"));
        assert_eq!(key("[deploy]\ngate = \"ci\"\n"), serde_json::json!("deploy.gate"));
        assert_eq!(key("pr = 5\n"), serde_json::json!("pr"));
        assert_eq!(key("not toml ["), serde_json::Value::Null);
    }

    #[test]
    fn an_invalid_repo_file_names_its_own_path() {
        let (_d, home) = home();
        write(&home, "owners/acme.toml", "");
        write(&home, "repos/acme/cpd.toml", "[pr]\ngate = 1\n");
        let err = resolve(&home, &repo_key("acme", "cpd")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyInvalid);
        assert!(err.details()["file"].as_str().unwrap().ends_with("repos/acme/cpd.toml"));
    }

    #[test]
    fn a_file_name_that_is_not_a_login_or_name_is_policy_invalid() {
        let (_d, home) = home();
        let p = |rel: &str| home.root().join("policy").join(rel);
        for bad in ["owners/-bad.toml", "owners/a--b.toml", "repos/acme/a b.toml", "repos/-x/y.toml", "owners/acme.txt", "stray.toml"] {
            let err = key_of_scope_file(&home, &p(bad)).unwrap_err();
            assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{bad}");
            assert_eq!(err.details()["key"], serde_json::Value::Null);
            assert!(err.details()["file"].as_str().unwrap().ends_with(bad));
        }
        assert_eq!(key_of_scope_file(&home, &p("owners/acme.toml")).unwrap(), PolicyKey::Owner(owner("acme")));
        assert_eq!(key_of_scope_file(&home, &p("repos/acme/cpd.toml")).unwrap(), repo_key("acme", "cpd"));
    }

    #[test]
    fn a_missing_owner_file_is_owner_unknown_even_with_a_repo_file() {
        let (_d, home) = home();
        write(&home, "repos/acme/cpd.toml", "");
        for key in [PolicyKey::Owner(owner("acme")), repo_key("acme", "cpd")] {
            let err = resolve(&home, &key).unwrap_err();
            assert_eq!(err.code(), ErrorCode::OwnerUnknown);
            assert_eq!(err.exit().code(), 1);
            assert_eq!(err.details()["owner"], "acme");
            assert_eq!(err.details().len(), 1);
        }
    }

    #[test]
    fn the_unowned_row_entry_point_takes_cpds_default_without_an_owner_file() {
        let (_d, home) = home();
        write(&home, "repos/acme/cpd.toml", "[code]\ntext = \"ignored\"\n");
        let r = resolve_unowned_row(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(&r.policy, default_policy());
        for d in Domain::ALL {
            assert_eq!(r.sources.get(*d).source, Source::DefaultNoOwnerFile);
        }
        assert_eq!(r.sources.get(Domain::Pr).to_string(), "default (no owner file)");
        // With an owner file it resolves as `resolve` does.
        write(&home, "owners/acme.toml", "[pr]\ngate = \"local\"\n");
        let r = resolve_unowned_row(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.pr.gate, Gate::Local);
        assert_eq!(r.policy.code.text, "ignored");
    }

    #[test]
    fn a_symlinked_file_has_its_realpath_as_source() {
        let (dir, home) = home();
        let target = dir.path().join("dotfiles").join("clayton.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "[code]\ntext = \"x\"\n").unwrap();
        let link = home.root().join("policy/owners/clayton-duarte.toml");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let r = resolve(&home, &PolicyKey::Owner(owner("clayton-duarte"))).unwrap();
        let expected = std::fs::canonicalize(&target).unwrap();
        assert_eq!(r.sources.get(Domain::Code).source, Source::Owner(expected.clone()));
        assert_eq!(r.sources.get(Domain::Code).to_string(), format!("owner:{}", expected.display()));
    }

    #[test]
    fn a_dangling_symlink_is_a_missing_owner_file() {
        let (_d, home) = home();
        let link = home.root().join("policy/owners/acme.toml");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(Path::new("/nonexistent/acme.toml"), &link).unwrap();
        let err = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap_err();
        assert_eq!(err.code(), ErrorCode::OwnerUnknown);
    }

    #[test]
    fn audience_is_fixed_in_code_and_texts_for_filters_by_role() {
        assert_eq!(audience(Domain::Pr), &[Role::Lead, Role::Worker]);
        assert_eq!(audience(Domain::Deploy), &[Role::Lead]);
        assert_eq!(audience(Domain::Instructions), &[Role::Manager, Role::Lead, Role::Worker]);
        let (_d, home) = home();
        write(
            &home,
            "owners/acme.toml",
            "[pr]\ntext = \"p\"\n[deploy]\ntext = \"d\"\n[instructions]\ntext = \"i\"\n[code]\ntext = \"\"\n",
        );
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        let names = |role| r.texts_for(role).iter().map(|(d, _, t)| format!("{}={t}", d.name())).collect::<Vec<_>>();
        assert_eq!(names(Role::Manager), ["instructions=i"]);
        assert_eq!(names(Role::Lead), ["pr=p", "deploy=d", "instructions=i"]);
        assert_eq!(names(Role::Worker), ["pr=p", "instructions=i"]);
    }

    #[test]
    fn validation_rows_owner_and_repo_errors() {
        let acme = owner("acme");
        let e = repo_unkeyed(Some("https://gitlab.com/a/b"));
        assert_eq!((e.code(), e.exit().code()), (ErrorCode::RepoUnkeyed, 1));
        assert_eq!(e.details()["url"], "https://gitlab.com/a/b");
        assert_eq!(repo_unkeyed(None).details()["url"], serde_json::Value::Null);

        let e = owner_mismatch(&acme, &owner("other"));
        assert_eq!((e.code(), e.exit().code()), (ErrorCode::OwnerMismatch, 3));
        assert_eq!(e.details()["owner"], "acme");
        assert_eq!(e.details()["repo_owner"], "other");

        let ctx = ContextId::parse("ctx").unwrap();
        let e = owner_unset(&ctx);
        assert_eq!((e.code(), e.exit().code()), (ErrorCode::OwnerUnset, 3));
        assert_eq!(e.details()["context"], "ctx");
        assert_eq!(e.details()["next"], "cpd context owner <login> --context ctx");

        let e = owner_set(&ctx, &acme);
        assert_eq!((e.code(), e.exit().code()), (ErrorCode::OwnerSet, 3));
        assert_eq!(e.details()["context"], "ctx");
        assert_eq!(e.details()["owner"], "acme");
    }

    // The contract test (§8.3 "Adding a domain or a key"): the domains table of
    // docs/contract.md is the registry's documentation, and this pins them equal.

    struct Row {
        domain: String,
        keys: Vec<(String, Option<String>, Vec<String>)>, // key path, default, allowed values
        audience: Vec<String>,
    }

    fn contract_rows() -> Vec<Row> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contract.md");
        let doc = std::fs::read_to_string(path).unwrap();
        let start = doc.find("**The domains** of the first build.").expect("domains table");
        let mut rows = Vec::new();
        let mut in_table = false;
        for line in doc[start..].lines() {
            if line.starts_with('|') {
                in_table = true;
                let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
                // Cells may contain `|` inside code spans (`"local" \| "ci"`): rejoin on the escape.
                let line = line.replace("\\|", "\u{1}");
                let cells: Vec<String> =
                    line.trim().trim_matches('|').split('|').map(|c| c.replace('\u{1}', "|").trim().to_owned()).collect();
                let _ = cells.len();
                if cells[0] == "Domain" || cells[0].starts_with("---") {
                    continue;
                }
                let domain = cells[0].trim_matches('`').to_owned();
                let keys = parse_keys(&cells[1]);
                let audience = cells[cells.len() - 1].split(',').map(|r| r.trim().to_owned()).collect();
                rows.push(Row { domain, keys, audience });
            } else if in_table {
                break;
            }
        }
        rows
    }

    /// `` `gate = "local" | "ci"` (`ci`; …) `` gives ("gate", Some("ci"), ["local", "ci"]).
    fn parse_keys(cell: &str) -> Vec<(String, Option<String>, Vec<String>)> {
        let parts: Vec<&str> = cell.split('`').collect();
        let mut keys = Vec::new();
        let mut i = 1;
        while i < parts.len() {
            let span = parts[i];
            if let Some((name, values)) = span.split_once(" = ") {
                let allowed: Vec<String> = values.split('|').map(|v| v.trim().trim_matches('"').to_owned()).collect();
                let default = (i + 2 < parts.len() && parts[i + 1].trim_end().ends_with('(')).then(|| parts[i + 2].to_owned());
                keys.push((name.to_owned(), default, allowed));
            }
            i += 2;
        }
        keys
    }

    fn leaves(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, String)>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    leaves(&p, v, out);
                }
            }
            serde_json::Value::String(s) => out.push((prefix.to_owned(), s.clone())),
            other => panic!("unexpected default value at {prefix}: {other}"),
        }
    }

    #[test]
    fn the_contract_domains_table_is_the_code() {
        let rows = contract_rows();
        let doc_domains: Vec<&str> = rows.iter().map(|r| r.domain.as_str()).collect();
        let code_domains: Vec<&str> = Domain::ALL.iter().map(|d| d.name()).collect();
        assert_eq!(doc_domains, code_domains, "the table's domains are Domain::ALL");

        let policy_json = serde_json::to_value(default_policy()).unwrap();
        let mut policy_fields: Vec<&str> = policy_json.as_object().unwrap().keys().map(String::as_str).collect();
        let mut sorted = code_domains.clone();
        sorted.sort();
        policy_fields.sort();
        assert_eq!(policy_fields, sorted, "the table's domains are Policy's fields");

        let default_table: toml::Table = include_str!("default.toml").parse().unwrap();
        let mut tables: Vec<&str> = default_table.keys().map(String::as_str).collect();
        tables.sort();
        assert_eq!(tables, sorted, "the table's domains are default.toml's tables");

        for row in &rows {
            let domain = Domain::from_name(&row.domain).unwrap();
            // Typed keys and their defaults.
            let mut found = Vec::new();
            leaves(&row.domain, &policy_json[&row.domain], &mut found);
            let mut typed: Vec<(String, String)> = found
                .into_iter()
                .filter(|(p, _)| !p.ends_with(".text"))
                .map(|(p, v)| (p[row.domain.len() + 1..].to_owned(), v))
                .collect();
            typed.sort();
            let mut documented: Vec<(String, String)> = row
                .keys
                .iter()
                .map(|(k, d, _)| (k.clone(), d.clone().unwrap_or_else(|| panic!("{}.{k} has no documented default", row.domain))))
                .collect();
            documented.sort();
            assert_eq!(documented, typed, "typed keys and defaults of {}", row.domain);
            // Every allowed value of a documented key is accepted by the loader.
            for (key, _, allowed) in &row.keys {
                for value in allowed {
                    let mut table = toml::Table::new();
                    let mut leaf = toml::Value::String(value.clone());
                    for part in key.split('.').rev() {
                        leaf = toml::Value::Table(toml::Table::from_iter([(part.to_owned(), leaf)]));
                    }
                    table.insert(row.domain.clone(), leaf);
                    parse_scope(Path::new("t.toml"), &table.to_string())
                        .unwrap_or_else(|e| panic!("{}.{key} = {value:?} is refused: {e}", row.domain));
                }
            }
            // Audience.
            let roles: Vec<&str> = audience(domain).iter().map(|r| match r {
                Role::Manager => "Manager",
                Role::Lead => "Lead",
                Role::Worker => "Worker",
            }).collect();
            assert_eq!(row.audience, roles, "audience of {}", row.domain);
        }
    }
}
