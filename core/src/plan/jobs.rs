//! A Plan's Jobs as read (contract §2.2 Job and Spec), the fingerprints of
//! §2.3, and the Dependency cycle check of `plan lock`.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::CpdError;
use crate::ids::PlanId;

/// The Job statuses of §3.2, in the order `counts` lists them.
pub const STATUSES: [&str; 6] = [
    "draft",
    "queued",
    "running",
    "merged",
    "failed",
    "cancelled",
];

/// The kinds of Job (`jobs.kind`, §2.2 "Job kinds"): `open` and `update` are cpd's, the others the Lead's.
pub const KINDS: [&str; 9] = [
    "work", "docs", "test", "impl", "verify", "open", "update", "scout", "research",
];

/// The explore kinds (`scout`, `research`): Jobs of the Plan, not of a Workflow.
pub fn is_explore(kind: &str) -> bool {
    matches!(kind, "scout" | "research")
}

/// The Spec of a Job (§2.2): all five fields, or none (a stub, below). The
/// arrays are kept as the JSON values stored, so a fingerprint hashes exactly
/// what the database holds.
#[derive(Clone, Debug, PartialEq)]
pub struct JobSpec {
    pub goal: String,
    pub context: String,
    pub touches: Value,
    pub acceptance: Value,
    pub out_of_scope: Value,
}

/// One Job with its Dependencies. A **stub draft** (hs-d241, §2.2) has no Tier,
/// no Spec, or neither: only a `draft` or a `cancelled` Job can (§4.4 CHECK).
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub seq: i64,
    pub key: String,
    /// The key of the Workflow the Job belongs to (`jobs.pr`, §2.2).
    pub workflow: Option<String>,
    pub title: String,
    pub tier: Option<String>,
    pub spec: Option<JobSpec>,
    pub status: String,
    pub appended: bool,
    /// The Job's kind (`jobs.kind`, §2.2 "Job kinds"): one of [`KINDS`]; `work` for every Job before migration `job-kinds`.
    pub kind: String,
    /// A `test` Job's named tests (`jobs.tests`, a JSON array): set only with a Spec, and only for kind `test`.
    pub tests: Option<Value>,
    /// The files that hold them (`jobs.test_files`, a JSON array): set exactly when `tests` is.
    pub test_files: Option<Value>,
    /// The exclusive resources the Job takes (`jobs.resources`, a JSON array): Locked with the Job, in no fingerprint.
    pub resources: Option<Value>,
    /// An `impl` Job's approved red-test paths (`jobs.test_approved`): not a Locked field, in no fingerprint.
    pub test_approved: Option<Value>,
    /// The paths the Lead approved a Job to change beyond its Spec `touches` (`jobs.touches_approved`): not a Locked
    /// field, in no fingerprint (§2.2, §7.5).
    pub touches_approved: Option<Value>,
    /// The urgent flag (`jobs.urgent`, §2.2): not a Locked field, in no fingerprint.
    pub urgent: bool,
    pub fingerprint: Option<String>,
    pub retry_tier: Option<String>,
    pub cancel_reason: Option<String>,
    pub cancel_requested_at: Option<String>,
    pub failed_cause: Option<String>,
    pub merged_sha: Option<String>,
    /// An explore Job's bounded summary (`jobs.summary`): set exactly when an explore Job is `merged`.
    pub summary: Option<String>,
    pub created_at: String,
    /// Keys of the Jobs it needs, sorted.
    pub needs: Vec<String>,
}

impl Job {
    /// The key of the Job's Workflow; empty for a Job with none (an explore Job), which no Workflow key equals.
    pub fn workflow_str(&self) -> &str {
        self.workflow.as_deref().unwrap_or_default()
    }

    pub fn is_draft(&self) -> bool {
        self.status == "draft"
    }

    /// A draft that is under review (§2.3, hs-d573): every draft but a `draft` `open` Job, which is cpd's and no Job of the Lead's.
    pub fn is_under_review(&self) -> bool {
        self.is_draft() && self.kind != "open"
    }

    /// What a stub lacks (`details.jobs[].missing` of `draft_incomplete`, commands.md §4.6):
    /// `tier` and/or `spec`; empty for a Job that has both.
    pub fn missing(&self) -> Vec<&'static str> {
        [("tier", self.tier.is_none()), ("spec", self.spec.is_none())]
            .into_iter()
            .filter_map(|(name, lacks)| lacks.then_some(name))
            .collect()
    }

    /// The Tier, for what runs a Job: a Job that is not a draft always has it (§4.4 CHECK), so its absence is a bad row.
    pub fn require_tier(&self) -> Result<&str, CpdError> {
        self.tier
            .as_deref()
            .ok_or_else(|| CpdError::bad_row(format!("Job {} has no tier", self.key)))
    }

    /// The Spec, for what runs a Job, as [`Job::require_tier`].
    pub fn require_spec(&self) -> Result<&JobSpec, CpdError> {
        self.spec
            .as_ref()
            .ok_or_else(|| CpdError::bad_row(format!("Job {} has no Spec", self.key)))
    }

    /// `{goal, tier, context, touches, acceptance, out_of_scope}`: what the fingerprints of §2.3
    /// hash, JSON null for what a stub lacks (a stub is hashed as it is).
    pub fn locked_fields(&self) -> Map<String, Value> {
        let spec = self.spec.as_ref();
        let field = |f: fn(&JobSpec) -> Value| spec.map_or(Value::Null, f);
        let mut fields = Map::new();
        fields.insert("tier".into(), json!(self.tier));
        fields.insert("goal".into(), field(|s| json!(s.goal)));
        fields.insert("context".into(), field(|s| json!(s.context)));
        fields.insert("touches".into(), field(|s| s.touches.clone()));
        fields.insert("acceptance".into(), field(|s| s.acceptance.clone()));
        fields.insert("out_of_scope".into(), field(|s| s.out_of_scope.clone()));
        fields
    }

    /// The Spec fields a kind adds to the Locked fields of the plan fingerprint (§2.3): `kind` only when it is not
    /// `work`, `tests` and `test_files` only when set, so the fingerprint of a Job of no new kind is what it was.
    pub fn kind_fields(&self) -> Map<String, Value> {
        let mut fields = Map::new();
        if self.kind != "work" {
            fields.insert("kind".into(), json!(self.kind));
        }
        for (name, value) in [("tests", &self.tests), ("test_files", &self.test_files)] {
            if let Some(value) = value {
                fields.insert(name.into(), value.clone());
            }
        }
        fields
    }

    /// The fields `plan show` prints for a Job (commands.md §4.3), without its Attempts: `tests` and `test_files`
    /// of a `test` Job and `test_approved` of an `impl` Job only (§5.7, "where the kind has them").
    pub fn to_json(&self) -> Value {
        let spec = |f: fn(&JobSpec) -> Value| self.spec.as_ref().map_or(Value::Null, f);
        let mut value = json!({
            "key": self.key, "seq": self.seq, "pr": self.workflow, "title": self.title, "kind": self.kind, "status": self.status, "tier": self.tier,
            "appended": self.appended, "urgent": self.urgent, "needs": self.needs,
            "goal": spec(|s| json!(s.goal)), "context": spec(|s| json!(s.context)), "touches": spec(|s| s.touches.clone()),
            "acceptance": spec(|s| s.acceptance.clone()), "out_of_scope": spec(|s| s.out_of_scope.clone()),
            "resources": self.resources.clone().unwrap_or_else(|| json!([])),
            "fingerprint": self.fingerprint, "retry_tier": self.retry_tier,
            "cancel_reason": self.cancel_reason, "cancel_requested_at": self.cancel_requested_at,
            "failed_cause": self.failed_cause, "merged_sha": self.merged_sha, "created_at": self.created_at,
        });
        match self.kind.as_str() {
            "test" => {
                value["tests"] = self.tests.clone().unwrap_or(Value::Null);
                value["test_files"] = self.test_files.clone().unwrap_or(Value::Null);
            }
            "impl" => value["test_approved"] = self.test_approved.clone().unwrap_or(Value::Null),
            _ => {}
        }
        value
    }
}

/// Every Job of `plan`, in creation order (`seq`).
pub fn load(conn: &Connection, plan: &PlanId) -> Result<Vec<Job>, CpdError> {
    let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt =
        conn.prepare("SELECT job, needs FROM dependencies WHERE plan = ?1 ORDER BY job, needs")?;
    for row in stmt.query_map([plan.as_str()], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (job, need) = row?;
        needs.entry(job).or_default().push(need);
    }
    let mut stmt = conn.prepare(
        "SELECT seq, key, workflow, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                fingerprint, retry_tier, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at, urgent,
                kind, tests, test_files, test_approved, resources, summary, touches_approved
           FROM jobs WHERE plan = ?1 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok((
                (
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ),
                (
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ),
                (
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                ),
                (
                    r.get::<_, String>(10)?,
                    r.get::<_, bool>(11)?,
                    r.get::<_, Option<String>>(12)?,
                    r.get::<_, Option<String>>(13)?,
                ),
                (
                    r.get::<_, Option<String>>(14)?,
                    r.get::<_, Option<String>>(15)?,
                    r.get::<_, Option<String>>(16)?,
                ),
                (
                    r.get::<_, Option<String>>(17)?,
                    r.get::<_, String>(18)?,
                    r.get::<_, bool>(19)?,
                ),
                (
                    r.get::<_, String>(20)?,
                    r.get::<_, Option<String>>(21)?,
                    r.get::<_, Option<String>>(22)?,
                    r.get::<_, Option<String>>(23)?,
                    r.get::<_, Option<String>>(24)?,
                    r.get::<_, Option<String>>(25)?,
                    r.get::<_, Option<String>>(26)?,
                ),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|((seq, key, workflow, title, tier), (goal, context), (touches, acceptance, out_of_scope), (status, appended, fingerprint, retry_tier), (cancel_reason, cancel_requested_at, failed_cause), (merged_sha, created_at, urgent), (kind, tests, test_files, test_approved, resources, summary, touches_approved))| {
            let array = |name: &str, text: &str| -> Result<Value, CpdError> {
                serde_json::from_str::<Value>(text)
                    .ok()
                    .filter(Value::is_array)
                    .ok_or_else(|| CpdError::bad_row(format!("Job {plan}/{key} has {name} {text:?}")))
            };
            // The Spec is all or nothing (§4.4 CHECK).
            let spec = match (goal, context, touches, acceptance, out_of_scope) {
                (Some(goal), Some(context), Some(touches), Some(acceptance), Some(out_of_scope)) => Some(JobSpec {
                    goal,
                    context,
                    touches: array("touches", &touches)?,
                    acceptance: array("acceptance", &acceptance)?,
                    out_of_scope: array("out_of_scope", &out_of_scope)?,
                }),
                (None, None, None, None, None) => None,
                _ => return Err(CpdError::bad_row(format!("Job {plan}/{key} has some of its Spec fields and not all"))),
            };
            let optional = |name: &str, text: Option<String>| text.map(|t| array(name, &t)).transpose();
            Ok(Job {
                tests: optional("tests", tests)?,
                test_files: optional("test_files", test_files)?,
                test_approved: optional("test_approved", test_approved)?,
                resources: optional("resources", resources)?,
                touches_approved: optional("touches_approved", touches_approved)?,
                kind,
                spec,
                needs: needs.remove(&key).unwrap_or_default(),
                seq,
                key,
                workflow,
                title,
                tier,
                status,
                appended,
                urgent,
                fingerprint,
                retry_tier,
                cancel_reason,
                cancel_requested_at,
                failed_cause,
                merged_sha,
                created_at,
                summary,
            })
        })
        .collect()
}

/// How many Jobs are in each status, every status listed.
pub fn counts(jobs: &[Job]) -> Value {
    let mut counts = serde_json::Map::new();
    for status in STATUSES {
        counts.insert(
            status.to_owned(),
            json!(jobs.iter().filter(|j| j.status == status).count()),
        );
    }
    Value::Object(counts)
}

/// `sha256` of `value`'s canonical JSON: keys sorted, no whitespace (§2.3).
/// serde_json's map is sorted (the crate is built without `preserve_order`),
/// so its compact form is canonical.
pub(super) fn sha256_of(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

/// The Locked-Job fingerprint (§2.3): `{goal, tier, context, touches,
/// acceptance, out_of_scope, needs}` with `needs` sorted. Stored at
/// `plan lock` and at append; recomputed by every engine step.
pub fn locked_fingerprint(job: &Job) -> String {
    let mut needs = job.needs.clone();
    needs.sort();
    let mut fields = job.locked_fields();
    fields.insert("needs".into(), json!(needs));
    sha256_of(&Value::Object(fields))
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
    let drafts: BTreeMap<&str, &Job> = jobs
        .iter()
        .filter(|j| j.is_draft())
        .map(|j| (j.key.as_str(), j))
        .collect();
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
            workflow: Some("web".into()),
            title: format!("Title {key}"),
            tier: Some("S2".into()),
            spec: Some(JobSpec {
                goal: "g".into(),
                context: "c".into(),
                touches: json!(["a"]),
                acceptance: json!(["b"]),
                out_of_scope: json!([]),
            }),
            status: status.into(),
            appended: false,
            kind: "work".into(),
            tests: None,
            test_files: None,
            resources: None,
            test_approved: None,
            touches_approved: None,
            urgent: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            summary: None,
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
        assert_eq!(
            locked_fingerprint(&job("k", "draft", &["y", "x"])),
            expected
        );
        assert_eq!(
            locked_fingerprint(&job("k", "draft", &["x", "y"])),
            expected
        );
        let mut retitled = job("k", "queued", &["x", "y"]);
        retitled.title = "Other".into();
        assert_eq!(
            locked_fingerprint(&retitled),
            expected,
            "the title is not in the Locked-Job fingerprint"
        );
    }

    #[test]
    fn finds_a_cycle_among_drafts_only() {
        assert_eq!(
            cycle(&[job("a", "draft", &[]), job("b", "draft", &["a"])]),
            None
        );
        let found = cycle(&[
            job("a", "draft", &["c"]),
            job("b", "draft", &["a"]),
            job("c", "draft", &["b"]),
        ])
        .unwrap();
        assert_eq!(found.first(), found.last());
        assert_eq!(found.len(), 4);
        assert_eq!(
            cycle(&[job("a", "draft", &["a"])]),
            Some(vec!["a".into(), "a".into()])
        );
        assert_eq!(
            cycle(&[job("a", "draft", &["b"]), job("b", "cancelled", &["a"])]),
            None
        );
    }

    #[test]
    fn counts_every_status() {
        let counts = counts(&[
            job("a", "draft", &[]),
            job("b", "draft", &[]),
            job("c", "merged", &[]),
        ]);
        assert_eq!(
            counts,
            json!({"draft": 2, "queued": 0, "running": 0, "merged": 1, "failed": 0, "cancelled": 0})
        );
    }
}
