//! The `model` domain (contract §8.3 "Models"): the four Tier models `O`, `S`, `H`, `G`, each a model ref
//! `<provider>/<model-id>`; one provider per resolved table.

use std::path::Path;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Deserializer, Serialize};

use super::domain::{HasText, Set};
use super::load::policy_invalid;
use super::PolicyKey;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, PlanId, WorkflowKey};

/// The four resolved model refs of a policy: the `model` domain's value.
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
}

impl Models {
    /// The models of a resolved policy.
    pub fn of(policy: &super::Policy) -> Models {
        policy.model.clone()
    }
}

/// The models of the Job `job` of `plan`: its PR's repo key (contract §8.3 "Models" (5)). A Worker Attempt, a Job's
/// Tier check, the ladder and the `open` and `update` Jobs read them here. A key that does not resolve is its error.
pub fn models_of_job(conn: &rusqlite::Connection, home: &CpdHome, plan: &PlanId, job: &str) -> Result<Models, CpdError> {
    let held: Option<Option<String>> = conn.query_row("SELECT workflow FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), job], |r| r.get(0)).optional()?;
    // An explore Job has no PR: its models are the Plan's Lead's, from the Context's owner.
    if let Some(None) = held {
        let context: String = conn.query_row("SELECT context FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
        return models_of_context(conn, home, &context);
    }
    if held.is_none() {
        return Err(CpdError::new(crate::error::ErrorCode::JobNotFound, format!("No Job {job} in Plan {plan}.")).with("plan", plan.as_str()).with("job", job));
    }
    Ok(Models::of(&crate::pr::policy_of_job(conn, home, plan, job)?))
}

/// The models of Attempt `id` of `plan`: those of its Job.
pub fn models_of_attempt(conn: &rusqlite::Connection, home: &CpdHome, plan: &PlanId, id: &str) -> Result<Models, CpdError> {
    let job: String = conn.query_row("SELECT job FROM attempts WHERE plan = ?1 AND id = ?2", [plan.as_str(), id], |r| r.get(0))?;
    models_of_job(conn, home, plan, &job)
}

/// The models of the PR `key' of `plan`: [`models_of_job`] for a PR.
pub fn models_of_workflow(conn: &rusqlite::Connection, home: &CpdHome, plan: &PlanId, key: &WorkflowKey) -> Result<Models, CpdError> {
    Ok(Models::of(&crate::pr::policy_of_workflow(conn, home, plan, key)?))
}

/// The models of Context `context`: its owner's key, or, for a Context with no owner, the machine layer over the
/// built-in default. A Lead, a Manager, the title writer and the distiller read them here.
pub fn models_of_context(conn: &rusqlite::Connection, home: &CpdHome, context: &str) -> Result<Models, CpdError> {
    let owner: Option<String> = conn.query_row("SELECT owner FROM contexts WHERE id = ?1", [context], |r| r.get(0))?;
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
    if ok { Ok(s.to_owned()) } else { Err(format!("{s:?} is not <provider>/<model-id>")) }
}

fn ref_key<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    value.map(|v| model_ref(&v).map_err(serde::de::Error::custom)).transpose()
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
}

impl Set for ModelSet {
    type Value = Models;

    fn fill(&self, base: &Models) -> (Models, Vec<&'static str>) {
        let pick = |set: &Option<String>, base: &String| set.clone().unwrap_or_else(|| base.clone());
        let value = Models { o: pick(&self.o, &base.o), s: pick(&self.s, &base.s), h: pick(&self.h, &base.h), g: pick(&self.g, &base.g) };
        let left_out = [("O", self.o.is_none()), ("S", self.s.is_none()), ("H", self.h.is_none()), ("G", self.g.is_none())]
            .into_iter()
            .filter_map(|(name, out)| out.then_some(name))
            .collect();
        (value, left_out)
    }

    fn complete(&self) -> Option<Models> {
        Some(Models { o: self.o.clone()?, s: self.s.clone()?, h: self.h.clone()?, g: self.g.clone()? })
    }
}

/// The provider of a model ref: the text before its first `/`.
pub fn provider_of(model_ref: &str) -> &str {
    model_ref.split_once('/').map_or(model_ref, |(provider, _)| provider)
}

/// Refuses a table whose four refs do not share one provider: `policy_invalid`, `details.file` the
/// file that set the winning `[model]`, `details.key` `model`, the message naming the keys and providers.
pub fn check_one_provider(models: &Models, file: &Path) -> Result<(), CpdError> {
    let keys = [("O", &models.o), ("S", &models.s), ("H", &models.h), ("G", &models.g)];
    let (first, first_ref) = keys[0];
    let first_provider = provider_of(first_ref);
    match keys.iter().find(|(_, r)| provider_of(r) != first_provider) {
        None => Ok(()),
        Some((key, r)) => Err(policy_invalid(
            file,
            Some("model"),
            format!("{first} is {first_provider} but {key} is {}: a model table has one provider", provider_of(r)),
        )),
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
