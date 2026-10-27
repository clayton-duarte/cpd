//! The typed domains of a policy (contract §8.3 "The domains").
//!
//! The `domains!` list below is the one registry: it generates the
//! [`Domain`] enum, the [`Policy`] (one field per domain), the [`Scope`] a
//! file sets (one optional table per domain), the audience table, and the
//! per-domain resolution. Adding a domain is one line there, one struct here,
//! its default in `default.toml` and one row of the contract's table.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::exposure::ScopeKind;
use super::model::{ModelSet, Models};
use crate::config::RepoTests;
use crate::git::hooks::Pipeline;
use crate::session::Role;

/// A Gate key: `local` runs the Gate, `none` leaves the check to the repo's CI (hs-d292, hs-d679).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Local,
    None,
}

/// The deprecated `[pr] gate` alias: `ci` and `none` both map to [`Gate::None`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateAlias {
    Local,
    Ci,
    None,
}

impl GateAlias {
    /// The [`Gate`] the deprecated key meant.
    pub fn gate(self) -> Gate {
        match self {
            GateAlias::Local => Gate::Local,
            GateAlias::Ci | GateAlias::None => Gate::None,
        }
    }
}

/// The `gate` domain: whether the merge and the pull request run the Gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gates {
    pub merge: Gate,
    pub pr: Gate,
    pub text: String,
}

/// `pr.feedback.checks`: whether failing checks raise feedback (hs-d292, hs-d283).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Checks {
    Any,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub checks: Checks,
}

/// The `pr` domain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pr {
    pub feedback: Feedback,
    pub text: String,
}

/// A domain with no typed keys: only `text`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub text: String,
}

/// What a scope file's `[pr.feedback]` table sets: a left-out key is `None`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackSet {
    pub checks: Option<Checks>,
}

/// What a scope file's `[pr]` table sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrSet {
    /// The deprecated `[pr] gate` alias; it is converted into the `gate` domain by `parse_scope`.
    pub gate: Option<GateAlias>,
    pub feedback: Option<FeedbackSet>,
    pub text: Option<String>,
}

/// What a scope file's `[gate]` table sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatesSet {
    pub merge: Option<Gate>,
    pub pr: Option<Gate>,
    pub text: Option<String>,
}

/// What a scope file's table of a domain with no typed keys sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSet {
    pub text: Option<String>,
}

/// What `Policy::text` reads: a domain with no `text` (the repo-only ones) has `""`.
pub trait HasText {
    fn text(&self) -> &str;
}

impl HasText for Pr {
    fn text(&self) -> &str {
        &self.text
    }
}

impl HasText for Gates {
    fn text(&self) -> &str {
        &self.text
    }
}

impl HasText for Text {
    fn text(&self) -> &str {
        &self.text
    }
}

/// The `hooks` domain (repo scope only, no `text`): each hook's pipeline of §7.3, empty when not set.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    #[serde(rename = "pre-start")]
    pub pre_start: Pipeline,
    #[serde(rename = "pre-commit")]
    pub pre_commit: Pipeline,
    #[serde(rename = "pre-merge")]
    pub pre_merge: Pipeline,
}

impl HasText for Hooks {
    fn text(&self) -> &str {
        ""
    }
}

/// The `test-commands` domain (repo scope only, no `text`): `[tests]` of §7.7, `None` when not set.
impl HasText for Option<RepoTests> {
    fn text(&self) -> &str {
        ""
    }
}

/// The `protected` domain (repo scope only, no `text`): the PR's protected paths of §7.5.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protected {
    pub paths: Vec<String>,
}

impl HasText for Protected {
    fn text(&self) -> &str {
        ""
    }
}

/// The `resources` domain (any scope, no `text`): the visible resources, resolved per name over host, owner and repo
/// (contract §8.3 "The `resources` domain"); `names` is sorted.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub names: Vec<String>,
    /// One per visible name, sorted by name (contract §8.3 "The `resources` domain").
    #[serde(default)]
    pub declared: Vec<Declared>,
}

/// The level of the file that declares a resource: it owns the resource's pool (contract §8.3, D2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceScope {
    Host,
    Owner,
    Repo,
}

/// A resource as declared: its name, the declaring level, its slots (`1..=64`) and the declaring file's path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declared {
    pub name: String,
    pub scope: ResourceScope,
    pub slots: u8,
    pub source: String,
}

impl HasText for Resources {
    fn text(&self) -> &str {
        ""
    }
}

/// A resource name: `^[a-z][a-z0-9-]{0,31}$`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceName(String);

impl ResourceName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResourceName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        let mut chars = name.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && name.len() <= 32
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok { Ok(ResourceName(name)) } else { Err(serde::de::Error::custom(format!("`{name}` is not a resource name (^[a-z][a-z0-9-]{{0,31}}$)"))) }
    }
}

/// One declared resource: a table with `slots` (`1..=64`, default 1; the range is checked by the loader).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDecl {
    pub slots: Option<i64>,
}

/// What a scope file's `[resources]` table sets: one table per resource name.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct ResourcesSet(pub std::collections::BTreeMap<ResourceName, ResourceDecl>);

/// What a scope file's `[hooks]` table sets. It is not read from a value (`toml::Table` sorts keys, and a
/// block's commands keep file order): the loader reads it from the file text (`load::parse_scope`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HooksSet {
    pub pre_start: Option<Pipeline>,
    pub pre_commit: Option<Pipeline>,
    pub pre_merge: Option<Pipeline>,
}

impl<'de> Deserialize<'de> for HooksSet {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("[hooks] is read from the file text, not from a value"))
    }
}

/// What a scope file's `[test-commands]` table sets. An empty table sets nothing.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCommandsSet {
    pub build: Option<String>,
    pub run: Option<String>,
    pub list: Option<String>,
}

impl TestCommandsSet {
    /// The commands the table sets: `None` for an empty table, else `build` and `run` are required and
    /// [`RepoTests::validate`] holds. The error is a sentence.
    pub fn commands(&self) -> Result<Option<RepoTests>, String> {
        let (build, run) = match (&self.build, &self.run) {
            (None, None) if self.list.is_none() => return Ok(None),
            (Some(build), Some(run)) => (build, run),
            (None, _) => return Err("[test-commands] has no `build`".to_owned()),
            (_, None) => return Err("[test-commands] has no `run`".to_owned()),
        };
        let tests = RepoTests { build: build.clone(), run: run.clone(), list: self.list.clone() };
        tests.validate().map_err(|e| e.replace("[tests]", "[test-commands]"))?;
        Ok(Some(tests))
    }
}

/// What a scope file's `[protected]` table sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedSet {
    pub paths: Option<Vec<String>>,
}

/// A table a scope sets, read over cpd's default.
pub trait Set {
    type Value: Clone;
    /// The value with every key the table leaves out taken from `base`, and
    /// the names of the **typed** keys left out (`(key default)`).
    fn fill(&self, base: &Self::Value) -> (Self::Value, Vec<&'static str>);
    /// The value when the table sets everything (only cpd's default does).
    fn complete(&self) -> Option<Self::Value>;
}

/// The typed keys of the `gate` and `pr` domains, by name.
pub const MERGE: &str = "merge";
pub const GATE_PR: &str = "pr";
pub const FEEDBACK_CHECKS: &str = "feedback.checks";

impl Set for GatesSet {
    type Value = Gates;

    fn fill(&self, base: &Gates) -> (Gates, Vec<&'static str>) {
        let left_out = left_out([(MERGE, self.merge.is_none()), (GATE_PR, self.pr.is_none())]);
        let value = Gates {
            merge: self.merge.unwrap_or(base.merge),
            pr: self.pr.unwrap_or(base.pr),
            text: self.text.clone().unwrap_or_else(|| base.text.clone()),
        };
        (value, left_out)
    }

    fn complete(&self) -> Option<Gates> {
        Some(Gates { merge: self.merge?, pr: self.pr?, text: self.text.clone()? })
    }
}

impl Set for PrSet {
    type Value = Pr;

    /// `[pr] gate` is the deprecated alias, not a key of the `pr` domain: `parse_scope` moves it into
    /// the `gate` domain of the same file, and an explicit `[gate]` table wins over it.
    fn fill(&self, base: &Pr) -> (Pr, Vec<&'static str>) {
        let checks = self.feedback.as_ref().and_then(|f| f.checks);
        let left_out = left_out([(FEEDBACK_CHECKS, checks.is_none())]);
        let value = Pr {
            feedback: Feedback { checks: checks.unwrap_or(base.feedback.checks) },
            text: self.text.clone().unwrap_or_else(|| base.text.clone()),
        };
        (value, left_out)
    }

    fn complete(&self) -> Option<Pr> {
        Some(Pr {
            feedback: Feedback { checks: self.feedback.as_ref()?.checks? },
            text: self.text.clone()?,
        })
    }
}

impl Set for TextSet {
    type Value = Text;

    fn fill(&self, base: &Text) -> (Text, Vec<&'static str>) {
        (Text { text: self.text.clone().unwrap_or_else(|| base.text.clone()) }, Vec::new())
    }

    fn complete(&self) -> Option<Text> {
        Some(Text { text: self.text.clone()? })
    }
}

/// The typed keys of the repo-only domains, by name.
pub const PRE_START: &str = "pre-start";
pub const PRE_COMMIT: &str = "pre-commit";
pub const PRE_MERGE: &str = "pre-merge";
pub const BUILD: &str = "build";
pub const RUN: &str = "run";
pub const LIST: &str = "list";
pub const PATHS: &str = "paths";

fn left_out<const N: usize>(keys: [(&'static str, bool); N]) -> Vec<&'static str> {
    keys.into_iter().filter_map(|(name, out)| out.then_some(name)).collect()
}

impl Set for HooksSet {
    type Value = Hooks;

    fn fill(&self, base: &Hooks) -> (Hooks, Vec<&'static str>) {
        let out = left_out([
            (PRE_START, self.pre_start.is_none()),
            (PRE_COMMIT, self.pre_commit.is_none()),
            (PRE_MERGE, self.pre_merge.is_none()),
        ]);
        let value = Hooks {
            pre_start: self.pre_start.clone().unwrap_or_else(|| base.pre_start.clone()),
            pre_commit: self.pre_commit.clone().unwrap_or_else(|| base.pre_commit.clone()),
            pre_merge: self.pre_merge.clone().unwrap_or_else(|| base.pre_merge.clone()),
        };
        (value, out)
    }

    fn complete(&self) -> Option<Hooks> {
        // A hook left out is no hook: cpd's default table is empty.
        Some(Hooks {
            pre_start: self.pre_start.clone().unwrap_or_default(),
            pre_commit: self.pre_commit.clone().unwrap_or_default(),
            pre_merge: self.pre_merge.clone().unwrap_or_default(),
        })
    }
}

impl Set for TestCommandsSet {
    type Value = Option<RepoTests>;

    fn fill(&self, base: &Option<RepoTests>) -> (Option<RepoTests>, Vec<&'static str>) {
        // The loader validated the table: it sets nothing, or `build` and `run`.
        match self.commands() {
            Ok(Some(tests)) => (Some(tests), left_out([(LIST, self.list.is_none())])),
            _ => (base.clone(), vec![BUILD, RUN, LIST]),
        }
    }

    fn complete(&self) -> Option<Option<RepoTests>> {
        self.commands().ok()
    }
}

impl Set for ProtectedSet {
    type Value = Protected;

    fn fill(&self, base: &Protected) -> (Protected, Vec<&'static str>) {
        (Protected { paths: self.paths.clone().unwrap_or_else(|| base.paths.clone()) }, left_out([(PATHS, self.paths.is_none())]))
    }

    fn complete(&self) -> Option<Protected> {
        Some(Protected { paths: self.paths.clone()? })
    }
}

impl Set for ResourcesSet {
    type Value = Resources;

    fn fill(&self, _base: &Resources) -> (Resources, Vec<&'static str>) {
        (Resources { names: self.0.keys().map(|n| n.0.clone()).collect(), declared: Vec::new() }, Vec::new())
    }

    fn complete(&self) -> Option<Resources> {
        Some(Resources { names: self.0.keys().map(|n| n.0.clone()).collect(), declared: Vec::new() })
    }
}

/// The visible resources of `layers` (most specific first): the union per name, the narrowest declaration of a name
/// wins whole (contract §8.3 "The `resources` domain").
pub(super) fn resources_union(layers: &[(Source, &Scope)]) -> Resources {
    let mut by_name = std::collections::BTreeMap::new();
    for (source, scope) in layers.iter().rev() {
        let (level, path) = match source {
            Source::Machine(p) => (ResourceScope::Host, p),
            Source::Owner(p) => (ResourceScope::Owner, p),
            Source::Repo(p) => (ResourceScope::Repo, p),
            Source::Default | Source::DefaultNoOwnerFile => continue,
        };
        let Some(set) = &scope.resources else { continue };
        for (name, decl) in &set.0 {
            let slots = decl.slots.map_or(1, |n| u8::try_from(n).expect("the loader checked slots to 1..=64"));
            by_name.insert(name.0.clone(), Declared { name: name.0.clone(), scope: level, slots, source: path.display().to_string() });
        }
    }
    Resources { names: by_name.keys().cloned().collect(), declared: by_name.into_values().collect() }
}

/// Where a domain's value came from (contract §8.3 "Model").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// cpd's compiled default.
    Default,
    /// The machine file, by realpath.
    Machine(PathBuf),
    /// The owner file, by realpath.
    Owner(PathBuf),
    /// The repo file, by realpath.
    Repo(PathBuf),
    /// cpd's default for a row no PR owns whose owner has no file.
    DefaultNoOwnerFile,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Default => f.write_str("default"),
            Source::Machine(p) => write!(f, "machine:{}", p.display()),
            Source::Owner(p) => write!(f, "owner:{}", p.display()),
            Source::Repo(p) => write!(f, "repo:{}", p.display()),
            Source::DefaultNoOwnerFile => f.write_str("default (no owner file)"),
        }
    }
}

/// A domain's source, and the typed keys it leaves out, which took cpd's default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSource {
    pub source: Source,
    pub left_out: Vec<&'static str>,
}

impl DomainSource {
    /// Whether a typed key was left out (`(key default)`).
    pub fn key_default(&self) -> bool {
        !self.left_out.is_empty()
    }
}

impl std::fmt::Display for DomainSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)?;
        if self.key_default() {
            f.write_str(" (key default)")?;
        }
        Ok(())
    }
}

macro_rules! domains {
    (@name $field:ident) => { stringify!($field) };
    (@name $field:ident $name:literal) => { $name };
    ($($field:ident $(= $name:literal)?, $variant:ident, [$($role:ident),*], [$($scope:ident),+], $value:ty, $set:ty;)+) => {
        /// A domain of the policy.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Domain {
            $($variant,)+
        }

        impl Domain {
            /// Every domain, in the contract's order.
            pub const ALL: &'static [Domain] = &[$(Domain::$variant,)+];

            /// The TOML table name.
            pub fn name(self) -> &'static str {
                match self {
                    $(Domain::$variant => domains!(@name $field $($name)?),)+
                }
            }

            pub fn from_name(name: &str) -> Option<Domain> {
                Domain::ALL.iter().copied().find(|d| d.name() == name)
            }
        }

        /// Who a domain's `text` reaches, fixed in code (contract §8.3).
        pub fn audience(domain: Domain) -> &'static [Role] {
            match domain {
                $(Domain::$variant => &[$(Role::$role),*],)+
            }
        }

        /// The scopes a domain may be set in, fixed in code (contract §8.3 "Exposure").
        pub fn exposure(domain: Domain) -> &'static [ScopeKind] {
            match domain {
                $(Domain::$variant => &[$(ScopeKind::$scope),+],)+
            }
        }

        /// A policy: every domain has a value, always.
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Policy {
            $($(#[serde(rename = $name)])? pub $field: $value,)+
        }

        impl Policy {
            /// The `text` of `domain`.
            pub fn text(&self, domain: Domain) -> &str {
                match domain {
                    $(Domain::$variant => HasText::text(&self.$field),)+
                }
            }
        }

        /// A scope file: the domains it has a table for.
        #[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Scope {
            $($(#[serde(rename = $name)])? pub $field: Option<$set>,)+
        }

        impl Scope {
            /// Whether the file has a table for `domain`.
            pub(super) fn sets(&self, domain: Domain) -> bool {
                match domain {
                    $(Domain::$variant => self.$field.is_some(),)+
                }
            }

            /// cpd's default from its own scope file: it must set everything.
            pub(super) fn complete(&self) -> Option<Policy> {
                Some(Policy { $($field: self.$field.as_ref()?.complete()?,)+ })
            }
        }

        /// Each domain's source.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct Sources([DomainSource; Domain::ALL.len()]);

        impl Sources {
            pub fn get(&self, domain: Domain) -> &DomainSource {
                &self.0[domain as usize]
            }
        }

        /// Reads `layers` (most specific first) over `default`, per domain.
        pub(super) fn merge(default: &Policy, layers: &[(Source, &Scope)]) -> super::Resolved {
            let mut sources = Sources::all(Source::Default);
            let mut policy = Policy {
                $($field: match layers.iter().find_map(|(src, scope)| scope.$field.as_ref().map(|set| (src, set))) {
                    Some((src, set)) => {
                        let (value, left_out) = set.fill(&default.$field);
                        sources.0[Domain::$variant as usize] =
                            DomainSource { source: src.clone(), left_out };
                        value
                    }
                    None => default.$field.clone(),
                },)+
            };
            policy.resources = resources_union(layers);
            let warnings = layers
                .iter()
                .filter_map(|(src, scope)| {
                    scope.pr.as_ref()?.gate.map(|_| {
                        format!(
                            "{src}: the deprecated [pr] gate key is set; use [gate] merge and [gate] pr instead \
                             (local maps to both local, ci/none to both none)"
                        )
                    })
                })
                .collect();
            super::Resolved { policy, sources, warnings }
        }
    };
}

domains! {
    gate, Gates, [Lead, Worker], [Machine, Owner, Repo], Gates, GatesSet;
    pr, Pr, [Lead, Worker], [Machine, Owner, Repo], Pr, PrSet;
    deploy, Deploy, [Lead], [Machine, Owner, Repo], Text, TextSet;
    code, Code, [Lead, Worker], [Machine, Owner, Repo], Text, TextSet;
    tests, Tests, [Lead, Worker], [Machine, Owner, Repo], Text, TextSet;
    monitor, Monitor, [Lead, Worker], [Machine, Owner, Repo], Text, TextSet;
    instructions, Instructions, [Manager, Lead, Worker], [Machine, Owner, Repo], Text, TextSet;
    model, Model, [], [Machine, Owner, Repo], Models, ModelSet;
    hooks, Hooks, [], [Repo], Hooks, HooksSet;
    test_commands = "test-commands", TestCommands, [], [Repo], Option<RepoTests>, TestCommandsSet;
    protected, Protected, [], [Repo], Protected, ProtectedSet;
    resources, Resources, [], [Machine, Owner, Repo], Resources, ResourcesSet;
}

impl Sources {
    /// All sources with one value (the default, or a missing owner file).
    pub(super) fn all(source: Source) -> Sources {
        Sources(std::array::from_fn(|_| DomainSource { source: source.clone(), left_out: Vec::new() }))
    }
}
