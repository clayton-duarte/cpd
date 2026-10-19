//! The typed domains of a policy (contract §8.3 "The domains").
//!
//! The `domains!` list below is the one registry: it generates the
//! [`Domain`] enum, the [`Policy`] (one field per domain), the [`Scope`] a
//! file sets (one optional table per domain), the audience table, and the
//! per-domain resolution. Adding a domain is one line there, one struct here,
//! its default in `default.toml` and one row of the contract's table.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::session::Role;

/// `pr.gate`: whether `pr open`, `pr push` and `pr rebase` run the Gate (hs-d292).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gate {
    Local,
    Ci,
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
    pub gate: Gate,
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
    pub gate: Option<Gate>,
    pub feedback: Option<FeedbackSet>,
    pub text: Option<String>,
}

/// What a scope file's table of a domain with no typed keys sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSet {
    pub text: Option<String>,
}

/// A table a scope sets, read over cpd's default.
pub trait Set {
    type Value: Clone;
    /// The value with every key the table leaves out taken from `base`, and
    /// whether any **typed** key was left out (`(key default)`).
    fn fill(&self, base: &Self::Value) -> (Self::Value, bool);
    /// The value when the table sets everything (only cpd's default does).
    fn complete(&self) -> Option<Self::Value>;
}

impl Set for PrSet {
    type Value = Pr;

    fn fill(&self, base: &Pr) -> (Pr, bool) {
        let checks = self.feedback.as_ref().and_then(|f| f.checks);
        let key_default = self.gate.is_none() || checks.is_none();
        let value = Pr {
            gate: self.gate.unwrap_or(base.gate),
            feedback: Feedback { checks: checks.unwrap_or(base.feedback.checks) },
            text: self.text.clone().unwrap_or_else(|| base.text.clone()),
        };
        (value, key_default)
    }

    fn complete(&self) -> Option<Pr> {
        Some(Pr {
            gate: self.gate?,
            feedback: Feedback { checks: self.feedback.as_ref()?.checks? },
            text: self.text.clone()?,
        })
    }
}

impl Set for TextSet {
    type Value = Text;

    fn fill(&self, base: &Text) -> (Text, bool) {
        (Text { text: self.text.clone().unwrap_or_else(|| base.text.clone()) }, false)
    }

    fn complete(&self) -> Option<Text> {
        Some(Text { text: self.text.clone()? })
    }
}

/// Where a domain's value came from (contract §8.3 "Model").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// cpd's compiled default.
    Default,
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
            Source::Owner(p) => write!(f, "owner:{}", p.display()),
            Source::Repo(p) => write!(f, "repo:{}", p.display()),
            Source::DefaultNoOwnerFile => f.write_str("default (no owner file)"),
        }
    }
}

/// A domain's source, and whether a typed key it leaves out took cpd's default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSource {
    pub source: Source,
    pub key_default: bool,
}

impl std::fmt::Display for DomainSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)?;
        if self.key_default {
            f.write_str(" (key default)")?;
        }
        Ok(())
    }
}

macro_rules! domains {
    ($($field:ident, $variant:ident, [$($role:ident),+], $value:ty, $set:ty;)+) => {
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
                    $(Domain::$variant => stringify!($field),)+
                }
            }

            pub fn from_name(name: &str) -> Option<Domain> {
                Domain::ALL.iter().copied().find(|d| d.name() == name)
            }
        }

        /// Who a domain's `text` reaches, fixed in code (contract §8.3).
        pub fn audience(domain: Domain) -> &'static [Role] {
            match domain {
                $(Domain::$variant => &[$(Role::$role),+],)+
            }
        }

        /// A policy: every domain has a value, always.
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Policy {
            $(pub $field: $value,)+
        }

        impl Policy {
            /// The `text` of `domain`.
            pub fn text(&self, domain: Domain) -> &str {
                match domain {
                    $(Domain::$variant => &self.$field.text,)+
                }
            }
        }

        /// A scope file: the domains it has a table for.
        #[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Scope {
            $(pub $field: Option<$set>,)+
        }

        impl Scope {
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
            let policy = Policy {
                $($field: match layers.iter().find_map(|(src, scope)| scope.$field.as_ref().map(|set| (src, set))) {
                    Some((src, set)) => {
                        let (value, key_default) = set.fill(&default.$field);
                        sources.0[Domain::$variant as usize] =
                            DomainSource { source: src.clone(), key_default };
                        value
                    }
                    None => default.$field.clone(),
                },)+
            };
            super::Resolved { policy, sources }
        }
    };
}

domains! {
    pr, Pr, [Lead, Worker], Pr, PrSet;
    deploy, Deploy, [Lead], Text, TextSet;
    code, Code, [Lead, Worker], Text, TextSet;
    tests, Tests, [Lead, Worker], Text, TextSet;
    monitor, Monitor, [Lead, Worker], Text, TextSet;
    instructions, Instructions, [Manager, Lead, Worker], Text, TextSet;
}

impl Sources {
    /// All sources with one value (the default, or a missing owner file).
    pub(super) fn all(source: Source) -> Sources {
        Sources(std::array::from_fn(|_| DomainSource { source: source.clone(), key_default: false }))
    }
}
