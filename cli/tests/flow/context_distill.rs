//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run") and the engine's debounced
//! rebuild (contract §3.7 E9), run as the built binary with a fake `pi` on `PATH`.

use crate::common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Child, Stdio};

use common::{CPD, Run, pi_says, TestEnv, refused, wait_until};
use cpd_core::config::Tier;
use cpd_core::context::{By, Draft, PlanRef, Record};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, MachineName, PlanId, RecordId};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

const CONTEXT: &str = "ctx";

struct Fx {
    env: TestEnv,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_pi(&pi_says(common::CURATES_ALL));
    // pi's catalog (hs-d538): the ladder a failed step climbs reads it.
    env.fake("node", &format!("echo '{LADDER_LEVELS}'"));
    let install = env.root().join("install");
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("current-version"), "1.0.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = install.join("releases/1.0.0/node_modules/@earendil-works").join(package).join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.js"), "").unwrap();
    }
    // The engine's E5 runs `cpd pr poll` at every tick (`start_engine`), through a `gh` that finds no pull requests: the
    // poll searches count whole ticks (`wait_whole_ticks`).
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    let fx = Fx { env };
    fx.cpd(&["context", "new", CONTEXT, "--title", "Ctx", "--owner", "acme"]).ok();
    fx
}

fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

impl Fx {
    /// Makes the ladder one step long (hs-d538), for the tests of what one step does: starts at H3, the top of
    /// the catalog's H efforts, and the catalog has no S effort above 0, which the S steps (S1..S4) need.
    fn one_step(self) -> Fx {
        self.env.fake("node", r#"echo '{"O":["off"],"S":["off"],"H":["off","low","medium","high"]}'"#);
        let config = self.env.cpd_home().join("config.toml");
        let text = fs::read_to_string(&config).unwrap().replacen("lead_pi_packages", "context_distill_tier = \"H3\"\nlead_pi_packages", 1);
        fs::write(&config, text).unwrap();
        self
    }

    fn cpd(&self, args: &[&str]) -> Run {
        self.env.cpd(args)
    }

    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap()
    }

    fn store(&self) -> Store {
        Store::open(&self.home(), 5000).unwrap()
    }

    fn digests(&self) -> PathBuf {
        self.home().context_digest_dir(&ContextId::parse(CONTEXT).unwrap())
    }

    /// The names in the Context's digest directory, sorted, without the run's own files.
    fn digest_files(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.digests()) else { return Vec::new() };
        let mut names: Vec<String> = entries
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| !["prompt.md", "batch.md", "known.md", "docs.md", "distill.log", "runs.jsonl"].contains(&n.as_str()))
            .collect();
        names.sort();
        names
    }

    /// Writes a record straight into the index with a chosen time (the commands write `at` as now).
    fn insert(&self, context: &str, id: &str, at: &str, text: &str, draft: Draft) {
        let record = Record { id: RecordId::parse(id).unwrap(), at: at.into(), by: By { session: None, plan: None }, text: text.into(), draft };
        let context = ContextId::parse(context).unwrap();
        self.store().write("fixture", |tx| Ok(record.insert(tx, &context).unwrap())).unwrap();
    }

    fn decide(&self, text: &str) -> String {
        self.cpd(&["context", "decide", text, "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned()
    }

    fn highest_seq(&self) -> u64 {
        self.env.read(|conn| Ok(conn.query_row("SELECT max(seq) FROM context_records WHERE context = ?1", [CONTEXT], |r| r.get::<_, i64>(0))?)) as u64
    }

    fn pi_calls(&self) -> usize {
        self.env.calls("pi").len()
    }

    /// The lines of the Context's `runs.jsonl`.
    fn runs(&self) -> Vec<Value> {
        let text = fs::read_to_string(self.digests().join("runs.jsonl")).unwrap_or_default();
        text.lines().map(|l| serde_json::from_str(l).expect("a run line is JSON")).collect()
    }

    /// The `error` Events of `context distill`.
    fn errors(&self) -> Vec<Value> {
        let events = self.cpd(&["events", "list", "--kind", "error"]).ok()["events"].as_array().unwrap().clone();
        events.into_iter().filter(|e| e["data"]["source"] == "context distill").collect()
    }
}

#[test]
fn the_distiller_gets_no_cpd_variable() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi(&format!("env > \"$HOME/pi.env\"\n{}", pi_says(common::CURATES_ALL)));
    let mut command = fx.env.command(CPD);
    command.args(["context", "distill", "--context", CONTEXT]).env("CPD_SESSION", "3f0c1b2a-5d4e-4f60-8a7b-9c0d1e2f3a4b").env("CPD_PLAN", "alpha");
    let out = command.output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let env = fs::read_to_string(fx.env.home().join("pi.env")).unwrap();
    assert!(env.lines().any(|l| l == "PI_SKIP_VERSION_CHECK=1"), "{env}");
    let names: Vec<&str> = env.lines().filter_map(|l| l.split_once('=')).map(|(n, _)| n).filter(|n| n.starts_with("CPD_")).collect();
    assert!(names.is_empty(), "{names:?}");
}

#[test]
fn a_failing_pi_is_distill_failed_with_its_stderr_tail() {
    let fx = fx().one_step();
    fx.decide("Use sqlite.");
    fx.env.fake_pi("echo 'half an answer'; echo 'model unavailable' >&2; exit 7");
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    let details = &error["details"];
    assert_eq!((details["reason"].clone(), details["exit"].clone()), (json!("exit"), json!(7)), "{details}");
    assert_eq!(details["tail"], "model unavailable");
    assert_eq!(details["upto_seq"], fx.highest_seq());
    assert!(fx.digest_files().is_empty(), "nothing is left");
    let events = fx.errors();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["code"], "distill_failed");
    assert_eq!(events[0]["context"], Value::Null);
    let log = fs::read_to_string(details["log"].as_str().unwrap()).unwrap();
    assert!(log.contains("model unavailable"));
    let runs = fx.runs();
    assert_eq!(runs.len(), 1, "a failed run is logged too: {runs:?}");
    assert_eq!(runs[0]["result"], "failed");
    assert_eq!((runs[0]["tokens_in"].clone(), runs[0]["tokens_out"].clone(), runs[0]["cost"].clone()), (Value::Null, Value::Null, Value::Null));
    assert!(runs[0]["seconds"].is_number());
}

#[test]
fn output_that_is_not_pi_json_is_distill_failed_output() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi("echo '# Digest, as the text mode printed it'");
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!((error["details"]["reason"].clone(), error["details"]["exit"].clone()), (json!("output"), Value::Null));
    assert!(error["message"].as_str().unwrap().contains("line 1"), "{error}");
    assert!(fx.digest_files().is_empty(), "nothing is left");
    assert_eq!(fx.errors().len(), 1);
    assert_eq!(fx.runs()[0]["result"], "failed");
}

#[test]
fn an_answer_cut_at_the_output_limit_fails_the_run_but_its_cost_is_logged() {
    let fx = fx();
    fx.decide("Use sqlite.");
    // pi exits 0 in JSON mode when the final message stopped with `length`.
    fx.env.fake_pi(&pi_says("echo 'half'").replace(r#""stopReason":"stop""#, r#""stopReason":"length""#));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!(error["details"]["reason"], "output");
    assert!(error["message"].as_str().unwrap().contains("length"), "{error}");
    assert!(fx.digest_files().is_empty());
    let runs = fx.runs();
    assert_eq!((runs[0]["result"].clone(), runs[0]["tokens_in"].clone(), runs[0]["cost"].clone()), (json!("failed"), json!(733), json!(0.25)));
}

#[test]
fn pi_that_cannot_start_is_distill_failed_spawn() {
    let fx = fx();
    fx.decide("Use sqlite.");
    let out = fx.env.command(CPD).args(["context", "distill", "--context", CONTEXT]).env("PATH", "/nonexistent").output().unwrap();
    let run = Run { exit: out.status.code().unwrap(), stdout: String::from_utf8(out.stdout).unwrap(), stderr: String::new() };
    let error = refused(&run, "distill_failed", 1);
    assert_eq!(error["details"]["reason"], "spawn");
    assert_eq!(error["details"]["exit"], Value::Null);
    assert_eq!(fx.errors().len(), 1);
    let runs = fx.runs();
    assert_eq!((runs[0]["result"].clone(), runs[0]["tokens_out"].clone(), runs[0]["cost"].clone()), (json!("failed"), Value::Null, Value::Null));
}

#[test]
fn a_context_with_nothing_in_force_has_nothing_to_do() {
    let fx = fx();
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["already"].clone(), out["digest"].clone()), (json!(true), Value::Null));
    assert_eq!(fx.pi_calls(), 0);
    assert!(fx.digest_files().is_empty());
}

#[test]
fn a_second_run_of_a_context_is_refused_while_the_first_is_alive() {
    let fx = fx();
    fx.decide("Use sqlite.");
    let home = fx.home();
    fs::create_dir_all(home.root().join("locks")).unwrap();
    let held = Flock::lock(File::create(home.distill_lock(&ContextId::parse(CONTEXT).unwrap())).unwrap(), FlockArg::LockExclusiveNonblock).unwrap();
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_running", 3);
    assert_eq!(error["details"]["context"], CONTEXT);
    assert_eq!(fx.pi_calls(), 0);
    assert!(fx.errors().is_empty(), "a refused run is no failure");
    drop(held);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
}

#[test]
fn the_context_is_required_and_must_exist() {
    let fx = fx();
    refused(&fx.cpd(&["context", "distill"]), "context_required", 2);
    refused(&fx.cpd(&["context", "distill", "--context", "nope"]), "context_not_found", 4);
}

// ---- The Tier ladder (hs-d538): contract §4.6 ----

const LADDER_LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium","high","xhigh","max"],"H":["off","low","medium","high"]}"#;

// ---- The curating distill step (contract §4.6 "The distiller curates", "The distill run"; hs-d549, hs-d551) ----

/// The rows of `record_curations`: `(record, topic, short, doc_candidate)`, by record.
fn curation_rows(fx: &Fx) -> Vec<(String, String, String, Option<String>)> {
    fx.env.read(|conn| {
        let mut statement = conn.prepare("SELECT record, topic, short, doc_candidate FROM record_curations WHERE context = 'ctx' ORDER BY record")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect()
    })
}

/// The rows of `record_retirements`: `(record, reason, target, status)`, by record.
fn retirement_rows(fx: &Fx) -> Vec<(String, String, String, String)> {
    fx.env.read(|conn| {
        let mut statement = conn.prepare("SELECT record, reason, target, status FROM record_retirements WHERE context = 'ctx' ORDER BY record")?;
        let rows = statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect()
    })
}

fn s(text: &str) -> String {
    text.to_owned()
}

/// A fake `pi` that keeps a copy of its input files under `$HOME/seen-<name>` and answers `answer` (shell).
fn pi_keeps_input_and_says(answer: &str) -> String {
    pi_says(&format!("for f in batch.md known.md docs.md; do cp \"$f\" \"$HOME/seen-$f\"; done\n{answer}"))
}

fn seen(fx: &Fx, name: &str) -> String {
    fs::read_to_string(fx.env.home().join(format!("seen-{name}"))).unwrap_or_else(|e| panic!("the distiller saw no {name}: {e}"))
}

/// The ids of the blocks of a `batch.md`, in file order.
fn block_ids(text: &str) -> Vec<String> {
    text.lines().filter(|l| l.starts_with('[')).map(|l| l[1..l.find(']').unwrap()].to_owned()).collect()
}

#[test]
fn a_step_sends_at_most_60_uncurated_records_and_the_known_short_forms() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(500), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO record_curations (context, record, topic, short, curated_at) VALUES ('ctx', 'hs-d1', 'storage', 'known short form', 't')",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
    for n in 1..=61 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100), &format!("Note number {n}."), Draft::Note);
    }
    fx.env.fake_pi(&pi_keeps_input_and_says(common::CURATES_ALL));

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["distilled"], true, "{out}");
    // The batch: the 60 oldest uncurated records in force, oldest first, full blocks; the curated one is not in it.
    let batch = seen(&fx, "batch.md");
    let expected: Vec<String> = (1..=60).map(|n| format!("hs-n{n}")).collect();
    assert_eq!(block_ids(&batch), expected, "{batch}");
    assert!(batch.contains("Note number 60."), "{batch}");
    // known.md: the topics in use, then `[<id>] <topic> | <short>` for every curated record in force.
    let known = seen(&fx, "known.md");
    assert!(known.lines().any(|l| l == "storage"), "{known}");
    assert!(known.lines().any(|l| l == "[hs-d1] storage | known short form"), "{known}");
    assert!(!known.contains("hs-n"), "{known}");
    for name in ["prompt.md", "batch.md", "known.md", "docs.md"] {
        assert!(fx.digests().join(name).exists(), "{name} is written");
    }
    assert_eq!(curation_rows(&fx).len(), 61, "the batch is curated, the 61st record is not");

    // The next run takes the rest, and sees what the first one curated.
    let again = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(again["distilled"], true, "{again}");
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n61"]);
    let known = seen(&fx, "known.md");
    assert!(known.lines().any(|l| l == "[hs-n1] t | s hs-n1"), "{known}");
    assert!(known.lines().any(|l| l == "[hs-d1] storage | known short form"), "{known}");
    assert_eq!(curation_rows(&fx).len(), 62);
    assert_eq!(fx.cpd(&["context", "distill", "--context", CONTEXT]).ok()["already"], true);
    assert_eq!(fx.pi_calls(), 2);
}

#[test]
fn a_valid_output_writes_curations_and_pending_proposals() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(80), "Use sqlite in WAL mode.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d3", &ago(70), "Use sqlite with WAL.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-n4", &ago(60), "Locks are flock.", Draft::Note);
    fx.env.fake_pi(&pi_says(
        r#"cat <<'EOF'
{"id":"hs-d1","topic":"storage","short":"sqlite store","retire":{"reason":"superseded","target":"hs-d2"},"doc":false}
{"id":"hs-d2","topic":"storage","short":"sqlite WAL","retire":null,"doc":false}
{"id":"hs-d3","topic":"storage","short":"sqlite WAL again","retire":{"reason":"duplicate","target":"hs-d2"},"doc":false}
{"id":"hs-n4","topic":"locking","short":"flock locks","retire":null,"doc":true}
EOF"#,
    ));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["distilled"], true, "{out}");
    let upto = fx.highest_seq();
    assert_eq!(
        curation_rows(&fx),
        [
            (s("hs-d1"), s("storage"), s("sqlite store"), None),
            (s("hs-d2"), s("storage"), s("sqlite WAL"), None),
            (s("hs-d3"), s("storage"), s("sqlite WAL again"), None),
            (s("hs-n4"), s("locking"), s("flock locks"), Some(s("pending"))),
        ]
    );
    assert_eq!(
        retirement_rows(&fx),
        [
            (s("hs-d1"), s("superseded"), s("hs-d2"), s("pending")),
            (s("hs-d3"), s("duplicate"), s("hs-d2"), s("pending")),
        ],
        "proposals only: nothing is retired without the User"
    );
    let paths: String = fx.env.query("SELECT doc_paths FROM record_curations WHERE record = 'hs-n4'");
    assert_eq!(paths, "[]", "a doc candidate with no grep hit");
    // One `context-curated` Event with the counts.
    let events = fx.cpd(&["events", "list", "--kind", "context-curated"]).ok()["events"].as_array().unwrap().clone();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["data"], json!({"context": CONTEXT, "curated": 4, "retire": 2, "docs": 1}));
    assert!(fx.errors().is_empty());
    // No digest file, no kept invalid output.
    assert!(!fx.digests().join(format!("{upto}.md")).exists());
    assert!(!fx.digests().join(format!("{upto}.jsonl.invalid")).exists());
    let runs = fx.runs();
    assert_eq!((runs.len(), runs[0]["result"].clone(), runs[0]["kind"].clone()), (1, json!("valid"), json!("distill")), "{runs:?}");
    // Everything is curated now: nothing left to do.
    assert_eq!(fx.cpd(&["context", "distill", "--context", CONTEXT]).ok()["already"], true);
    assert_eq!(fx.pi_calls(), 1);
}

#[test]
fn an_output_missing_a_batch_id_is_invalid_and_climbs_the_ladder() {
    let fx = fx();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(80), "Use flock.", Draft::Decision { supersedes: None });
    fx.env.fake_pi(&pi_says(r#"echo '{"id":"hs-d1","topic":"storage","short":"sqlite","retire":null,"doc":false}'"#));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let upto = fx.highest_seq();
    let details = &error["details"];
    assert_eq!((details["context"].clone(), details["upto_seq"].clone()), (json!(CONTEXT), json!(upto)));
    let errors = details["errors"].as_array().unwrap_or_else(|| panic!("details.errors lists the failures: {details}"));
    assert!(errors.iter().any(|e| e["id"] == "hs-d2" && e["what"].is_string()), "{errors:?}");
    let invalid = fx.digests().join(format!("{upto}.jsonl.invalid"));
    assert_eq!(details["invalid"], invalid.display().to_string());
    assert!(fs::read_to_string(&invalid).unwrap().contains("hs-d1"), "the last output is kept");
    // The ladder (hs-d538) climbs as before, and the whole run writes one Event.
    assert_eq!(tiers_of(&fx.runs()), ["H0", "H1", "H2", "H3", "S1", "S2", "S3", "S4"]);
    assert_eq!(fx.errors().len(), 1);
    assert_eq!(fx.errors()[0]["data"]["code"], "distill_invalid");
    assert_eq!((curation_rows(&fx), retirement_rows(&fx)), (vec![], vec![]), "an invalid output writes nothing");
}

#[test]
fn a_superseded_target_must_be_newer() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(80), "Use sqlite in WAL mode.", Draft::Decision { supersedes: None });
    // hs-d2 is superseded by hs-d1, which is older (a lower seq): invalid.
    fx.env.fake_pi(&pi_says(
        r#"cat <<'EOF'
{"id":"hs-d1","topic":"storage","short":"sqlite","retire":null,"doc":false}
{"id":"hs-d2","topic":"storage","short":"sqlite WAL","retire":{"reason":"superseded","target":"hs-d1"},"doc":false}
EOF"#,
    ));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let errors = error["details"]["errors"].as_array().unwrap_or_else(|| panic!("details.errors: {error}"));
    assert!(errors.iter().any(|e| e["id"] == "hs-d2"), "{errors:?}");
    assert!(!errors.iter().any(|e| e["id"] == "hs-d1"), "only the bad line is listed: {errors:?}");
    assert_eq!((curation_rows(&fx), retirement_rows(&fx)), (vec![], vec![]));
    let upto = fx.highest_seq();
    assert!(fx.digests().join(format!("{upto}.jsonl.invalid")).exists());

    // The other way round is valid, and removes the kept output.
    fx.env.fake_pi(&pi_says(
        r#"cat <<'EOF'
{"id":"hs-d1","topic":"storage","short":"sqlite","retire":{"reason":"superseded","target":"hs-d2"},"doc":false}
{"id":"hs-d2","topic":"storage","short":"sqlite WAL","retire":null,"doc":false}
EOF"#,
    ));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(retirement_rows(&fx), [(s("hs-d1"), s("superseded"), s("hs-d2"), s("pending"))]);
    assert!(!fx.digests().join(format!("{upto}.jsonl.invalid")).exists());
}

/// Plan `pl` of `ctx` with one PR in each of `repos` (the PRs' repos are the Context's repos).
fn plan_with_repos(fx: &Fx, repos: &[&std::path::Path]) {
    let prs: String = repos
        .iter()
        .enumerate()
        .map(|(n, repo)| {
            format!(
                "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                 VALUES ('pl', 'pr{n}', '{}', 'main', 'waiting', 'cpd/pl/pr{n}', cpd_txn(), 't');",
                repo.display()
            )
        })
        .collect();
    fx.env.sql(&format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'grounding', 'lead-pl', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/', 'O3', 'idle', 't', 't');
         {prs}"
    ));
}

/// A git repo `name` with `files` (path, text) committed.
fn repo_with(fx: &Fx, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let repo = fx.env.git_repo(name).canonicalize().unwrap();
    for (path, text) in files {
        let file = repo.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    fx.env.git(&repo, &["add", "-A"]);
    fx.env.git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t.invalid", "commit", "-q", "-m", "docs"]);
    repo
}

#[test]
fn docs_md_lists_the_grep_hits_of_the_contexts_repos() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d10", &ago(80), "Use flock.", Draft::Decision { supersedes: None });
    let a = repo_with(&fx, "a", &[("docs/design.md", "We use sqlite, see hs-d1.\n"), ("notes.txt", "hs-d1 is not markdown\n"), ("docs/other.md", "Only hs-d10 here.\n")]);
    let b = repo_with(&fx, "b", &[("README.md", "Decided in hs-d1.\n")]);
    let elsewhere = repo_with(&fx, "c", &[("docs/x.md", "hs-d1 in a repo no PR of the Context has.\n")]);
    // `a` has two PRs: its hits are listed once.
    plan_with_repos(&fx, &[&a, &b, &a]);
    let _ = elsewhere;
    fx.env.fake_pi(&pi_keeps_input_and_says(common::CURATES_ALL));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let docs = seen(&fx, "docs.md");
    let (hit_a, hit_b) = (format!("{}:docs/design.md", a.display()), format!("{}:README.md", b.display()));
    assert_eq!(docs.matches(&hit_a).count(), 1, "{docs}");
    assert_eq!(docs.matches(&hit_b).count(), 1, "{docs}");
    assert!(!docs.contains("notes.txt") && !docs.contains("elsewhere") && !docs.contains("/c:"), "{docs}");
    // `-w`: hs-d10's own hit is not hs-d1's.
    assert_eq!(docs.matches(&format!("{}:docs/other.md", a.display())).count(), 1, "{docs}");
}

#[test]
fn a_documented_target_must_be_a_grep_hit() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(80), "Use flock.", Draft::Decision { supersedes: None });
    let a = repo_with(&fx, "a", &[("docs/design.md", "We use sqlite, see hs-d1.\n")]);
    plan_with_repos(&fx, &[&a]);
    let hit = format!("{}:docs/design.md", a.display());
    // hs-d1 names a path that is no hit; hs-d2 names a real hit of hs-d1, not of its own.
    fx.env.fake_pi(&pi_says(&format!(
        r#"cat <<'EOF'
{{"id":"hs-d1","topic":"storage","short":"sqlite","retire":{{"reason":"documented","target":"{a}:docs/invented.md"}},"doc":false}}
{{"id":"hs-d2","topic":"locking","short":"flock","retire":{{"reason":"documented","target":"{hit}"}},"doc":false}}
EOF"#,
        a = a.display()
    )));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let errors = error["details"]["errors"].as_array().unwrap_or_else(|| panic!("details.errors: {error}"));
    assert!(errors.iter().any(|e| e["id"] == "hs-d1") && errors.iter().any(|e| e["id"] == "hs-d2"), "every failure at once: {errors:?}");
    assert_eq!(retirement_rows(&fx), vec![]);

    fx.env.fake_pi(&pi_says(&format!(
        r#"cat <<'EOF'
{{"id":"hs-d1","topic":"storage","short":"sqlite","retire":{{"reason":"documented","target":"{hit}"}},"doc":false}}
{{"id":"hs-d2","topic":"locking","short":"flock","retire":null,"doc":false}}
EOF"#
    )));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(retirement_rows(&fx), [(s("hs-d1"), s("documented"), hit, s("pending"))]);
}

#[test]
fn no_repair_turn_and_no_contradictions_section() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    // Output that is no JSON line: invalid. With one step of the ladder, one pi run; no repair turn.
    fx.env.fake_pi(&pi_says("echo '# Digest'; echo 'Cites [hs-d1].'"));
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(fx.pi_calls(), 1, "a failure climbs the ladder, nothing repairs it: {:?}", fx.env.calls("pi"));
    assert!(!fx.env.calls("pi").iter().flatten().any(|a| a.contains("missing.md") || a.contains("repair")));
    for gone in ["repair-prompt.md", "missing.md", "problem.md", "records.md"] {
        assert!(!fx.digests().join(gone).exists(), "{gone}");
    }
    let prompt = fs::read_to_string(fx.digests().join("prompt.md")).unwrap();
    assert!(!prompt.contains("Contradictions") && !prompt.contains("Record index"), "{prompt}");

    fx.env.fake_pi(&pi_says(common::CURATES_ALL));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let runs = fx.runs();
    assert_eq!(runs.len(), 2, "one line per pi run: {runs:?}");
    for run in &runs {
        assert_eq!(run["kind"], "distill", "{run}");
        assert!(run.get("contradictions").is_none() && run.get("contradictions_dropped").is_none(), "{run}");
        assert_eq!(run.as_object().unwrap().len(), 11, "{run}");
    }
}

// ---- The fixes FX1 to FX4 (hs-d549, hs-d551) ----

/// The `what` of every failure of a refused run, with the ids they name.
fn failures(error: &Value) -> Vec<(Option<String>, String)> {
    let errors = error["details"]["errors"].as_array().unwrap_or_else(|| panic!("details.errors: {error}"));
    errors.iter().map(|e| (e["id"].as_str().map(str::to_owned), e["what"].as_str().unwrap_or_default().to_owned())).collect()
}

/// A line of the distiller's answer.
fn line(id: &str, topic: &str, short: &str, retire: &str) -> String {
    format!(r#"{{"id":"{id}","topic":"{topic}","short":"{short}","retire":{retire},"doc":false}}"#)
}

/// A fake `pi` that answers `lines`.
fn pi_answers(lines: &[String]) -> String {
    pi_says(&format!("cat <<'EOF'\n{}\nEOF", lines.join("\n")))
}

fn two_decisions(fx: &Fx) {
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(80), "Use sqlite in WAL mode.", Draft::Decision { supersedes: None });
}

#[test]
fn a_duplicate_target_newer_than_the_record_is_invalid() {
    let fx = fx().one_step();
    two_decisions(&fx);
    // hs-d1 (older) is a duplicate of hs-d2 (newer): the first is kept, so this is invalid.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", r#"{"reason":"duplicate","target":"hs-d2"}"#),
        line("hs-d2", "storage", "sqlite WAL", "null"),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let failed = failures(&error);
    let bad = failed.iter().find(|(id, _)| id.as_deref() == Some("hs-d1")).unwrap_or_else(|| panic!("hs-d1 is listed: {failed:?}"));
    assert!(bad.1.contains("hs-d1") && bad.1.contains("hs-d2"), "the error names both ids: {failed:?}");
    assert!(!failed.iter().any(|(id, _)| id.as_deref() == Some("hs-d2")), "{failed:?}");
    assert_eq!((curation_rows(&fx), retirement_rows(&fx)), (vec![], vec![]));

    // The newer one a duplicate of the older is valid.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", "null"),
        line("hs-d2", "storage", "sqlite WAL", r#"{"reason":"duplicate","target":"hs-d1"}"#),
    ]));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(retirement_rows(&fx), [(s("hs-d2"), s("duplicate"), s("hs-d1"), s("pending"))]);
}

#[test]
fn two_records_naming_each_other_as_duplicates_are_invalid() {
    let fx = fx().one_step();
    two_decisions(&fx);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", r#"{"reason":"duplicate","target":"hs-d2"}"#),
        line("hs-d2", "storage", "sqlite WAL", r#"{"reason":"duplicate","target":"hs-d1"}"#),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert!(!failures(&error).is_empty(), "{error}");
    assert_eq!((curation_rows(&fx), retirement_rows(&fx)), (vec![], vec![]), "two records never retire each other");
}

#[test]
fn a_topic_outside_lowercase_digits_and_hyphen_is_invalid() {
    let fx = fx().one_step();
    for n in 1..=6 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100 - n), &format!("Note {n}."), Draft::Note);
    }
    let too_long = "a".repeat(41);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "Storage", "upper case", "null"),
        line("hs-n2", "two words", "a space", "null"),
        line("hs-n3", "snake_case", "an underscore", "null"),
        line("hs-n4", &too_long, "41 characters", "null"),
        line("hs-n5", "storage-2", "digits and hyphen", "null"),
        line("hs-n6", "é", "not ascii", "null"),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let ids: Vec<String> = failures(&error).into_iter().filter_map(|(id, _)| id).collect();
    assert_eq!(ids, ["hs-n1", "hs-n2", "hs-n3", "hs-n4", "hs-n6"], "every bad topic at once, and not hs-n5");
    assert_eq!(curation_rows(&fx), vec![]);

    // 40 characters of the set are valid.
    let longest = "a1-".repeat(13) + "a";
    assert_eq!(longest.len(), 40);
    fx.env.fake_pi(&pi_answers(&(1..=6).map(|n| line(&format!("hs-n{n}"), &longest, "ok", "null")).collect::<Vec<_>>()));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(curation_rows(&fx).len(), 6);
}

#[test]
fn a_short_form_over_120_bytes_is_invalid() {
    let fx = fx().one_step();
    for n in 1..=3 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100 - n), &format!("Note {n}."), Draft::Note);
    }
    // hs-n1: 121 bytes; hs-n2: 120 bytes but an upper case topic (FX3); hs-n3 is fine: all failures at once.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "notes", &"x".repeat(121), "null"),
        line("hs-n2", "Notes", &"x".repeat(120), "null"),
        line("hs-n3", "notes", &"x".repeat(120), "null"),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let ids: Vec<String> = failures(&error).into_iter().filter_map(|(id, _)| id).collect();
    assert_eq!(ids, ["hs-n1", "hs-n2"], "{error}");
    assert_eq!(curation_rows(&fx), vec![]);
}

#[test]
fn an_unknown_or_repeated_id_is_invalid() {
    let fx = fx().one_step();
    for n in 1..=3 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100 - n), &format!("Note {n}."), Draft::Note);
    }
    // hs-n1 twice, hs-zz unknown, hs-n3 missing, and a bad topic on hs-n2 (FX3), all listed at once.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "notes", "one", "null"),
        line("hs-n1", "notes", "one again", "null"),
        line("hs-zz", "notes", "unknown", "null"),
        line("hs-n2", "Not A Topic", "two", "null"),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let failed = failures(&error);
    for id in ["hs-n1", "hs-n2", "hs-n3"] {
        assert!(failed.iter().any(|(i, _)| i.as_deref() == Some(id)), "{id} is listed: {failed:?}");
    }
    assert!(failed.iter().any(|(_, what)| what.contains("hs-zz")), "the unknown id is named: {failed:?}");
    assert_eq!(curation_rows(&fx), vec![]);
}

#[test]
fn a_retirement_whose_target_left_force_meanwhile_is_dropped_and_the_curations_stay() {
    let fx = fx().one_step();
    fx.insert(CONTEXT, "hs-d1", &ago(200), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-d2", &ago(190), "Use sqlite.", Draft::Decision { supersedes: None });
    // hs-d1 and hs-d2 are curated; hs-d2 has a pending retirement the User will approve while the distiller runs.
    fx.store()
        .write("fixture", |tx| {
            for id in ["hs-d1", "hs-d2"] {
                tx.execute(
                    "INSERT INTO record_curations (context, record, topic, short, curated_at) VALUES ('ctx', ?1, 'storage', 'sqlite', 't')",
                    [id],
                )?;
            }
            tx.execute(
                "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at)
                 VALUES ('ctx', 'hs-d2', 'duplicate', 'hs-d1', 'pending', 't')",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
    fx.insert(CONTEXT, "hs-d3", &ago(80), "Use sqlite in WAL mode.", Draft::Decision { supersedes: None });
    fx.insert(CONTEXT, "hs-n4", &ago(70), "Locks are flock.", Draft::Note);
    // The fake pi has the User approve hs-d2's retirement (hs-d2 leaves force) while it runs, then proposes
    // hs-d3 as a duplicate of hs-d2 (which was in force when the batch was checked... and is not when it is written)
    // and hs-n4 as a duplicate of hs-d1 (still in force).
    let approve = format!("CPD_HOME='{}' '{CPD}' context retire --context ctx --approve hs-d2 >/dev/null", fx.env.cpd_home().display());
    fx.env.fake_pi(&pi_says(&format!(
        "{approve}\ncat <<'EOF'\n{}\n{}\nEOF",
        line("hs-d3", "storage", "sqlite WAL", r#"{"reason":"duplicate","target":"hs-d2"}"#),
        line("hs-n4", "locking", "flock", r#"{"reason":"duplicate","target":"hs-d1"}"#),
    )));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]);
    assert_eq!(out.exit, 0, "a dropped proposal is not an error: {}{}", out.stdout, out.stderr);
    assert_eq!(
        curation_rows(&fx).iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        ["hs-d1", "hs-d2", "hs-d3", "hs-n4"],
        "the curations are written"
    );
    assert_eq!(
        retirement_rows(&fx),
        [
            (s("hs-d2"), s("duplicate"), s("hs-d1"), s("approved")),
            (s("hs-n4"), s("duplicate"), s("hs-d1"), s("pending")),
        ],
        "hs-d3's proposal is dropped: its target left force"
    );
    let events = fx.cpd(&["events", "list", "--kind", "context-curated"]).ok()["events"].as_array().unwrap().clone();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["data"], json!({"context": CONTEXT, "curated": 2, "retire": 1, "docs": 0}), "only the written ones count");
}

#[test]
fn batch_n_sends_at_most_n_records() {
    let fx = fx().one_step();
    for n in 1..=12 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100 - n), &format!("Note number {n}."), Draft::Note);
    }
    fx.env.fake_pi(&pi_keeps_input_and_says(common::CURATES_ALL));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT, "--batch", "5"]).ok();
    assert_eq!(out["distilled"], true, "{out}");
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n1", "hs-n2", "hs-n3", "hs-n4", "hs-n5"], "the 5 oldest");
    assert_eq!(curation_rows(&fx).len(), 5);
    // `--batch 1`: the next oldest alone; the default is 60 (all that is left).
    fx.cpd(&["context", "distill", "--context", CONTEXT, "--batch", "1"]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n6"]);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")).len(), 6);
    assert_eq!(curation_rows(&fx).len(), 12);
}

#[test]
fn batch_md_stops_at_49152_bytes_and_always_holds_the_first_record() {
    let fx = fx().one_step();
    // Six records of 10000 bytes: four fit under 49152 bytes of text, the fifth would make 50000.
    for n in 1..=6 {
        fx.insert(CONTEXT, &format!("hs-n{n}"), &ago(100 - n), &"x".repeat(10_000), Draft::Note);
    }
    fx.env.fake_pi(&pi_keeps_input_and_says(common::CURATES_ALL));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n1", "hs-n2", "hs-n3", "hs-n4"], "the bytes bound the batch, not the 60");
    // The rest goes in the next run.
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n5", "hs-n6"]);

    // One record over the bound is taken alone.
    fx.insert(CONTEXT, "hs-n7", &ago(50), &"y".repeat(60_000), Draft::Note);
    fx.insert(CONTEXT, "hs-n8", &ago(40), "Small.", Draft::Note);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n7"], "the first record is always taken, and goes alone");
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n8"]);
}
