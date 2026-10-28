//! The red tests of a `test` Job and what an `impl` Job may do to them (contract §2.2 "Job kinds", §7.5, §7.7).
//!
//! An `impl` Job names no tests of its own: its tests are the union of the tests of the `test` Jobs it needs directly,
//! and the files of those Jobs are guarded against its diff unless the Lead approved the path (`job approve-tests`).

use serde_json::Value;

use crate::plan::jobs::Job;

/// What an `impl` Job's `test` Jobs say, read from the Jobs as stored now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    /// The keys of the `test` Jobs the Job needs directly, in the Job's order.
    pub jobs: Vec<String>,
    /// The named tests of those Jobs, in order, each once.
    pub tests: Vec<String>,
    /// The `test_files` entries of those Jobs, in order, each once.
    pub files: Vec<String>,
    /// The paths the Lead approved the Job to change (`jobs.test_approved`).
    pub approved: Vec<String>,
}

/// A JSON array of text as a list; anything else is empty (the schema's CHECKs keep it an array).
fn texts(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

fn push_new(into: &mut Vec<String>, items: Vec<String>) {
    for item in items {
        if !into.contains(&item) {
            into.push(item);
        }
    }
}

/// The guard of `job`: its direct Dependencies of kind `test`. Empty for a Job that needs none.
pub fn of(jobs: &[Job], job: &Job) -> Guard {
    let mut guard = Guard { approved: texts(job.test_approved.as_ref()), ..Guard::default() };
    for need in &job.needs {
        let Some(test) = jobs.iter().find(|j| j.key == *need && j.kind == "test") else { continue };
        guard.jobs.push(test.key.clone());
        push_new(&mut guard.tests, texts(test.tests.as_ref()));
        push_new(&mut guard.files, texts(test.test_files.as_ref()));
    }
    guard
}

/// The exclusive resources the Job takes (its Spec's `resources`); empty when none.
pub fn resources(job: &Job) -> Vec<String> {
    texts(job.resources.as_ref())
}

/// A `test` Job's own named tests and files, as [`Guard`] holds a derived set.
pub fn own(job: &Job) -> (Vec<String>, Vec<String>) {
    (texts(job.tests.as_ref()), texts(job.test_files.as_ref()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn job(key: &str, kind: &str, needs: &[&str]) -> Job {
        Job {
            seq: 0,
            key: key.to_owned(),
            pr: "p".to_owned(),
            title: String::new(),
            tier: None,
            spec: None,
            status: "queued".to_owned(),
            appended: false,
            kind: kind.to_owned(),
            tests: None,
            test_files: None,
            test_approved: None,
            resources: None,
            urgent: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            created_at: String::new(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    #[test]
    fn an_impl_jobs_tests_are_the_union_of_the_test_jobs_it_needs_directly() {
        let mut a = job("a", "test", &[]);
        a.tests = Some(json!(["t::one", "t::two"]));
        a.test_files = Some(json!(["tests/a.rs"]));
        let mut b = job("b", "test", &[]);
        b.tests = Some(json!(["t::two", "t::three"]));
        b.test_files = Some(json!(["tests/"]));
        let docs = job("d", "docs", &[]);
        let mut i = job("i", "impl", &["a", "b", "d"]);
        i.test_approved = Some(json!(["tests/a.rs"]));
        let guard = of(&[a, b, docs, i.clone()], &i);
        assert_eq!(guard.jobs, ["a", "b"]);
        assert_eq!(guard.tests, ["t::one", "t::two", "t::three"]);
        assert_eq!(guard.files, ["tests/a.rs", "tests/"]);
        assert_eq!(guard.approved, ["tests/a.rs"]);
    }

    #[test]
    fn a_job_that_needs_no_test_job_has_an_empty_guard() {
        let i = job("i", "impl", &["d"]);
        assert_eq!(of(&[job("d", "docs", &[]), i.clone()], &i), Guard::default());
    }
}
