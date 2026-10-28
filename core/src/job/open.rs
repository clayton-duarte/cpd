//! The `open` Job (contract §2.2 "Job kinds", §3.7 E4, hs-d340): the last Job of a started PR, which cpd adds and the Lead never does.
//! Its Spec is fixed text compiled into the binary and rendered from the PR's repo, base and Lead Branch, so nothing in a repo changes
//! it; it has no Dependency rows (its Ready rule is derived, [`waiting_for`]); its fingerprint is stored at the insert.

use rusqlite::params;
use serde_json::{Value, json};

use crate::config::Tier;
use crate::error::CpdError;
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::plan::PrRow;
use crate::plan::jobs::{Job, JobSpec, locked_fingerprint};
use crate::store::WriteTxn;

/// `job-added`'s `by` of an `open` Job: engine E4.
pub const BY_ENGINE: &str = "engine";

/// `job-added`'s `by` of the opener stub `pr add` lays out (commands.md §9.4, hs-d573).
pub const BY_TEMPLATE: &str = "pr add";

/// The title of a Job cpd adds to a PR: the PR's own title, else its key (a PR that existed before migration `pr-title` has none).
fn pr_title(pr: &PrRow) -> &str {
    pr.title.as_deref().unwrap_or(pr.key.as_str())
}

/// The key of a PR's `open` or `update` Job: the first free of `<prefix>-<pr>`, `<prefix>-<pr>-2`, … (`open` or `up`; at most 32
/// characters, the PR key having at most 24).
fn free_key(taken: &[String], prefix: &str, pr: &PrRow) -> Result<JobKey, CpdError> {
    let stem = format!("{prefix}-{}", pr.key);
    (1u32..)
        .map(|n| if n == 1 { stem.clone() } else { format!("{stem}-{n}") })
        .find(|key| !taken.contains(key))
        .ok_or_else(|| CpdError::bad_row(format!("PR {} has no free {prefix} Job key", pr.key)))
        .and_then(|key| JobKey::parse(&key))
}

/// The fixed Spec of an `open` Job of `pr` (§2.2 "Job kinds", §3.4 A3).
pub(crate) fn spec(pr: &PrRow) -> JobSpec {
    let (base, lead) = (&pr.base, &pr.lead_branch);
    let remote = format!("origin/{base}");
    if let Some(head_ref) = &pr.head_ref {
        return adopted_spec(pr, head_ref);
    }
    JobSpec {
        goal: format!(
            "Make the Lead Branch {lead} of PR {key} fit its base {base}, so that cpd can gate it, push it and open the pull request: merge {remote} into it only if the two conflict, resolve the conflicts, and submit.",
            key = pr.key
        ),
        context: format!(
            "Every other Job of PR {key} is merged or cancelled: its work is on {lead}, in the repo {repo}, and you work in a worktree cut from it. \
             You push nothing, open nothing and run no Gate: when your submit passes, cpd runs the Gate in a test slot, pushes {lead} and opens the pull request, ready, never a draft.\n\n\
             1. `git fetch origin {base}`.\n\
             2. Test for a conflict with `git merge-tree --write-tree --name-only {remote} HEAD`: exit 0 means none, and a base that merely moved ahead is no reason to merge it; exit 1 lists the conflicted files.\n\
             3. Only on a conflict: `git merge --no-edit {remote}`, resolve every conflict keeping both sides' intent, and commit the merge with what it needs to build and pass the Gate (recomputed hashes, tests adapted to the base's new rules, a renumbered migration).\n\
             4. Run the quick check and `cpd attempt submit` for your Attempt (the Worker Spec gives its id). With no conflict there is nothing to commit: submit as it is.\n\n\
             A refused submit says what to fix: `base_conflict` (merge the base, as above), `lead_branch_moved` (a Job merged into {lead} since you began: merge it, `git merge --no-edit {lead}`), `gate_failed` (fix what the Gate reports, commit, submit again), `gate_error` or `pr_closed` (ask the Lead).",
            key = pr.key,
            repo = pr.repo,
        ),
        touches: json!([format!("The files {remote} conflicts with {lead} in, and what the merge needs in order to build and pass the Gate")]),
        acceptance: json!([
            format!("`cpd attempt submit` succeeds: {lead} has no conflict with {remote} and the Gate, which cpd runs at submit, passes"),
            "Nothing was pushed by the Worker and no pull request was created or edited by it",
        ]),
        out_of_scope: json!([
            "Pushing, `gh`, running the Gate or opening the pull request yourself: cpd does them",
            format!("Merging {remote} when it does not conflict"),
            "Features, refactors or fixes that the conflict does not need",
        ]),
    }
}

/// The fixed Spec of an `open` Job of an adopted PR: the pull request exists, and cpd updates it by pushing to its head branch.
fn adopted_spec(pr: &PrRow, head_ref: &str) -> JobSpec {
    let mut spec = update_spec(pr);
    spec.goal = format!(
        "Make the Lead Branch {lead} of PR {key} fit its base {base}, so that cpd can gate it and push it to {head_ref}, the head of the pull request that exists already: merge origin/{base} into it only if the two conflict, resolve the conflicts, and submit.",
        lead = pr.lead_branch,
        key = pr.key,
        base = pr.base,
    );
    spec.context = format!(
        "The pull request of PR {key} exists already and is adopted: it is updated, never created. Its head is {head_ref} on origin; {lead}, in the repo {repo}, is the local branch cpd pushes to it, and you work in a worktree cut from it.\n\n{rest}",
        key = pr.key,
        lead = pr.lead_branch,
        repo = pr.repo,
        rest = spec.context.split_once("\n\n").map_or("", |(_, rest)| rest),
    );
    spec
}

/// The fixed Spec of an `update` Job of `pr` (§2.2 "Job kinds", §3.4 A3): the open Job's work for a pull request that exists.
fn update_spec(pr: &PrRow) -> JobSpec {
    let (base, lead) = (&pr.base, &pr.lead_branch);
    let remote = format!("origin/{base}");
    JobSpec {
        goal: format!(
            "Bring the pull request of PR {key} up to date with its base {base}, so that cpd can gate the Lead Branch {lead} and push it: merge {remote} into it only if the two conflict, resolve the conflicts, and submit.",
            key = pr.key
        ),
        context: format!(
            "The pull request of PR {key} exists already: {lead}, in the repo {repo}, is its head, and you work in a worktree cut from it. Either GitHub reports a conflict between it and {base}, or fix Jobs of the PR have merged into {lead} and only have to reach the pull request (then there may be nothing to merge). \
             You push nothing, open nothing and run no Gate: when your submit passes, cpd runs the Gate in a test slot and pushes {lead}.\n\n\
             1. `git fetch origin {base}`.\n\
             2. Test for a conflict with `git merge-tree --write-tree --name-only {remote} HEAD`: exit 0 means none, and a base that merely moved ahead is no reason to merge it; exit 1 lists the conflicted files.\n\
             3. Only on a conflict: `git merge --no-edit {remote}`, resolve every conflict keeping both sides' intent, and commit the merge with what it needs to build and pass the Gate (recomputed hashes, tests adapted to the base's new rules, a renumbered migration).\n\
             4. Run the quick check and `cpd attempt submit` for your Attempt (the Worker Spec gives its id). With no conflict there is nothing to commit: submit as it is.\n\n\
             A refused submit says what to fix: `base_conflict` (merge the base, as above), `lead_branch_moved` (a Job merged into {lead} since you began: merge it, `git merge --no-edit {lead}`), `gate_failed` (fix what the Gate reports, commit, submit again), `gate_error` or `pr_closed` (ask the Lead).",
            key = pr.key,
            repo = pr.repo,
        ),
        touches: json!([format!("The files {remote} conflicts with {lead} in, and what the merge needs in order to build and pass the Gate")]),
        acceptance: json!([
            format!("`cpd attempt submit` succeeds: {lead} has no conflict with {remote} and the Gate, which cpd runs at submit, passes"),
            "Nothing was pushed by the Worker and no pull request was created or edited by it",
        ]),
        out_of_scope: json!([
            "Pushing, `gh`, running the Gate or editing the pull request yourself: cpd does them",
            format!("Merging {remote} when it does not conflict"),
            "Features, refactors or fixes that the conflict does not need",
        ]),
    }
}

/// Inserts PR `pr`'s `open` Job in `tx` (§3.7 E4): `queued` at `tier`, `appended`, `urgent` 0, fixed Spec, no Dependency rows, its
/// fingerprint stored at once, with `job-added` (`kind` `open`, `by` the caller's) in the same transaction. Returns its key.
pub fn add(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrRow, tier: Tier, draft: bool, by: &str) -> Result<JobKey, CpdError> {
    insert(tx, plan, pr, tier, "open", by, None, draft)
}

/// Why an `update` Job is queued (`job-added`'s `reason`, §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The poll saw the pull request conflict with its base.
    Conflict,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Conflict => "conflict",
        }
    }
}

/// Queues PR `pr`'s `update` Job in `tx` (§2.2 "Job kinds", §3.7 E5), unless the PR has one `queued` or `running`: one transaction writes the
/// cause and the Job, so a cause is told once. `by` is `job-added`'s `by` (`poll`, `job add`, `plan review`). Returns the key of the Job it
/// inserted, none when one was live.
pub fn queue_update(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrRow, tier: Tier, by: &str, cause: Cause) -> Result<Option<JobKey>, CpdError> {
    // A conflict-driven update is the poll's own work: a held Plan queues none (hs-d643, contract §3.7 E5).
    if cause == Cause::Conflict && crate::incident::is_held(tx, plan) {
        return Ok(None);
    }
    let live: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND pr = ?2 AND kind = 'update' AND status IN ('queued', 'running'))",
        [plan.as_str(), pr.key.as_str()],
        |r| r.get(0),
    )?;
    if live {
        return Ok(None);
    }
    insert(tx, plan, pr, tier, "update", by, Some(cause.as_str()), false).map(Some)
}

/// The insert of an `open` or `update` Job: the same row, Spec apart, with `job-added` in the same transaction.
fn insert(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrRow, tier: Tier, kind: &str, by: &str, reason: Option<&str>, draft: bool) -> Result<JobKey, CpdError> {
    let taken: Vec<String> = {
        let mut stmt = tx.prepare("SELECT key FROM jobs WHERE plan = ?1")?;
        stmt.query_map([plan.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let update = kind == "update";
    let key = free_key(&taken, if update { "up" } else { "open" }, pr)?;
    let tier = tier.to_string();
    let title = if update { format!("Bring the pull request up to date: {}", pr_title(pr)) } else { format!("Open the pull request: {}", pr_title(pr)) };
    let mut job = Job {
        seq: 0,
        key: key.as_str().to_owned(),
        pr: pr.key.as_str().to_owned(),
        title,
        tier: Some(tier.clone()),
        spec: Some(if update { update_spec(pr) } else { spec(pr) }),
        status: if draft { "draft" } else { "queued" }.to_owned(),
        appended: true,
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
        needs: Vec::new(),
    };
    job.fingerprint = (!draft).then(|| locked_fingerprint(&job));
    let spec = job.spec.as_ref().expect("the open Job has its Spec");
    let array = |v: &Value| v.to_string();
    tx.execute(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, kind, created_txn, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?13, 1, ?11, ?12, cpd_txn(), (SELECT at FROM txns WHERE id = cpd_txn()))",
        params![
            plan.as_str(),
            job.pr,
            job.key,
            job.title,
            tier,
            spec.goal,
            spec.context,
            array(&spec.touches),
            array(&spec.acceptance),
            array(&spec.out_of_scope),
            job.fingerprint,
            kind,
            job.status,
        ],
    )?;
    let text = if draft {
        format!(
            "Job {key} added to Plan {plan} by {by} as a draft: PR {pr} has a draft Job, so it is queued with the PR's drafts, or when none remains. \
             It merges the base only on a conflict, and cpd gates, pushes and opens PR {pr}'s pull request when it is done. Next: cpd job show {key} --plan {plan}",
            pr = pr.key
        )
    } else if update {
        format!(
            "Job {key} added to Plan {plan} by {by} ({why}): it merges the base only on a conflict, and cpd gates and pushes PR {pr}'s Lead Branch when it is done. \
             It waits for every other Job of the PR. Next: cpd job show {key} --plan {plan}",
            why = reason.unwrap_or("update"),
            pr = pr.key
        )
    } else {
        format!(
            "Job {key} added to Plan {plan} by {by}: it merges the base only on a conflict, and cpd gates, pushes and opens PR {pr}'s pull request when it is done. \
             It waits for every other Job of the PR. Next: cpd job show {key} --plan {plan}",
            pr = pr.key
        )
    };
    append(tx, NewEvent::job_added_by_cpd(text, pr.key.as_str(), kind, &job.status, &tier, by, reason).plan(plan.clone()).job(key.clone()))?;
    Ok(key)
}

/// The other Jobs of `job`'s PR that its Ready rule counts (§3.2): every Job but the `open` and `update` ones, which do not count each other.
fn others<'a>(all: &'a [Job], job: &'a Job) -> impl Iterator<Item = &'a Job> {
    all.iter().filter(move |j| j.pr == job.pr && j.key != job.key && !matches!(j.kind.as_str(), "open" | "update"))
}

/// What an `open` or `update` Job waits for (§3.2: they have no Dependency rows, so this is their needs, derived each time it is read):
/// for an `open` Job the keys of the PR's other Jobs that are neither `merged` nor `cancelled`; for an `update` Job those that are `queued`
/// or `running` (a `draft` or `failed` Job does not hold it back). `None` for a Job of another kind, whose Dependency rows are its needs.
pub fn unfinished(all: &[Job], job: &Job) -> Option<Vec<String>> {
    match job.kind.as_str() {
        "open" => Some(others(all, job).filter(|j| !matches!(j.status.as_str(), "merged" | "cancelled")).map(|j| j.key.clone()).collect()),
        "update" => Some(others(all, job).filter(|j| matches!(j.status.as_str(), "queued" | "running")).map(|j| j.key.clone()).collect()),
        _ => None,
    }
}

/// The `needs` `job list` and `job show` print for an `open` or `update` Job (§3.2): the PR's other Jobs that are not `cancelled`, by key
/// (their Ready rule's list, derived each time it is read). The Job's own `needs` for every other kind.
pub fn shown_needs(all: &[Job], job: &Job) -> Vec<String> {
    if !matches!(job.kind.as_str(), "open" | "update") {
        return job.needs.clone();
    }
    // A Job chained after the merged open Job (it needs it, directly or not) is no need of it: that would be a cycle.
    let mut after: std::collections::BTreeSet<&str> = std::collections::BTreeSet::from([job.key.as_str()]);
    while let Some(next) = all.iter().find(|j| !after.contains(j.key.as_str()) && j.needs.iter().any(|n| after.contains(n.as_str()))) {
        after.insert(next.key.as_str());
    }
    let mut keys: Vec<String> = others(all, job).filter(|j| j.status != "cancelled" && !after.contains(j.key.as_str())).map(|j| j.key.clone()).collect();
    keys.sort();
    keys
}

/// Whether none of the PR's other Jobs is `merged`: an `open` Job is not Ready then, as a PR whose Jobs are all cancelled has nothing to open.
pub fn nothing_merged(all: &[Job], job: &Job) -> bool {
    others(all, job).all(|j| j.status != "merged")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::jobs::Job;
    use crate::ids::PrKey;

    fn job(key: &str, pr: &str, kind: &str, status: &str) -> Job {
        Job {
            seq: 1,
            key: key.into(),
            pr: pr.into(),
            title: format!("Title {key}"),
            tier: Some("S2".into()),
            spec: None,
            status: status.into(),
            appended: false,
            kind: kind.into(),
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
            created_at: "t".into(),
            needs: vec!["unused".into()],
        }
    }

    fn pr(key: &str) -> PrRow {
        PrRow::fixture(key, "/r", "main", &[], "dispatch")
    }

    #[test]
    fn the_key_is_the_first_free_of_open_pr_and_its_numbered_successors() {
        let key = |taken: &[&str]| free_key(&taken.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>(), "open", &pr("api")).unwrap();
        assert_eq!(key(&[]).as_str(), "open-api");
        assert_eq!(key(&["open-api"]).as_str(), "open-api-2");
        assert_eq!(key(&["open-api", "open-api-2", "open-web"]).as_str(), "open-api-3");
        // A PR key of 24 characters leaves room for a suffix of up to two digits (32 characters).
        let long = PrKey::parse(&"a".repeat(24)).unwrap().to_string();
        let taken: Vec<String> = std::iter::once(format!("open-{long}")).chain((2..10).map(|n| format!("open-{long}-{n}"))).collect();
        assert_eq!(free_key(&taken, "open", &pr(&long)).unwrap().as_str().len(), 32);
    }

    #[test]
    fn an_update_job_waits_only_for_the_queued_and_running_jobs_of_its_pr() {
        let update = job("up-a", "a", "update", "queued");
        let all = [
            job("q", "a", "work", "queued"),
            job("r", "a", "work", "running"),
            job("d", "a", "work", "draft"),
            job("f", "a", "work", "failed"),
            job("m", "a", "work", "merged"),
            job("other", "b", "work", "queued"),
            job("open-a", "a", "open", "queued"),
            update.clone(),
        ];
        assert_eq!(unfinished(&all, &update), Some(vec!["q".to_owned(), "r".to_owned()]));
        assert_eq!(shown_needs(&all, &update), ["d", "f", "m", "q", "r"].map(str::to_owned));
    }

    #[test]
    fn an_open_job_waits_for_the_other_jobs_of_its_pr_that_are_not_merged_or_cancelled() {
        let open = job("open-a", "a", "open", "queued");
        let all = [
            job("done", "a", "work", "merged"),
            job("gone", "a", "work", "cancelled"),
            job("later", "a", "work", "draft"),
            job("broken", "a", "work", "failed"),
            job("busy", "a", "work", "running"),
            job("queued", "a", "work", "queued"),
            job("other-pr", "b", "work", "running"),
            job("up-a", "a", "update", "queued"),
            open.clone(),
        ];
        let mut waiting = unfinished(&all, &open).unwrap();
        waiting.sort();
        assert_eq!(waiting, ["broken", "busy", "later", "queued"], "the open and update Jobs of a PR do not count each other");
        assert_eq!(unfinished(&all, &all[0]), None, "a Job of another kind has Dependency rows");
        assert!(!nothing_merged(&all, &open));
        assert!(nothing_merged(&[job("gone", "a", "work", "cancelled"), job("merged-open", "a", "open", "merged"), open.clone()], &open));
    }

    #[test]
    fn the_needs_an_open_job_shows_are_the_other_jobs_of_its_pr_that_are_not_cancelled() {
        let open = job("open-a", "a", "open", "queued");
        let all = [job("z", "a", "work", "merged"), job("gone", "a", "work", "cancelled"), job("b", "a", "work", "queued"), job("x", "b", "work", "queued"), open.clone()];
        assert_eq!(shown_needs(&all, &open), ["b", "z"]);
        assert_eq!(shown_needs(&all, &all[0]), ["unused"], "any other kind shows its Dependency rows");
    }

    #[test]
    fn a_merged_open_job_does_not_show_the_jobs_chained_after_it_as_needs() {
        let open = job("open-a", "a", "open", "merged");
        let mut fix1 = job("fix1", "a", "work", "merged");
        fix1.needs = vec!["open-a".into()];
        let mut fix2 = job("fix2", "a", "work", "queued");
        fix2.needs = vec!["fix1".into()];
        let early = job("early", "a", "work", "merged");
        assert_eq!(shown_needs(&[early, fix1, fix2, open.clone()], &open), ["early"]);
    }
}
