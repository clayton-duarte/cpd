//! The `model` domain (contract §8.3 "Models"): the five Tier models `O`, `S`, `H`, `G`, `J`, each a model ref
//! `<provider>/<model-id>`; one provider per resolved table.

use std::path::Path;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Deserializer, Serialize};

use super::PolicyKey;
use super::domain::{HasText, Set};
use super::load::policy_invalid;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, PlanId, WorkflowKey};

/// The five resolved model refs of a policy: the `model` domain's value.
///
/// The resolved policy hands them out through [`Models::of`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Models {
    #[serde(rename = "O")]
    pub o: String,
    #[serde(rename = "S")]
    pub s: String,
    #[serde(rename = "H")]
    pub h: String,
    #[serde(rename = "G")]
    pub g: String,
    /// The decide role's letter (contract §8.3 "Models", §9 "Roles and ranges").
    #[serde(rename = "J")]
    pub j: String,
}

impl Models {
    /// The models of a resolved policy.
    pub fn of(policy: &super::Policy) -> Models {
        policy.model.clone()
    }
}

/// The models of the Job `job` of `plan`: its Workflow's repo key (contract §8.3 "Models" (5)). A Worker Attempt, a Job's
/// Tier check, the ladder and the `open` and `update` Jobs read them here. A key that does not resolve is its error.
pub fn models_of_job(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    plan: &PlanId,
    job: &str,
) -> Result<Models, CpdError> {
    let held: Option<Option<String>> = conn
        .query_row(
            "SELECT workflow FROM jobs WHERE plan = ?1 AND key = ?2",
            [plan.as_str(), job],
            |r| r.get(0),
        )
        .optional()?;
    // An explore Job has no Workflow: its models are the Plan's Lead's, from the Context's owner.
    if let Some(None) = held {
        let context: String = conn.query_row(
            "SELECT context FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| r.get(0),
        )?;
        return models_of_context(conn, home, &context);
    }
    if held.is_none() {
        return Err(CpdError::new(
            crate::error::ErrorCode::JobNotFound,
            format!("No Job {job} in Plan {plan}."),
        )
        .with("plan", plan.as_str())
        .with("job", job));
    }
    Ok(Models::of(&crate::pr::policy_of_job(
        conn, home, plan, job,
    )?))
}

/// The models of Attempt `id` of `plan`: those of its Job.
pub fn models_of_attempt(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    plan: &PlanId,
    id: &str,
) -> Result<Models, CpdError> {
    let job: String = conn.query_row(
        "SELECT job FROM attempts WHERE plan = ?1 AND id = ?2",
        [plan.as_str(), id],
        |r| r.get(0),
    )?;
    models_of_job(conn, home, plan, &job)
}

/// The models of the Workflow `key' of `plan`: [`models_of_job`] for a Workflow.
pub fn models_of_workflow(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<Models, CpdError> {
    Ok(Models::of(&crate::pr::policy_of_workflow(
        conn, home, plan, key,
    )?))
}

/// The models of Context `context`: its owner's key, or, for a Context with no owner, the machine layer over the
/// built-in default. A Lead, a Manager, the title writer and the distiller read them here.
pub fn models_of_context(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    context: &str,
) -> Result<Models, CpdError> {
    let owner: Option<String> =
        conn.query_row("SELECT owner FROM contexts WHERE id = ?1", [context], |r| {
            r.get(0)
        })?;
    let resolved = match owner {
        Some(owner) => super::resolve(home, &PolicyKey::Owner(OwnerLogin::parse(&owner)?))?,
        None => super::resolve_machine(home)?,
    };
    Ok(Models::of(&resolved.policy))
}

impl HasText for Models {
    fn text(&self) -> &str {
        ""
    }
}

/// `^[^/\s]+/\S+$`: `<provider>/<model-id>`.
pub fn model_ref(s: &str) -> Result<String, String> {
    let ok = s.split_once('/').is_some_and(|(provider, id)| {
        !provider.is_empty()
            && !provider.chars().any(char::is_whitespace)
            && !id.is_empty()
            && !id.chars().any(char::is_whitespace)
    });
    if ok {
        Ok(s.to_owned())
    } else {
        Err(format!("{s:?} is not <provider>/<model-id>"))
    }
}

fn ref_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    value
        .map(|v| model_ref(&v).map_err(serde::de::Error::custom))
        .transpose()
}

/// What a scope file's `[model]` table sets.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSet {
    #[serde(rename = "O", default, deserialize_with = "ref_key")]
    pub o: Option<String>,
    #[serde(rename = "S", default, deserialize_with = "ref_key")]
    pub s: Option<String>,
    #[serde(rename = "H", default, deserialize_with = "ref_key")]
    pub h: Option<String>,
    #[serde(rename = "G", default, deserialize_with = "ref_key")]
    pub g: Option<String>,
    #[serde(rename = "J", default, deserialize_with = "ref_key")]
    pub j: Option<String>,
}

impl Set for ModelSet {
    type Value = Models;

    fn fill(&self, base: &Models) -> (Models, Vec<&'static str>) {
        let pick =
            |set: &Option<String>, base: &String| set.clone().unwrap_or_else(|| base.clone());
        let g = pick(&self.g, &base.g);
        // No J in the winning table: J is the resolved G, never the default's J.
        let j = self.j.clone().unwrap_or_else(|| g.clone());
        let value = Models {
            o: pick(&self.o, &base.o),
            s: pick(&self.s, &base.s),
            h: pick(&self.h, &base.h),
            g,
            j,
        };
        let left_out = [
            ("O", self.o.is_none()),
            ("S", self.s.is_none()),
            ("H", self.h.is_none()),
            ("G", self.g.is_none()),
            ("J", self.j.is_none()),
        ]
        .into_iter()
        .filter_map(|(name, out)| out.then_some(name))
        .collect();
        (value, left_out)
    }

    fn complete(&self) -> Option<Models> {
        let g = self.g.clone()?;
        Some(Models {
            o: self.o.clone()?,
            s: self.s.clone()?,
            h: self.h.clone()?,
            j: self.j.clone().unwrap_or_else(|| g.clone()),
            g,
        })
    }
}

/// The provider of a model ref: the text before its first `/`.
pub fn provider_of(model_ref: &str) -> &str {
    model_ref
        .split_once('/')
        .map_or(model_ref, |(provider, _)| provider)
}

/// Refuses a table whose five refs do not share one provider: `policy_invalid`, `details.file` the
/// file that set the winning `[model]`, `details.key` `model`, the message naming the keys and providers.
pub fn check_one_provider(models: &Models, file: &Path) -> Result<(), CpdError> {
    let keys = [
        ("O", &models.o),
        ("S", &models.s),
        ("H", &models.h),
        ("G", &models.g),
        ("J", &models.j),
    ];
    let (first, first_ref) = keys[0];
    let first_provider = provider_of(first_ref);
    match keys.iter().find(|(_, r)| provider_of(r) != first_provider) {
        None => Ok(()),
        Some((key, r)) => Err(policy_invalid(
            file,
            Some("model"),
            format!(
                "{first} is {first_provider} but {key} is {}: a model table has one provider",
                provider_of(r)
            ),
        )),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectPolicy {
    provider: String,
    manager: super::RoleSelection,
    lead: super::RoleSelection,
    worker: super::RoleSelection,
}

pub(super) fn validate_direct(
    roles: &super::RoleSelections,
    owner: &crate::ids::OwnerLogin,
    capabilities: &dyn super::SelectionCapabilities,
) -> Result<(), String> {
    let mut provider = None;
    for role in [&roles.manager, &roles.lead, &roles.worker] {
        let (p, m) = role.model.split_once('/').ok_or("invalid model")?;
        if p.is_empty()
            || m.is_empty()
            || p.contains(char::is_whitespace)
            || m.contains(char::is_whitespace)
            || p.eq_ignore_ascii_case("copilot")
            || provider.is_some_and(|v| v != p)
        {
            return Err("invalid or mixed provider".into());
        }
        provider = Some(p);
        if owner.as_str() == "missionlane" && p != "bifrost" {
            return Err("missionlane requires bifrost".into());
        }
        match capabilities.evidence(&role.model, role.effort) {
            Ok(true) => {}
            Ok(false) => {
                return Err(format!(
                    "unsupported model effort: {} {}",
                    role.model, role.effort
                ));
            }
            Err(e) => return Err(format!("Pi capability: {e:?}")),
        }
    }
    if roles.worker.model == roles.lead.model && roles.worker.effort >= roles.lead.effort {
        return Err("worker effort must be below lead effort".into());
    }
    Ok(())
}

fn direct_file(path: &std::path::Path) -> Result<Option<DirectPolicy>, crate::error::CpdError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(crate::error::CpdError::new(
                crate::error::ErrorCode::FileUnreadable,
                format!("Cannot read {}: {e}", path.display()),
            ));
        }
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| super::load::policy_invalid(path, None, e))?;
    table
        .get("role_selection")
        .map(|v| {
            v.clone()
                .try_into()
                .map_err(|e| super::load::policy_invalid(path, Some("role_selection"), e))
        })
        .transpose()
}

pub(super) fn resolve_direct(
    home: &crate::home::CpdHome,
    key: &super::PolicyKey,
    capabilities: &dyn super::SelectionCapabilities,
) -> Result<super::RoleSelections, crate::error::CpdError> {
    let dir = super::resolve::policy_dir(home);
    let host = dir.join("host.toml");
    if direct_file(&host)?.is_some() {
        return Err(super::load::policy_invalid(
            &host,
            Some("role_selection"),
            "not allowed at host scope",
        ));
    }
    if let super::PolicyKey::Repo(owner, repo) = key {
        let path = super::resolve::repo_path(home, owner, repo);
        if direct_file(&path)?.is_some() {
            return Err(super::load::policy_invalid(
                &path,
                Some("role_selection"),
                "not allowed at repo scope",
            ));
        }
    }
    let owner_path = super::resolve::owner_path(home, key.owner());
    let owner = direct_file(&owner_path)?;
    if key.owner().as_str() == "missionlane" && owner.is_none() {
        return Err(super::load::policy_invalid(
            &owner_path,
            Some("role_selection"),
            "missionlane requires explicit roles",
        ));
    }
    let shared_path = dir.join("default.toml");
    let (mut roles, shared) = if let Some(p) = owner {
        (direct_roles(p, &owner_path)?, false)
    } else if let Some(p) = direct_file(&shared_path)? {
        (direct_roles(p, &shared_path)?, true)
    } else {
        (
            super::RoleSelections {
                manager: super::RoleSelection {
                    model: "openai/gpt-6.1-sol".into(),
                    effort: 2,
                },
                lead: super::RoleSelection {
                    model: "openai/gpt-6.1-terra".into(),
                    effort: 2,
                },
                worker: super::RoleSelection {
                    model: "openai/gpt-6-luna".into(),
                    effort: 0,
                },
            },
            true,
        )
    };
    // The approved OpenAI shared worker permits effort 1 only on explicit evidence that 0 is unsupported.
    if roles.worker.model == "openai/gpt-6-luna"
        && roles.worker.effort == 0
        && shared
        && capabilities.evidence(&roles.worker.model, 0) == Ok(false)
    {
        roles.worker.effort = 1;
    }
    validate_direct(&roles, key.owner(), capabilities)
        .map_err(|e| super::load::policy_invalid(&owner_path, Some("role_selection"), e))?;
    Ok(roles)
}

fn direct_roles(
    p: DirectPolicy,
    path: &std::path::Path,
) -> Result<super::RoleSelections, crate::error::CpdError> {
    let roles = super::RoleSelections {
        manager: p.manager,
        lead: p.lead,
        worker: p.worker,
    };
    if [&roles.manager, &roles.lead, &roles.worker]
        .iter()
        .any(|r| {
            r.model
                .split_once('/')
                .is_none_or(|(provider, _)| provider != p.provider)
        })
    {
        return Err(super::load::policy_invalid(
            path,
            Some("role_selection.provider"),
            "provider does not match roles",
        ));
    }
    Ok(roles)
}
