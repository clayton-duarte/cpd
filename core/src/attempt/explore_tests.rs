//! Red tests of the explore Attempt at `attempt submit` (Plan `explore-roles`, R8-R9; contract §7.7 "Explore kinds at
//! submit", §5.2, §5.3; commands.md §6.1, §5.7): the three refusals, the checks that do not run, the one-transaction end
//! with no git merge, the `job-merged` Event and its three-line text, and `job show`. Every command runs in process on a
//! temp `$CPD_HOME` with the fakes of `crate::ports`: no git, no tmux, no process.
//!
//! The [`Rig`] and its helpers are also the explore fixture of `driver::explore_tests`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::SubmitArgs;
use super::submit::submit_in;
use crate::args::PlanArg;
use crate::config::{Catalog, TierModel};
use crate::driver::{StartCtx, start_with};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::output::Output;
use crate::ports::{Fakes, Ports};

pub(crate) fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

pub(crate) fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![1, 2, 3],
    };
    Catalog {
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    }
}

/// The SHA-256 (hex) of `bytes`: what an explore end stores as the artefact's `sha`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The artefact of Job `key`: `$CPD_HOME/run/pay/explore/<key>.md`.
pub(crate) fn artefact(home: &CpdHome, key: &str) -> PathBuf {
    home.run_dir()
        .join("pay")
        .join("explore")
        .join(format!("{key}.md"))
}

/// Plan `pay` (phase `dispatch`) of Context `ctx` owned by `acme`, in a temp `$CPD_HOME` with a config and the owner's
/// policy file, on the in-process fakes.
pub(crate) struct Rig {
    pub(crate) tmp: TempDir,
    pub(crate) fakes: Fakes,
    pub(crate) env: Env,
}

impl Rig {
    pub(crate) fn new() -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
        let owner = home.config().parent().unwrap().join("policy").join("acme");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(owner.join("owner.toml"), "").unwrap();
        let fakes = Fakes::new();
        let mut env = Env::open_home_with(&fakes.ports, home).unwrap();
        env.catalog = Some(catalog());
        let mut rig = Rig { tmp, fakes, env };
        rig.sql(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
               VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
        );
        rig
    }

    pub(crate) fn sql(&mut self, sql: &str) {
        run_sql(&mut self.env, sql);
    }

    pub(crate) fn home(&self) -> CpdHome {
        self.env.home.clone()
    }

    /// A queued explore Job `key` of `kind` and `tier`, with its true fingerprint.
    pub(crate) fn add_job(&mut self, key: &str, kind: &str, tier: &str) {
        add_job(&mut self.env, key, kind, tier);
    }

    /// Starts Job `key` as the starter does (the driver is only recorded by the fake spawn) and makes its Attempt `running`.
    pub(crate) fn start(&mut self, key: &str) -> AttemptId {
        start_running(&mut self.env, &self.fakes, key)
    }

    /// Writes the artefact of Job `key`, creating the explore directory.
    pub(crate) fn write_artefact(&self, key: &str, text: &str) -> PathBuf {
        let path = artefact(&self.env.home, key);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// A summary file with `text`, outside `$CPD_HOME`.
    pub(crate) fn summary_file(&self, text: &str) -> PathBuf {
        let path = self.tmp.path().join("summary.md");
        std::fs::write(&path, text).unwrap();
        path
    }

    /// `cpd attempt submit <attempt> --plan pay [--summary-file <path>]` on the fakes.
    pub(crate) fn submit(
        &mut self,
        attempt: &AttemptId,
        summary: Option<&Path>,
    ) -> Result<Output, CpdError> {
        submit(&mut self.env, &self.fakes, attempt, summary)
    }

    /// One-field read of the store.
    pub(crate) fn one<T: rusqlite::types::FromSql>(&mut self, sql: &str) -> T {
        self.env
            .store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// `(text, data)` of the Events of `kind`, in order.
    pub(crate) fn events(&mut self, kind: &str) -> Vec<(String, Value)> {
        self.env
            .store
            .read(|c| {
                let mut stmt =
                    c.prepare("SELECT text, data FROM events WHERE kind = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([kind], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            serde_json::from_str(&r.get::<_, String>(1)?).unwrap(),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }
}

pub(crate) fn run_sql(env: &mut Env, sql: &str) {
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

/// A queued explore Job `key` in Plan `pay` (no Workflow, the artefact as its `touches`), with its true fingerprint.
pub(crate) fn add_job(env: &mut Env, key: &str, kind: &str, tier: &str) {
    run_sql(
        env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', NULL, '{key}', 'T', '{tier}', 'g', 'c', '[\"run/pay/explore/{key}.md\"]', '[\"b\"]', '[]', '{kind}', 'queued', 0, '{}', cpd_txn(), 't');",
            "f".repeat(64)
        ),
    );
    let jobs = env
        .store
        .read(|conn| crate::plan::jobs::load(conn, &plan()))
        .unwrap();
    let job = jobs.iter().find(|j| j.key == key).unwrap();
    let fingerprint = crate::plan::locked_fingerprint(job);
    run_sql(
        env,
        &format!(
            "UPDATE jobs SET fingerprint = '{fingerprint}' WHERE plan = 'pay' AND key = '{key}';"
        ),
    );
}

/// The starter's Attempt of Job `key`, then `running`.
pub(crate) fn start_running(env: &mut Env, fakes: &Fakes, key: &str) -> AttemptId {
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    let catalog = catalog();
    let ctx = StartCtx {
        home: &env.home,
        config: &env.config,
        catalog: &catalog,
        command: "test",
    };
    let started = start_with(
        &fakes.ports,
        &mut env.store,
        &ctx,
        &plan(),
        &JobKey::parse(key).unwrap(),
    )
    .unwrap();
    run_sql(
        env,
        &format!(
            "UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = '{}';",
            started.attempt
        ),
    );
    started.attempt
}

/// `attempt submit` on `env`, on the fakes' ports.
pub(crate) fn submit(
    env: &mut Env,
    fakes: &Fakes,
    attempt: &AttemptId,
    summary: Option<&Path>,
) -> Result<Output, CpdError> {
    let args = SubmitArgs {
        attempt: attempt.clone(),
        summary_file: summary.map(Path::to_path_buf),
        plan: PlanArg { plan: Some(plan()) },
    };
    Ports::scope(&fakes.ports, || submit_in(env, args))
}

/// Asserts `result` is the refusal `code` (exit 3) and returns it.
fn refused(result: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    match result {
        Ok(out) => panic!("expected {code:?}, got a success: {out:?}"),
        Err(e) => {
            assert_eq!(e.code(), code, "{e:?}");
            assert_eq!(e.exit().code(), 3, "{code:?} is a refusal");
            e
        }
    }
}

fn summary_of(n: usize) -> String {
    "é".repeat(n)
}

/// The Attempt is still `running`, its Job `running`, and no `job-merged` Event exists.
fn nothing_changed(rig: &mut Rig, attempt: &AttemptId) {
    assert_eq!(
        rig.one::<String>(&format!(
            "SELECT status FROM attempts WHERE id = '{attempt}'"
        )),
        "running"
    );
    assert_eq!(
        rig.one::<String>("SELECT status FROM jobs WHERE key = 'look'"),
        "running"
    );
    assert!(rig.events("job-merged").is_empty());
}

#[test]
fn submit_refuses_a_missing_or_empty_artefact() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    let summary = rig.summary_file("Found it.");

    // No file at all; and with no summary either, the artefact is refused first.
    refused(
        rig.submit(&attempt, Some(&summary)),
        ErrorCode::ArtefactMissing,
    );
    refused(rig.submit(&attempt, None), ErrorCode::ArtefactMissing);
    nothing_changed(&mut rig, &attempt);

    // An empty file is no artefact.
    rig.write_artefact("look", "");
    refused(
        rig.submit(&attempt, Some(&summary)),
        ErrorCode::ArtefactMissing,
    );
    nothing_changed(&mut rig, &attempt);
}

#[test]
fn submit_refuses_a_missing_summary() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\n");
    let missing = rig.tmp.path().join("nowhere.md");
    let empty = rig.summary_file("");

    refused(rig.submit(&attempt, None), ErrorCode::SummaryMissing);
    refused(
        rig.submit(&attempt, Some(&missing)),
        ErrorCode::SummaryMissing,
    );
    refused(
        rig.submit(&attempt, Some(&empty)),
        ErrorCode::SummaryMissing,
    );
    nothing_changed(&mut rig, &attempt);
}

#[test]
fn submit_refuses_a_summary_over_1500_characters() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\n");
    // 1501 characters of 2 bytes each: counted as characters, never bytes.
    let long = rig.summary_file(&summary_of(1501));

    let e = refused(rig.submit(&attempt, Some(&long)), ErrorCode::SummaryTooLong);
    let details = serde_json::to_value(e.details()).unwrap();
    assert_eq!(details["length"], 1501, "{details}");
    assert_eq!(details["cap"], 1500, "{details}");
    nothing_changed(&mut rig, &attempt);
    assert_eq!(
        std::fs::read_to_string(&long).unwrap().chars().count(),
        1501,
        "the file is never truncated"
    );
}

#[test]
fn submit_accepts_a_summary_of_exactly_1500_characters() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\n");
    let exact = summary_of(1500);
    assert!(exact.len() > 1500, "multibyte: more bytes than characters");
    let file = rig.summary_file(&exact);

    let result = rig.submit(&attempt, Some(&file));
    assert!(result.is_ok(), "1500 characters pass: {result:?}");
    assert_eq!(
        rig.one::<String>("SELECT summary FROM jobs WHERE key = 'look'"),
        exact,
        "the summary is stored whole"
    );
}

#[test]
fn submit_of_an_explore_job_runs_no_quick_check_tamper_check_or_tests() {
    let mut rig = Rig::new();
    rig.add_job("look", "research", "S1");
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\n");
    let summary = rig.summary_file("Found it.");
    // Whatever the starter ran, from here on a git, hook or test command is a call of the check.
    let before = rig.fakes.runner.calls().len();

    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");
    let calls = rig.fakes.runner.calls();
    assert_eq!(
        calls[before..]
            .iter()
            .map(|c| format!("{} {:?}", c.program, c.args))
            .collect::<Vec<_>>(),
        Vec::<String>::new(),
        "no git, hook or test command"
    );
    assert!(
        rig.fakes.runner.unscripted().is_empty(),
        "nothing was asked of the runner"
    );
    // No `checks` span either: the Worker's clocks never stop.
    let checks = rig
        .events("attempt-status")
        .into_iter()
        .filter(|(_, d)| d.get("check").is_some())
        .count();
    assert_eq!(checks, 0, "no check span opened");
}

#[test]
fn explore_job_merges_without_a_git_merge_and_stores_the_summary() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    let body = "# Findings\nThe cache key is built in cache.rs.\n";
    rig.write_artefact("look", body);
    let summary_text = "The cache key is built in cache.rs.\n";
    let summary = rig.summary_file(summary_text);
    let before = rig.fakes.runner.calls().len();
    let hash = sha256_hex(body.as_bytes());

    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");

    assert_eq!(
        rig.one::<String>("SELECT status FROM jobs WHERE key = 'look'"),
        "merged"
    );
    assert_eq!(
        rig.one::<String>("SELECT summary FROM jobs WHERE key = 'look'"),
        summary_text
    );
    assert_eq!(
        rig.one::<String>("SELECT merged_sha FROM jobs WHERE key = 'look'"),
        hash,
        "the artefact's SHA-256"
    );
    let row = |rig: &mut Rig, column: &str| {
        rig.one::<Option<String>>(&format!(
            "SELECT {column} FROM attempts WHERE id = '{attempt}'"
        ))
    };
    assert_eq!(row(&mut rig, "status").as_deref(), Some("ended"));
    assert_eq!(row(&mut rig, "outcome").as_deref(), Some("merged"));
    assert_eq!(row(&mut rig, "merged_sha").as_deref(), Some(hash.as_str()));
    assert_eq!(
        row(&mut rig, "submitted_sha").as_deref(),
        Some(hash.as_str())
    );
    assert_eq!(
        rig.fakes.runner.calls().len(),
        before,
        "no git call was made"
    );

    // Both ends are one transaction: the Job's and the Attempt's rows, and the Event, carry one txn.
    let ends = "SELECT txn FROM history WHERE tbl = 'jobs' AND op = 'update' AND json_extract(after, '$.status') = 'merged' AND json_extract(after, '$.seq') = (SELECT seq FROM jobs WHERE plan = 'pay' AND key = 'look')
                UNION ALL SELECT txn FROM history WHERE tbl = 'attempts' AND op = 'update' AND json_extract(after, '$.status') = 'ended' AND json_extract(after, '$.id') = 'look-a1'
                UNION ALL SELECT txn FROM events WHERE kind = 'job-merged'";
    assert_eq!(
        rig.one::<i64>(&format!("SELECT count(*) FROM ({ends})")),
        3,
        "the Job's end, the Attempt's end and the Event are all recorded"
    );
    assert_eq!(
        rig.one::<i64>(&format!("SELECT count(DISTINCT txn) FROM ({ends})")),
        1,
        "the Attempt, the Job and the Event end in one transaction"
    );
    // Nothing keeps the driver lock held: the Attempt has ended.
    let free = crate::driver::lock::probe(&rig.home(), &plan(), &attempt).unwrap();
    assert!(free.is_some(), "no driver lock is held");
}

#[test]
fn an_explore_attempt_stores_an_empty_actual_files() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    let body = "# Findings\n";
    rig.write_artefact("look", body);
    let summary = rig.summary_file("Found it.\n");
    let before = rig.fakes.runner.calls().len();

    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");

    let column = |rig: &mut Rig, c: &str| {
        rig.one::<Option<String>>(&format!("SELECT {c} FROM attempts WHERE id = '{attempt}'"))
    };
    assert_eq!(
        column(&mut rig, "submitted_sha").as_deref(),
        Some(sha256_hex(body.as_bytes()).as_str()),
        "submitted_sha is set"
    );
    assert_eq!(
        column(&mut rig, "actual_files").as_deref(),
        Some("[]"),
        "an explore Attempt writes no repo file"
    );
    assert_eq!(
        rig.fakes.runner.calls().len(),
        before,
        "no git call was made"
    );
}

#[test]
fn job_merged_event_of_an_explore_job_carries_summary_and_artefact() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    let body = "# Findings\n";
    rig.write_artefact("look", body);
    let summary = rig.summary_file("Found it.");

    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");

    let events = rig.events("job-merged");
    assert_eq!(events.len(), 1);
    let data = &events[0].1;
    assert_eq!(data["summary"], "Found it.");
    assert_eq!(
        data["artefact"], "run/pay/explore/look.md",
        "relative to $CPD_HOME"
    );
    assert_eq!(data["sha"], sha256_hex(body.as_bytes()));
    assert_eq!(data["tier"], "H2");
    assert_eq!(data["kind"], "scout");
    for absent in ["pr", "number", "url"] {
        assert!(data.get(absent).is_none(), "{absent} is absent: {data}");
    }
    // The Event kind's field spec (contract §5.2) accepts exactly these keys.
    let spec = crate::events::Kind::JobMerged.spec();
    let allowed: Vec<&str> = spec.required.iter().chain(spec.optional).copied().collect();
    for key in data.as_object().unwrap().keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "data.{key} is not in the field spec {allowed:?}"
        );
    }
    for key in spec.required {
        assert!(
            data.get(*key).is_some(),
            "required data.{key} is missing: {data}"
        );
    }
}

#[test]
fn job_merged_text_of_an_explore_job_is_the_three_line_summary_message() {
    for (kind, tier) in [("scout", "H2"), ("research", "S1")] {
        let mut rig = Rig::new();
        rig.add_job("look", kind, tier);
        let attempt = rig.start("look");
        rig.write_artefact("look", "# Findings\nNever in the message.\n");
        let summary = rig.summary_file("Found it in cache.rs.");

        let result = rig.submit(&attempt, Some(&summary));
        assert!(result.is_ok(), "{kind}: {result:?}");

        let events = rig.events("job-merged");
        assert_eq!(events.len(), 1);
        let expected = format!(
            "Job look ({kind}) merged: findings in $CPD_HOME/run/pay/explore/look.md.\n\
             Summary: Found it in cache.rs.\n\
             Next: cite that path in the Specs that need it; add another explore Job when the summary is not enough. cpd job show look --plan pay"
        );
        assert_eq!(events[0].0, expected, "{kind}");
    }
}

#[test]
fn job_merged_of_an_explore_job_always_turns_the_lead() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    // Another Job of the Plan is running, and no Job is a draft: the §5.2 rule alone would not turn the Lead.
    rig.add_job("busy", "scout", "H2");
    rig.sql("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'busy';");
    assert_eq!(
        rig.one::<i64>("SELECT count(*) FROM jobs WHERE status = 'draft'"),
        0
    );
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\n");
    let summary = rig.summary_file("Found it.");

    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");

    let events = rig.events("job-merged");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1["turn"], true, "{}", events[0].1);
}

#[test]
fn job_show_of_a_merged_explore_job_shows_summary_and_artefact() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let attempt = rig.start("look");
    let hash = sha256_hex(b"# Findings\n");
    rig.sql(&format!(
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = '{hash}', submitted_sha = '{hash}',
                submitted_at = 't', ended_at = 't' WHERE plan = 'pay' AND id = '{attempt}';
         UPDATE jobs SET status = 'merged', merged_sha = '{hash}', summary = 'Found it in cache.rs.' WHERE plan = 'pay' AND key = 'look';"
    ));

    let args = crate::job::ShowArgs {
        key: JobKey::parse("look").unwrap(),
        plan: PlanArg { plan: Some(plan()) },
    };
    let out = crate::job::show::show_in(&mut rig.env, args).unwrap();
    let shown = match out {
        Output::Success(map) => Value::Object(map).to_string(),
        Output::Owned => panic!("job show prints through the envelope"),
    };
    assert!(
        shown.contains("Found it in cache.rs."),
        "the summary is shown: {shown}"
    );
    assert!(
        shown.contains("run/pay/explore/look.md"),
        "the artefact is shown: {shown}"
    );
}

/// `cpd job add <key> --kind scout --tier H2 --spec <file> --plan pay` on `rig`, parsed by clap and run in process.
fn job_add_scout(rig: &mut Rig, key: &str) -> Result<Output, CpdError> {
    #[derive(clap::Parser)]
    struct Wrap {
        #[command(flatten)]
        add: crate::job::AddArgs,
    }
    let spec = rig.tmp.path().join(format!("{key}-spec.json"));
    std::fs::write(
        &spec,
        r#"{"goal": "g", "context": "c", "acceptance": ["b"], "out_of_scope": []}"#,
    )
    .unwrap();
    let wrap = <Wrap as clap::Parser>::try_parse_from([
        "wrap",
        key,
        "--title",
        "T",
        "--tier",
        "H2",
        "--spec",
        spec.to_str().unwrap(),
        "--kind",
        "scout",
        "--plan",
        "pay",
    ])
    .unwrap();
    crate::job::add::add_in(&mut rig.env, wrap.add)
}

#[test]
fn ending_an_explore_job_keeps_a_replanned_plan_in_phase_plan() {
    // pro-d429/pro-d433: an explore add moves the Plan to phase plan (locked_at kept); settle_done never touches phase plan.
    let mut rig = Rig::new();
    rig.sql(
        "INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
           VALUES (7, 'o/r', 7, 'https://example/7', 'Ship', 0, 'merged', 'none', 'none', 't', 't', 't', 'unknown', 'unknown', 0, 0);
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, merged_at, created_txn, created_at, started_at)
           VALUES ('pay', 'api', '/r', 'main', 'merged', 'cpd/pay/api', '/lw', 7, 'https://example/7', 't', 7, 't', 1, 't', 't');",
    );
    job_add_scout(&mut rig, "look").unwrap();
    assert_eq!(
        rig.one::<String>("SELECT phase FROM plans WHERE id = 'pay'"),
        "plan",
        "the add moved the Plan back to phase plan"
    );
    let locked = rig.one::<Option<String>>("SELECT locked_at FROM plans WHERE id = 'pay'");
    assert!(locked.is_some());
    let attempt = rig.start("look");
    rig.write_artefact("look", "# Findings\nnothing to change\n");
    let summary = rig.summary_file("Nothing to change.\n");
    let result = rig.submit(&attempt, Some(&summary));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        rig.one::<String>("SELECT status FROM jobs WHERE key = 'look'"),
        "merged"
    );
    assert_eq!(
        rig.one::<String>("SELECT phase FROM plans WHERE id = 'pay'"),
        "plan"
    );
    assert_eq!(
        rig.one::<Option<String>>("SELECT locked_at FROM plans WHERE id = 'pay'"),
        locked
    );
    assert_eq!(
        rig.one::<i64>("SELECT count(*) FROM events WHERE kind = 'plan-done'"),
        0
    );
    assert_eq!(
        rig.one::<Option<String>>("SELECT done_at FROM plans WHERE id = 'pay'"),
        None
    );
}
