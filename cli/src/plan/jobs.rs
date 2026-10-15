//! A Plan's Jobs as read (contract §2.2 Job and Spec), the fingerprints of
//! §2.3, and the Dependency cycle check of `plan lock`.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::bad_row;
use crate::error::CpdError;
use crate::ids::PlanId;

/// The Job statuses of §3.2, in the order `counts` lists them.
pub const STATUSES: [&str; 6] = ["draft", "queued", "running", "merged", "failed", "cancelled"];

/// One Job with its Dependencies. The Spec arrays are kept as the JSON values
/// stored, so a fingerprint hashes exactly what the database holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub seq: i64,
    pub key: String,
    pub title: String,
    pub tier: String,
    pub goal: String,
    pub context: String,
    pub touches: Value,
    pub acceptance: Value,
    pub out_of_scope: Value,
    pub status: String,
    pub appended: bool,
    pub fingerprint: Option<String>,
    pub retry_tier: Option<String>,
    pub cancel_reason: Option<String>,
    pub cancel_requested_at: Option<String>,
    pub failed_cause: Option<String>,
    pub merged_sha: Option<String>,
    pub created_at: String,
    /// Keys of the Jobs it needs, sorted.
    pub needs: Vec<String>,
}

impl Job {
    pub fn is_draft(&self) -> bool {
        self.status == "draft"
    }

    /// The fields `plan show` prints for a Job (commands.md §4.3), without its Attempts.
    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key, "seq": self.seq, "title": self.title, "status": self.status, "tier": self.tier,
            "appended": self.appended, "needs": self.needs,
            "goal": self.goal, "context": self.context, "touches": self.touches,
            "acceptance": self.acceptance, "out_of_scope": self.out_of_scope,
            "fingerprint": self.fingerprint, "retry_tier": self.retry_tier,
            "cancel_reason": self.cancel_reason, "cancel_requested_at": self.cancel_requested_at,
            "failed_cause": self.failed_cause, "merged_sha": self.merged_sha, "created_at": self.created_at,
        })
    }
}

/// Every Job of `plan`, in creation order (`seq`).
pub fn load(conn: &Connection, plan: &PlanId) -> Result<Vec<Job>, CpdError> {
    let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT job, needs FROM dependencies WHERE plan = ?1 ORDER BY job, needs")?;
    for row in stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (job, need) = row?;
        needs.entry(job).or_default().push(need);
    }
    let mut stmt = conn.prepare(
        "SELECT seq, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                fingerprint, retry_tier, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at
           FROM jobs WHERE plan = ?1 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok((
                (r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?),
                (r.get::<_, String>(4)?, r.get::<_, String>(5)?),
                (r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?),
                (r.get::<_, String>(9)?, r.get::<_, bool>(10)?, r.get::<_, Option<String>>(11)?, r.get::<_, Option<String>>(12)?),
                (r.get::<_, Option<String>>(13)?, r.get::<_, Option<String>>(14)?, r.get::<_, Option<String>>(15)?),
                (r.get::<_, Option<String>>(16)?, r.get::<_, String>(17)?),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|((seq, key, title, tier), (goal, context), (touches, acceptance, out_of_scope), (status, appended, fingerprint, retry_tier), (cancel_reason, cancel_requested_at, failed_cause), (merged_sha, created_at))| {
            let array = |name: &str, text: &str| -> Result<Value, CpdError> {
                serde_json::from_str::<Value>(text)
                    .ok()
                    .filter(Value::is_array)
                    .ok_or_else(|| bad_row(format!("Job {plan}/{key} has {name} {text:?}")))
            };
            Ok(Job {
                touches: array("touches", &touches)?,
                acceptance: array("acceptance", &acceptance)?,
                out_of_scope: array("out_of_scope", &out_of_scope)?,
                needs: needs.remove(&key).unwrap_or_default(),
                seq,
                key,
                title,
                tier,
                goal,
                context,
                status,
                appended,
                fingerprint,
                retry_tier,
                cancel_reason,
                cancel_requested_at,
                failed_cause,
                merged_sha,
                created_at,
            })
        })
        .collect()
}

/// How many Jobs are in each status, every status listed.
pub fn counts(jobs: &[Job]) -> Value {
    let mut counts = serde_json::Map::new();
    for status in STATUSES {
        counts.insert(status.to_owned(), json!(jobs.iter().filter(|j| j.status == status).count()));
    }
    Value::Object(counts)
}

/// `sha256` of `value`'s canonical JSON: keys sorted, no whitespace (§2.3).
/// serde_json's map is sorted (the crate is built without `preserve_order`),
/// so its compact form is canonical.
fn sha256_of(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

/// The Locked-Job fingerprint (§2.3): `{goal, tier, context, touches,
/// acceptance, out_of_scope, needs}` with `needs` sorted. Stored at
/// `plan lock` and at append; recomputed by every engine step.
pub fn locked_fingerprint(job: &Job) -> String {
    let mut needs = job.needs.clone();
    needs.sort();
    sha256_of(&json!({
        "goal": job.goal, "tier": job.tier, "context": job.context, "touches": job.touches,
        "acceptance": job.acceptance, "out_of_scope": job.out_of_scope, "needs": needs,
    }))
}

/// The plan fingerprint (§2.3): the canonical JSON array of the Locked fields
/// (title, Tier, Spec and needs, with the key, as cpd 1 §11 `plan render`) of
/// every `draft` Job, ordered by key.
pub fn plan_fingerprint(jobs: &[Job]) -> String {
    let mut drafts: Vec<&Job> = jobs.iter().filter(|j| j.is_draft()).collect();
    drafts.sort_by(|a, b| a.key.cmp(&b.key));
    let array: Vec<Value> = drafts
        .into_iter()
        .map(|j| {
            let mut needs = j.needs.clone();
            needs.sort();
            json!({
                "key": j.key, "title": j.title, "tier": j.tier, "goal": j.goal, "context": j.context,
                "touches": j.touches, "acceptance": j.acceptance, "out_of_scope": j.out_of_scope, "needs": needs,
            })
        })
        .collect();
    sha256_of(&Value::Array(array))
}

/// The Locked Jobs whose stored fingerprint differs from the recomputed one:
/// the `fingerprint` entries of the tamper report (§4.3).
pub fn tampered(plan: &PlanId, jobs: &[Job]) -> Vec<Value> {
    jobs.iter()
        .filter_map(|j| {
            let stored = j.fingerprint.as_ref()?;
            let found = locked_fingerprint(j);
            (*stored != found).then(|| json!({"kind": "fingerprint", "plan": plan, "job": j.key, "expected": stored, "found": found}))
        })
        .collect()
}

/// A cycle among the Dependencies of the `draft` Jobs, as the keys along it
/// (the first repeated at the end), if there is one.
pub fn cycle(jobs: &[Job]) -> Option<Vec<String>> {
    let drafts: BTreeMap<&str, &Job> = jobs.iter().filter(|j| j.is_draft()).map(|j| (j.key.as_str(), j)).collect();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for start in drafts.keys() {
        let mut path: Vec<&str> = Vec::new();
        if let Some(cycle) = visit(start, &drafts, &mut done, &mut path) {
            return Some(cycle);
        }
    }
    None
}

fn visit<'a>(
    key: &'a str,
    drafts: &BTreeMap<&'a str, &'a Job>,
    done: &mut BTreeSet<&'a str>,
    path: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    if let Some(at) = path.iter().position(|k| *k == key) {
        let mut cycle: Vec<String> = path[at..].iter().map(|k| (*k).to_owned()).collect();
        cycle.push(key.to_owned());
        return Some(cycle);
    }
    if done.contains(key) {
        return None;
    }
    let job = drafts.get(key)?;
    path.push(key);
    for need in &job.needs {
        if let Some(cycle) = visit(need, drafts, done, path) {
            return Some(cycle);
        }
    }
    path.pop();
    done.insert(key);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(key: &str, status: &str, needs: &[&str]) -> Job {
        Job {
            seq: 1,
            key: key.into(),
            title: format!("Title {key}"),
            tier: "S2".into(),
            goal: "g".into(),
            context: "c".into(),
            touches: json!(["a"]),
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
            status: status.into(),
            appended: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            created_at: "t".into(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    #[test]
    fn the_locked_fingerprint_hashes_the_canonical_spec_with_needs_sorted() {
        let expected = format!(
            "{:x}",
            Sha256::digest(
                br#"{"acceptance":["b"],"context":"c","goal":"g","needs":["x","y"],"out_of_scope":[],"tier":"S2","touches":["a"]}"#
            )
        );
        assert_eq!(locked_fingerprint(&job("k", "draft", &["y", "x"])), expected);
        assert_eq!(locked_fingerprint(&job("k", "draft", &["x", "y"])), expected);
        let mut retitled = job("k", "queued", &["x", "y"]);
        retitled.title = "Other".into();
        assert_eq!(locked_fingerprint(&retitled), expected, "the title is not in the Locked-Job fingerprint");
    }

    #[test]
    fn the_plan_fingerprint_covers_the_drafts_by_key() {
        let a = job("a", "draft", &[]);
        let b = job("b", "draft", &["a"]);
        let gone = job("c", "cancelled", &[]);
        let base = plan_fingerprint(&[a.clone(), b.clone()]);
        assert_eq!(plan_fingerprint(&[b.clone(), gone, a.clone()]), base, "order and non-drafts do not count");
        let mut retitled = b.clone();
        retitled.title = "New".into();
        assert_ne!(plan_fingerprint(&[a.clone(), retitled]), base);
        let mut retiered = b;
        retiered.tier = "O3".into();
        assert_ne!(plan_fingerprint(&[a, retiered]), base);
        assert_eq!(plan_fingerprint(&[]), format!("{:x}", Sha256::digest(b"[]")));
    }

    #[test]
    fn finds_a_cycle_among_drafts_only() {
        assert_eq!(cycle(&[job("a", "draft", &[]), job("b", "draft", &["a"])]), None);
        let found = cycle(&[job("a", "draft", &["c"]), job("b", "draft", &["a"]), job("c", "draft", &["b"])]).unwrap();
        assert_eq!(found.first(), found.last());
        assert_eq!(found.len(), 4);
        assert_eq!(cycle(&[job("a", "draft", &["a"])]), Some(vec!["a".into(), "a".into()]));
        assert_eq!(cycle(&[job("a", "draft", &["b"]), job("b", "cancelled", &["a"])]), None);
    }

    #[test]
    fn counts_every_status() {
        let counts = counts(&[job("a", "draft", &[]), job("b", "draft", &[]), job("c", "merged", &[])]);
        assert_eq!(counts, json!({"draft": 2, "queued": 0, "running": 0, "merged": 1, "failed": 0, "cancelled": 0}));
    }
}
