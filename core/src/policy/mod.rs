//! Policies (contract §8.3): typed domains with cpd's compiled default, owner
//! and repo scope files, and one entry point, [`resolve`], per policy key.
//!
//! - [`domain`]: the domains, [`Policy`], the audience table;
//! - [`load`]: parsing and validating one scope file;
//! - [`origin`]: keying a repo from its `origin` URL;
//! - [`resolve`]: the default, the owner file, the repo file, per domain;
//! - [`show`]: `cpd policy show`.

mod domain;
mod exposure;
mod known;
mod load;
mod machine;
pub mod model;
mod origin;
mod jira;
mod post;
mod resolve;
pub(crate) mod show;

use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, OwnerLogin, RepoName};
use crate::session::Role;

pub use domain::{
    audience, Checks, Declared, Deploys, Domain, DomainSource, Feedback, Gate, GateAlias, Gates, Jira, JiraSet, Policy, Post, PostSet, Pr, Workflow, ResourceScope, Resources, Scope, Source, Sources,
    Text,
};
/// The `policy` commands (commands.md §22).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Show the resolved policy of a key (§22.1)
    Show(show::ShowArgs),
}

/// Runs a `policy` command.
pub fn run(command: Command) -> Result<crate::output::Output, CpdError> {
    match command {
        Command::Show(args) => show::show(args),
    }
}

pub use machine::{name_ok, load_machine, parse_machine, Exposure, Machine, MachineSource, McpOauth, McpServer};
pub use show::KeyArgs;
pub use origin::{parse_origin, repo_key};
pub use exposure::ScopeKind;
pub use load::{load_scope_file, parse_scope, policy_invalid};
pub use post::render_post;
pub use known::known_owners;
pub use resolve::{check_file_names, default_policy, key_of_scope_file, policy_dir, repo_path, resolve, resolve_machine, resolve_unowned_row};

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
    /// Deprecations met while resolving (the `[pr] gate` alias), for `cpd policy show`.
    pub warnings: Vec<String>,
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

/// The owner whose policy an unknown owner falls back to, once the User confirms (contract §8.3).
pub const FALLBACK_OWNER: &str = "clayton-duarte";

/// The flag that confirms the fallback.
const CONFIRM_FLAG: &str = "--confirm-fallback";

/// `owner_unconfirmed` (exit 1): the key's owner has no `<owner>/owner.toml` (a file or a link that resolves).
pub fn owner_unconfirmed(home: &crate::home::CpdHome, owner: &OwnerLogin) -> CpdError {
    let file = resolve::owner_path(home, owner);
    CpdError::new(
        ErrorCode::OwnerUnconfirmed,
        format!(
            "The owner {owner} has no policy file ({}). Warn the User and ask: if they confirm, repeat with {CONFIRM_FLAG} to use {FALLBACK_OWNER}'s policy for {owner}.",
            file.display()
        ),
    )
    .with("owner", owner.as_str())
    .with("file", file.display().to_string())
    .with("fallback", FALLBACK_OWNER)
    .with("flag", CONFIRM_FLAG)
}

/// Confirms the fallback for `owner`: creates `<owner>/` when missing and links `<owner>/owner.toml` to
/// `../clayton-duarte/owner.toml` (relative, atomic: made under a temporary name, then renamed over a dangling
/// link). A known owner changes nothing; a missing `clayton-duarte/owner.toml` is `fallback_missing`, and nothing is created.
pub fn confirm_fallback(home: &crate::home::CpdHome, owner: &OwnerLogin) -> Result<(), CpdError> {
    let file = resolve::owner_path(home, owner);
    if std::fs::metadata(&file).is_ok() {
        return Ok(());
    }
    let fallback = OwnerLogin::parse(FALLBACK_OWNER).expect("the fallback owner is a valid login");
    let source = resolve::owner_path(home, &fallback);
    if std::fs::metadata(&source).is_err() {
        return Err(CpdError::new(
            ErrorCode::FallbackMissing,
            format!("The fallback owner's policy file {} does not exist: nothing was created.", source.display()),
        )
        .with("file", source.display().to_string()));
    }
    let dir = file.parent().expect("an owner file has a directory");
    let target = format!("../{FALLBACK_OWNER}/{}", resolve::OWNER_FILE);
    let tmp = dir.join(format!(".{}.tmp", resolve::OWNER_FILE));
    let io = |e: std::io::Error| {
        CpdError::new(ErrorCode::FileUnreadable, format!("Cannot link {}: {e}.", file.display())).with("file", file.display().to_string())
    };
    std::fs::create_dir_all(dir).map_err(io)?;
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(&target, &tmp).map_err(io)?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
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
    CpdError::new(ErrorCode::OwnerUnset, format!("The Context {context} has no owner: set it with `cpd context owner <login> --context {context}`."))
        .with("context", context.as_str())
}

/// `owner_in_use` (exit 3): `context owner` changing the owner of a Context whose Plans have Workflow rows.
pub fn owner_in_use(context: &ContextId, owner: &OwnerLogin, workflows: &[String]) -> CpdError {
    CpdError::new(
        ErrorCode::OwnerInUse,
        format!(
            "The Context {context} has the owner {owner} and its Plans have Workflow rows ({}): the owner of a Context cannot change once a Workflow row exists.",
            workflows.join(", ")
        ),
    )
    .with("context", context.as_str())
    .with("owner", owner.as_str())
    .with("prs", workflows)
}


#[cfg(test)]
#[path = "resources_tests.rs"]
mod resources_tests;

#[cfg(test)]
#[path = "deploys_tests.rs"]
mod deploys_tests;

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
    fn default_sets_every_domain_with_gate_none_and_any() {
        let d = default_policy();
        assert_eq!((d.gate.merge, d.gate.workflow), (Gate::None, Gate::None));
        assert_eq!(d.pr.feedback.checks, Checks::Any);
        for domain in Domain::ALL {
            assert_eq!(d.text(*domain), "");
        }
    }

    #[test]
    fn empty_owner_file_is_default_everywhere() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "");
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(&r.policy, default_policy());
        for d in Domain::ALL {
            assert_eq!(r.sources.get(*d), &DomainSource { source: Source::Default, left_out: vec![] });
        }
    }

    #[test]
    fn precedence_is_repo_over_owner_over_default_per_domain() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "[code]\ntext = \"owner code\"\n[tests]\ntext = \"owner tests\"\n");
        write(&home, "acme/cpd.toml", "[tests]\ntext = \"repo tests\"\n");
        let r = resolve(&home, &repo_key("Acme", "CPD")).unwrap();
        assert_eq!(r.policy.code.text, "owner code");
        assert_eq!(r.sources.get(Domain::Code).source, Source::Owner(real(&home, "acme/owner.toml")));
        assert_eq!(r.policy.tests.text, "repo tests");
        assert_eq!(r.sources.get(Domain::Tests).source, Source::Repo(real(&home, "acme/cpd.toml")));
        assert_eq!(r.policy.deploy.text, "");
        assert_eq!(r.sources.get(Domain::Deploy).source, Source::Default);
        // The owner key resolves without the repo file.
        let o = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(o.policy.tests.text, "owner tests");
    }

    #[test]
    fn a_repo_without_a_file_resolves_from_its_owner() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "[code]\ntext = \"c\"\n");
        write(&home, "acme/other.toml", "");
        let r = resolve(&home, &repo_key("acme", "other")).unwrap();
        assert_eq!(r.policy.code.text, "c");
    }

    #[test]
    fn a_repo_without_a_file_resolves_from_its_owner_and_the_owner_is_still_checked_first() {
        let (_d, home) = home();
        let err = resolve(&home, &repo_key("acme", "other")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::OwnerUnconfirmed, "the owner is checked first");
        write(&home, "acme/owner.toml", "[gate]\npr = \"local\"\n");
        let r = resolve(&home, &repo_key("acme", "other")).unwrap();
        assert_eq!(r.policy.gate.workflow, Gate::Local);
        assert!(matches!(r.sources.get(Domain::Gates).source, Source::Owner(_)));
    }

    #[test]
    fn the_repo_only_domains_are_policy_invalid_in_an_owner_file() {
        for (domain, text) in [
            ("hooks", "[hooks]\npre-start = \"x\"\n"),
            ("test-commands", "[test-commands]\nbuild = \"b\"\nrun = \"r\"\n"),
            ("protected", "[protected]\npaths = []\n"),
        ] {
            let err = invalid(text);
            assert_eq!(err.details()["key"], domain);
            assert!(err.message().contains("exposed to repo only"), "{err:?}");
        }
    }

    #[test]
    fn required_checks_defaults_to_empty_and_the_repo_file_wins_over_the_owner_file() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "");
        assert_eq!(resolve(&home, &repo_key("acme", "cpd")).unwrap().policy.pr.required_checks, Vec::<String>::new());
        write(&home, "acme/owner.toml", "[pr]\nrequired_checks = [\"owner-gate\"]\n");
        assert_eq!(resolve(&home, &repo_key("acme", "cpd")).unwrap().policy.pr.required_checks, ["owner-gate"]);
        write(&home, "acme/cpd.toml", "[pr]\nrequired_checks = [\"gate\"]\n");
        assert_eq!(resolve(&home, &repo_key("acme", "cpd")).unwrap().policy.pr.required_checks, ["gate"]);
    }

    #[test]
    fn a_set_domain_replaces_the_whole_domain_and_left_out_keys_take_cpds_default() {
        let (_d, home) = home();
        write(
            &home,
            "acme/owner.toml",
            "[pr]\nfeedback.checks = \"none\"\njira_required = true\nhuman_gated_contexts = []\nexcused_labels = []\nrequired_checks = []\ntext = \"owner pr\"\n[pr.post]\nenabled = true\nchannel = \"\"\ntemplate = \"{{ url }}\"\nmerged_reaction = \"\"\n[pr.jira]\nin_progress = \"\"\nin_review = \"\"\ndone = \"\"\nestimate_fields = []\n",
        );
        write(&home, "acme/cpd.toml", "[pr]\n");
        let r = resolve(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.pr.feedback.checks, Checks::Any, "cpd's default, not the owner's none");
        assert_eq!(r.policy.pr.text, "", "the text is replaced, not inherited");
        let src = r.sources.get(Domain::Pr);
        assert_eq!(src.source, Source::Repo(real(&home, "acme/cpd.toml")));
        assert_eq!(
            src.left_out,
            ["feedback.checks", "pr.post.enabled", "pr.post.channel", "pr.post.template", "pr.post.merged_reaction", "pr.human_gated_contexts", "pr.excused_labels", "pr.required_checks", "pr.jira.in_progress", "pr.jira.in_review", "pr.jira.done", "pr.jira.estimate_fields"]
        );
        assert!(src.to_string().ends_with(" (key default)"));
        // The owner file sets every typed key: no `(key default)`.
        let o = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!(o.policy.pr.feedback.checks, Checks::None);
        assert!(!o.sources.get(Domain::Pr).key_default());
    }

    #[test]
    fn the_deprecated_pr_gate_alias_sets_both_gates_and_warns() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "[pr]\ngate = \"local\"\n");
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!((r.policy.gate.merge, r.policy.gate.workflow), (Gate::Local, Gate::Local));
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("[pr] gate"), "{}", r.warnings[0]);
        write(&home, "acme/owner.toml", "[pr]\ngate = \"ci\"\n");
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!((r.policy.gate.merge, r.policy.gate.workflow), (Gate::None, Gate::None));
        // An explicit `[gate]` table wins over the alias.
        write(&home, "acme/owner.toml", "[gate]\npr = \"local\"\n\n[pr]\ngate = \"ci\"\n");
        let r = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap();
        assert_eq!((r.policy.gate.merge, r.policy.gate.workflow), (Gate::None, Gate::Local));
    }

    #[test]
    fn an_empty_table_is_cpds_default_for_the_whole_domain() {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", "[pr]\nfeedback.checks = \"none\"\ntext = \"t\"\n");
        write(&home, "acme/cpd.toml", "[pr]\n");
        let r = resolve(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.pr, default_policy().pr);
        assert!(matches!(r.sources.get(Domain::Pr).source, Source::Repo(_)));
        assert!(r.sources.get(Domain::Pr).key_default());
    }

    fn invalid(text: &str) -> CpdError {
        let (_d, home) = home();
        write(&home, "acme/owner.toml", text);
        let err = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyInvalid);
        assert_eq!(err.exit().code(), 1);
        assert!(err.details()["file"].as_str().unwrap().ends_with("acme/owner.toml"));
        err
    }

    #[test]
    fn invalid_files_are_policy_invalid_with_file_and_key() {
        let key = |text: &str| invalid(text).details()["key"].clone();
        assert_eq!(key("[mcp]\n"), serde_json::json!("mcp"));
        assert_eq!(key("[hooks]\ntext = \"x\"\n"), serde_json::json!("hooks.text"), "no `text` in a repo-only domain");
        assert_eq!(key("[protected]\ntext = \"x\"\n"), serde_json::json!("protected.text"));
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
        write(&home, "acme/owner.toml", "");
        write(&home, "acme/cpd.toml", "[pr]\ngate = 1\n");
        let err = resolve(&home, &repo_key("acme", "cpd")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PolicyInvalid);
        assert!(err.details()["file"].as_str().unwrap().ends_with("acme/cpd.toml"));
    }

    #[test]
    fn a_file_name_that_is_not_a_login_or_name_is_policy_invalid() {
        let (_d, home) = home();
        let p = |rel: &str| home.root().join("policy").join(rel);
        for bad in ["-bad/owner.toml", "a--b/owner.toml", "acme/a b.toml", "-x/y.toml", "acme/x.txt", "stray.toml", "Clayton-Duarte/owner.toml", "clayton_duarte/owner.toml", "acme/Cpd.toml"] {
            let err = key_of_scope_file(&home, &p(bad)).unwrap_err();
            assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{bad}");
            assert_eq!(err.details()["key"], serde_json::Value::Null);
            assert!(err.details()["file"].as_str().unwrap().ends_with(bad));
        }
        assert_eq!(key_of_scope_file(&home, &p("acme/owner.toml")).unwrap(), PolicyKey::Owner(owner("acme")));
        assert_eq!(key_of_scope_file(&home, &p("acme/cpd.toml")).unwrap(), repo_key("acme", "cpd"));
    }

    #[test]
    fn a_missing_owner_file_is_owner_unconfirmed_even_with_a_repo_file() {
        let (_d, home) = home();
        write(&home, "acme/cpd.toml", "");
        for key in [PolicyKey::Owner(owner("acme")), repo_key("acme", "cpd")] {
            let err = resolve(&home, &key).unwrap_err();
            assert_eq!(err.code(), ErrorCode::OwnerUnconfirmed);
            assert_eq!(err.exit().code(), 1);
            assert_eq!(err.details()["owner"], "acme");
            assert_eq!(err.details().len(), 4);
            assert_eq!(err.details()["fallback"], "clayton-duarte");
            assert_eq!(err.details()["flag"], "--confirm-fallback");
        }
    }

    #[test]
    fn the_unowned_row_entry_point_takes_cpds_default_without_an_owner_file() {
        let (_d, home) = home();
        write(&home, "acme/cpd.toml", "[code]\ntext = \"ignored\"\n");
        let r = resolve_unowned_row(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(&r.policy, default_policy());
        for d in Domain::ALL {
            assert_eq!(r.sources.get(*d).source, Source::DefaultNoOwnerFile);
        }
        assert_eq!(r.sources.get(Domain::Workflow).to_string(), "default (no owner file)");
        // With an owner file it resolves as `resolve` does.
        write(&home, "acme/owner.toml", "[gate]\npr = \"local\"\n");
        let r = resolve_unowned_row(&home, &repo_key("acme", "cpd")).unwrap();
        assert_eq!(r.policy.gate.workflow, Gate::Local);
        assert_eq!(r.policy.code.text, "ignored");
    }

    #[test]
    fn a_symlinked_file_has_its_realpath_as_source() {
        let (dir, home) = home();
        let target = dir.path().join("dotfiles").join("clayton.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "[code]\ntext = \"x\"\n").unwrap();
        let link = home.root().join("policy/clayton-duarte/owner.toml");
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
        let link = home.root().join("policy/acme/owner.toml");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(Path::new("/nonexistent/acme.toml"), &link).unwrap();
        let err = resolve(&home, &PolicyKey::Owner(owner("acme"))).unwrap_err();
        assert_eq!(err.code(), ErrorCode::OwnerUnconfirmed);
    }

    #[test]
    fn audience_is_fixed_in_code_and_texts_for_filters_by_role() {
        assert_eq!(audience(Domain::Pr), &[Role::Lead, Role::Worker]);
        assert_eq!(audience(Domain::Deploy), &[Role::Lead]);
        assert_eq!(audience(Domain::Instructions), &[Role::Manager, Role::Lead, Role::Worker]);
        let (_d, home) = home();
        write(
            &home,
            "acme/owner.toml",
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
        assert!(e.details().get("next").is_none() && e.message().contains("cpd context owner <login> --context ctx"));

        let e = owner_in_use(&ctx, &acme, &["p/a".to_string()]);
        assert_eq!((e.code(), e.exit().code()), (ErrorCode::OwnerInUse, 3));
        assert_eq!(e.details()["prs"], serde_json::json!(["p/a"]));
        assert_eq!(e.details()["context"], "ctx");
        assert_eq!(e.details()["owner"], "acme");
    }

    // The contract test (§8.3 "Adding a domain or a key"): the domains table of
    // docs/contract.md is the registry's documentation, and this pins them equal.

    struct Row {
        cell: String,
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
                // Cells may contain `|` inside code spans (`"local" \| "ci"`): rejoin on the escape.
                let line = line.replace("\\|", "\u{1}");
                let cells: Vec<String> =
                    line.trim().trim_matches('|').split('|').map(|c| c.replace('\u{1}', "|").trim().to_owned()).collect();
                if cells[0] == "Domain" || cells[0].starts_with("---") {
                    continue;
                }
                let domain = cells[0].trim_matches('`').to_owned();
                let keys = parse_keys(&cells[1]);
                let audience = cells[cells.len() - 1].split(',').map(|r| r.trim().to_owned()).collect();
                rows.push(Row { cell: cells[1].clone(), domain, keys, audience });
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
                let allowed: Vec<String> = if values.starts_with('[') { Vec::new() } else { values.split('|').map(|v| v.trim().trim_matches('"').to_owned()).collect() };
                let default = (i + 2 < parts.len() && parts[i + 1].trim_end().ends_with('(')).then(|| parts[i + 2].to_owned());
                // A list default is compared as compact JSON, the form `leaves` gives.
                let default = default.map(|d| match serde_json::from_str::<serde_json::Value>(&d) {
                    Ok(list @ serde_json::Value::Array(_)) => list.to_string(),
                    _ => d,
                });
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
            serde_json::Value::Bool(b) => out.push((prefix.to_owned(), b.to_string())),
            serde_json::Value::Array(_) => out.push((prefix.to_owned(), v.to_string())),
            other => panic!("unexpected default value at {prefix}: {other}"),
        }
    }

    #[test]
    fn the_contract_domains_table_is_the_code() {
        let rows = contract_rows();
        let doc_domains: Vec<&str> = rows.iter().map(|r| r.domain.as_str()).collect();
        let code_domains: Vec<&str> = Domain::ALL.iter().map(|d| d.name()).collect();
        // `model` is documented in §8.3 "Models", not in the table (its key rows are pinned by policy::model).
        let table_domains: Vec<&str> = code_domains.iter().copied().filter(|d| *d != "model").collect();
        assert_eq!(doc_domains, table_domains, "the table's domains are Domain::ALL but model");

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
            // A repo-only domain (empty audience) has list and optional values, documented in prose: no table
            // of `key = value` spans to compare, and the repo-only tests pin their defaults.
            if audience(domain).is_empty() {
                assert!(row.keys.is_empty(), "{} documents no `key = value` spans", row.domain);
                continue;
            }
            // Typed keys and their defaults.
            let mut found = Vec::new();
            leaves(&row.domain, &policy_json[&row.domain], &mut found);
            let mut typed: Vec<(String, String)> = found
                .into_iter()
                .filter(|(p, _)| !p.ends_with(".text"))
                .map(|(p, v)| (p[row.domain.len() + 1..].to_owned(), v))
                .collect();
            typed.sort();
            // A key documented in prose only (no `key = value` span) must still be named in the cell, as `key`.
            let spanned: Vec<&str> = row.keys.iter().map(|(k, _, _)| k.as_str()).collect();
            typed.retain(|(k, _)| {
                spanned.contains(&k.as_str()) || {
                    assert!(row.cell.contains(&format!("`{k}`")), "{}.{k} is not documented", row.domain);
                    false
                }
            });
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
                    let mut leaf = match value.as_str() {
                        "true" | "false" => toml::Value::Boolean(value == "true"),
                        _ => toml::Value::String(value.clone()),
                    };
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
