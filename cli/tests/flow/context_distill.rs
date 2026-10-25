//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run") and the engine's debounced
//! rebuild (contract §3.7 E9), run as the built binary with a fake `pi` on `PATH`.

use crate::common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Child, Stdio};

use common::fx::Fx as InProcess;
use common::{CPD, Run, pi_says, TestEnv, refused, wait_until};
use cpd_core::context::{By, Draft, PlanRef, Record};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, RecordId};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

const CONTEXT: &str = "ctx";

struct Fx {
    env: TestEnv,
}

/// The config, the fake `pi`, `node`, `gh` and `cpd` and pi's install directory of every test here.
fn configure(env: &TestEnv) {
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\ncontext_curation_min_records = 1\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_pi(&pi_says(common::CURATES_ALL));
    // pi's catalog (G has the one level off).
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
}

fn fx() -> Fx {
    let env = TestEnv::new();
    configure(&env);
    let fx = Fx { env };
    fx.cpd(&["context", "new", CONTEXT, "--title", "Ctx", "--owner", "acme"]).ok();
    fx
}

/// [`fx`] with the command tree in this process: for a test that never reaches `pi` or another process.
fn fast() -> InProcess {
    let fx = InProcess::new();
    configure(&fx.env);
    fx.cpd(&["context", "new", CONTEXT, "--title", "Ctx", "--owner", "acme"]).ok();
    fx
}

/// What the helpers below need of a fixture, whichever way it runs `cpd`.
trait Distill {
    fn env(&self) -> &TestEnv;
}

impl Distill for Fx {
    fn env(&self) -> &TestEnv {
        &self.env
    }
}

impl Distill for InProcess {
    fn env(&self) -> &TestEnv {
        &self.env
    }
}

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

/// Writes a record straight into the index with a chosen time (the commands write `at` as now).
fn insert(env: &TestEnv, context: &str, id: &str, at: &str, text: &str, draft: Draft) {
    let record = Record { id: RecordId::parse(id).unwrap(), at: at.into(), by: By { session: None, plan: None }, text: text.into(), draft };
    let context = ContextId::parse(context).unwrap();
    store(env).write("fixture", |tx| Ok(record.insert(tx, &context).unwrap())).unwrap();
}

fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

impl Fx {
    /// The run is one step long (hs-d562): the distiller starts on GX, its default, and never climbs.
    fn one_step(self) -> Fx {
        self
    }

    fn cpd(&self, args: &[&str]) -> Run {
        self.env.cpd(args)
    }

    fn home(&self) -> CpdHome {
        home(&self.env)
    }

    fn store(&self) -> Store {
        store(&self.env)
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

    fn insert(&self, context: &str, id: &str, at: &str, text: &str, draft: Draft) {
        insert(&self.env, context, id, at, text, draft);
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
    assert_eq!(out, json!({"ok": true, "context": CONTEXT, "skipped": "young", "records": 0, "floor": 1}), "{out}");
    assert_eq!(fx.pi_calls(), 0);
    assert!(fx.digest_files().is_empty());

    floor_notes(&fx, 1);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["already"].clone(), out["digest"].clone()), (json!(true), Value::Null));
    assert_eq!(fx.pi_calls(), 1);
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
    let fx = fast();
    refused(&fx.cpd(&["context", "distill"]), "context_required", 2);
    refused(&fx.cpd(&["context", "distill", "--context", "nope"]), "context_not_found", 4);
}

// ---- pi's catalog (hs-d560: one step, no ladder) ----

const LADDER_LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium","high","xhigh","max"],"H":["off","low","medium","high"],"G":["off"]}"#;

// ---- The curating distill step (contract §4.6 "The distiller curates", "The distill run"; hs-d549, hs-d551) ----

/// The rows of `record_curations`: `(record, topic, short, doc_candidate)`, by record.
fn curation_rows(fx: &impl Distill) -> Vec<(String, String, String, Option<String>)> {
    fx.env().read(|conn| {
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
    assert!(!prompt.contains("Contradictions"), "{prompt}");

    fx.env.fake_pi(&pi_says(common::CURATES_ALL));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let runs = fx.runs();
    assert_eq!(runs.len(), 2, "one line per pi run: {runs:?}");
    for run in &runs {
        assert_eq!(run["kind"], "distill", "{run}");
        assert!(run.get("contradictions").is_none() && run.get("contradictions_dropped").is_none(), "{run}");
        assert_eq!(run.as_object().unwrap().len(), 12, "{run}");
    }
}

// ---- The fixes FX1 to FX4 (hs-d549, hs-d551) ----

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

// ---- Per-line acceptance (GXD1 to GXD3, hs-d563) ----

/// The `rejected` of a run's JSON, as `(id, line)`.
fn rejected_of(value: &Value) -> Vec<(Option<String>, u64)> {
    let list = value["rejected"].as_array().unwrap_or_else(|| panic!("`rejected` is an array: {value}"));
    list.iter()
        .map(|r| {
            assert!(r["what"].as_str().is_some_and(|w| !w.is_empty()), "every rejection says why: {r}");
            (r["id"].as_str().map(str::to_owned), r["line"].as_u64().unwrap_or_else(|| panic!("`line` is a number: {r}")))
        })
        .collect()
}

/// `n` notes `hs-n1` to `hs-nN`, oldest first.
fn notes(fx: &Fx, n: i64) {
    for i in 1..=n {
        fx.insert(CONTEXT, &format!("hs-n{i}"), &ago(100 - i), &format!("Note {i}."), Draft::Note);
    }
}

fn curated_ids(fx: &Fx) -> Vec<String> {
    curation_rows(fx).into_iter().map(|r| r.0).collect()
}

fn invalid_file(fx: &Fx) -> PathBuf {
    fx.digests().join(format!("{}.jsonl.invalid", fx.highest_seq()))
}

/// The `context-curated` Events' data.
fn curated_events(fx: &Fx) -> Vec<Value> {
    let events = fx.cpd(&["events", "list", "--kind", "context-curated"]).ok()["events"].as_array().unwrap().clone();
    events.into_iter().map(|e| e["data"].clone()).collect()
}

#[test]
fn a_short_over_120_bytes_rejects_its_line_and_the_other_lines_are_curated() {
    let fx = fx().one_step();
    notes(&fx, 4);
    // hs-n2: 121 bytes. hs-n3: 120 bytes of 3-byte characters... 40 of them, which is 120 bytes and valid.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "notes", "one", "null"),
        line("hs-n2", "notes", &"x".repeat(121), "null"),
        line("hs-n3", "notes", &"€".repeat(40), "null"),
        line("hs-n4", "notes", "four", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["curated"].clone(), rejected_of(&out)), (json!(3), vec![(Some(s("hs-n2")), 2)]), "{out}");
    assert_eq!(curated_ids(&fx), ["hs-n1", "hs-n3", "hs-n4"], "hs-n2 stays uncurated");
    assert_eq!(curated_events(&fx).len(), 1);
    assert_eq!(curated_events(&fx)[0]["curated"], 3);
    assert!(fx.errors().is_empty(), "a run with accepted lines is no failure");
    // The next run takes the rejected record.
    fx.env.fake_pi(&pi_answers(&[line("hs-n2", "notes", "two", "null")]));
    let again = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((again["curated"].clone(), rejected_of(&again)), (json!(1), vec![]), "{again}");
    assert_eq!(curation_rows(&fx).len(), 4);
}

#[test]
fn a_duplicate_target_newer_than_the_record_rejects_only_that_line() {
    let fx = fx().one_step();
    two_decisions(&fx);
    fx.insert(CONTEXT, "hs-n3", &ago(70), "Locks are flock.", Draft::Note);
    // hs-d1 (older) a duplicate of hs-d2 (newer): the first is kept, so that line is rejected.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", r#"{"reason":"duplicate","target":"hs-d2"}"#),
        line("hs-d2", "storage", "sqlite WAL", "null"),
        line("hs-n3", "locking", "flock", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["curated"].clone(), rejected_of(&out)), (json!(2), vec![(Some(s("hs-d1")), 1)]), "{out}");
    let what = out["rejected"][0]["what"].as_str().unwrap();
    assert!(what.contains("hs-d1") && what.contains("hs-d2"), "the reason names both ids: {what}");
    assert_eq!(curated_ids(&fx), ["hs-d2", "hs-n3"]);
    assert_eq!(retirement_rows(&fx), vec![], "the rejected line proposes nothing");

    // The newer one a duplicate of the older is accepted.
    fx.env.fake_pi(&pi_answers(&[line("hs-d1", "storage", "sqlite", "null")]));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(curation_rows(&fx).len(), 3);
}

#[test]
fn two_records_naming_each_other_as_duplicates_keep_only_the_newer_records_line() {
    let fx = fx().one_step();
    two_decisions(&fx);
    fx.insert(CONTEXT, "hs-n3", &ago(70), "Locks are flock.", Draft::Note);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", r#"{"reason":"duplicate","target":"hs-d2"}"#),
        line("hs-d2", "storage", "sqlite WAL", r#"{"reason":"duplicate","target":"hs-d1"}"#),
        line("hs-n3", "locking", "flock", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["curated"].clone(), rejected_of(&out)), (json!(2), vec![(Some(s("hs-d1")), 1)]), "{out}");
    assert_eq!(curated_ids(&fx), ["hs-d2", "hs-n3"]);
    assert_eq!(retirement_rows(&fx), [(s("hs-d2"), s("duplicate"), s("hs-d1"), s("pending"))], "two records never retire each other");
}

#[test]
fn a_topic_outside_lowercase_digits_and_hyphen_rejects_only_that_line() {
    let fx = fx().one_step();
    notes(&fx, 4);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "storage-2", "digits and hyphen", "null"),
        line("hs-n2", "Storage", "upper case", "null"),
        line("hs-n3", &"a".repeat(40), "forty characters", "null"),
        line("hs-n4", "snake_case", "an underscore", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(rejected_of(&out), [(Some(s("hs-n2")), 2), (Some(s("hs-n4")), 4)], "{out}");
    assert_eq!(out["curated"], 2);
    assert_eq!(curated_ids(&fx), ["hs-n1", "hs-n3"]);

    // Over 40 characters, a space, and a non-ASCII topic are each one rejected line.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n2", &"a".repeat(41), "41 characters", "null"),
        line("hs-n4", "two words", "a space", "null"),
    ]));
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(curation_rows(&fx).len(), 2);
}

#[test]
fn a_superseded_target_that_is_not_newer_rejects_only_that_line() {
    let fx = fx().one_step();
    two_decisions(&fx);
    fx.insert(CONTEXT, "hs-n3", &ago(70), "Locks are flock.", Draft::Note);
    // hs-d2 is superseded by hs-d1, which is older (a lower seq): that line is rejected.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", "null"),
        line("hs-d2", "storage", "sqlite WAL", r#"{"reason":"superseded","target":"hs-d1"}"#),
        line("hs-n3", "locking", "flock", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["curated"].clone(), rejected_of(&out)), (json!(2), vec![(Some(s("hs-d2")), 2)]), "{out}");
    assert_eq!(curated_ids(&fx), ["hs-d1", "hs-n3"]);
    assert_eq!(retirement_rows(&fx), vec![]);

    // The other way round is accepted.
    fx.env.fake_pi(&pi_answers(&[line("hs-d2", "storage", "sqlite WAL", "null")]));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(curation_rows(&fx).len(), 3);
}

#[test]
fn a_documented_target_that_is_not_a_grep_hit_rejects_only_that_line() {
    let fx = fx().one_step();
    two_decisions(&fx);
    fx.insert(CONTEXT, "hs-n3", &ago(70), "Locks are flock.", Draft::Note);
    let a = repo_with(&fx, "a", &[("docs/design.md", "We use sqlite, see hs-d1.\n"), ("docs/wal.md", "WAL: hs-d2.\n")]);
    plan_with_repos(&fx, &[&a]);
    let (own, other) = (format!("{}:docs/wal.md", a.display()), format!("{}:docs/design.md", a.display()));
    // hs-d1 names a path that is no hit; hs-d2 names a real hit of its own; hs-n3 names a hit of hs-d1.
    fx.env.fake_pi(&pi_answers(&[
        line("hs-d1", "storage", "sqlite", &format!(r#"{{"reason":"documented","target":"{}:docs/invented.md"}}"#, a.display())),
        line("hs-d2", "storage", "sqlite WAL", &format!(r#"{{"reason":"documented","target":"{own}"}}"#)),
        line("hs-n3", "locking", "flock", &format!(r#"{{"reason":"documented","target":"{other}"}}"#)),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(rejected_of(&out), [(Some(s("hs-d1")), 1), (Some(s("hs-n3")), 3)], "{out}");
    assert_eq!(out["curated"], 1);
    assert_eq!(curated_ids(&fx), ["hs-d2"]);
    assert_eq!(retirement_rows(&fx), [(s("hs-d2"), s("documented"), own, s("pending"))]);
}

#[test]
fn an_unknown_id_is_ignored_and_a_repeated_id_leaves_its_record_uncurated() {
    let fx = fx().one_step();
    notes(&fx, 4);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "notes", "one", "null"),
        line("hs-n1", "notes", "one again", "null"),
        line("hs-zz", "notes", "unknown", "null"),
        line("hs-n2", "notes", "two", "null"),
    ]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["curated"], 1, "{out}");
    assert_eq!(curated_ids(&fx), ["hs-n2"], "hs-n1 twice: neither line; hs-zz: ignored; hs-n3 and hs-n4: missing");
    let rejected = rejected_of(&out);
    assert_eq!(rejected.iter().filter(|(id, _)| id.as_deref() == Some("hs-n1")).map(|(_, l)| *l).collect::<Vec<_>>(), [1, 2], "both lines of the repeated id");
    assert!(!rejected.iter().any(|(id, _)| id.as_deref() == Some("hs-zz")), "an unknown id is ignored, not rejected: {rejected:?}");
    assert_eq!(curated_events(&fx)[0]["curated"], 1);
}

#[test]
fn a_record_missing_from_the_output_stays_uncurated_and_the_run_succeeds() {
    let fx = fx().one_step();
    notes(&fx, 3);
    fx.env.fake_pi(&pi_answers(&[line("hs-n1", "notes", "one", "null"), line("hs-n3", "notes", "three", "null")]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["curated"].clone(), out["remaining"].clone()), (json!(2), json!(1)), "{out}");
    assert_eq!(curated_ids(&fx), ["hs-n1", "hs-n3"]);
    // The next run takes it.
    fx.env.fake_pi(&pi_keeps_input_and_says(&format!("echo '{}'", line("hs-n2", "notes", "two", "null"))));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(block_ids(&seen(&fx, "batch.md")), ["hs-n2"]);
    assert_eq!(curation_rows(&fx).len(), 3);
}

#[test]
fn a_run_with_no_accepted_line_is_distill_invalid_listing_every_rejection() {
    let fx = fx().one_step();
    notes(&fx, 3);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "Notes", "bad topic", "null"),
        line("hs-n2", "notes", &"x".repeat(121), "null"),
        "this is not JSON".to_owned(),
    ]));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let upto = fx.highest_seq();
    let details = &error["details"];
    assert_eq!((details["context"].clone(), details["upto_seq"].clone()), (json!(CONTEXT), json!(upto)));
    let errors = details["errors"].as_array().unwrap_or_else(|| panic!("details.errors: {details}"));
    let listed: Vec<(Option<&str>, Option<u64>)> = errors.iter().map(|e| (e["id"].as_str(), e["line"].as_u64())).collect();
    for want in [(Some("hs-n1"), Some(1)), (Some("hs-n2"), Some(2)), (None, Some(3))] {
        assert!(listed.contains(&want), "{want:?} is listed: {listed:?}");
    }
    assert_eq!(details["invalid"], invalid_file(&fx).display().to_string());
    assert!(fs::read_to_string(invalid_file(&fx)).unwrap().contains("this is not JSON"));
    assert_eq!((curation_rows(&fx), retirement_rows(&fx)), (vec![], vec![]));
    assert_eq!(fx.errors().len(), 1);
    assert!(curated_events(&fx).is_empty());
    let runs = fx.runs();
    assert_eq!(rejected_of(&runs[0]).len(), 3, "the run log lists the rejections of a failed run too: {runs:?}");
}

#[test]
fn the_rejected_lines_are_in_the_output_the_run_log_and_the_kept_raw_text() {
    let fx = fx().one_step();
    notes(&fx, 3);
    let answer = [
        line("hs-n1", "notes", "one", "null"),
        line("hs-n2", "notes", &"x".repeat(121), "null"),
        line("hs-n3", "notes", "three", "null"),
    ];
    fx.env.fake_pi(&pi_answers(&answer));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(rejected_of(&out), [(Some(s("hs-n2")), 2)], "{out}");
    assert_eq!(out["rejected"][0]["id"], "hs-n2");
    let runs = fx.runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!((runs[0]["result"].clone(), runs[0]["kind"].clone()), (json!("valid"), json!("distill")), "{runs:?}");
    assert_eq!(rejected_of(&runs[0]), [(Some(s("hs-n2")), 2)], "the run log carries the same list");
    let raw = fs::read_to_string(invalid_file(&fx)).unwrap_or_else(|e| panic!("the raw text is kept: {e}"));
    assert!(raw.contains(&answer[1]) && raw.contains(&answer[0]), "{raw}");
    assert_eq!(curated_events(&fx)[0]["rejected"], 1);
}

#[test]
fn a_clean_run_removes_the_kept_raw_text() {
    let fx = fx().one_step();
    notes(&fx, 3);
    fx.env.fake_pi(&pi_answers(&[
        line("hs-n1", "notes", "one", "null"),
        line("hs-n2", "notes", &"x".repeat(121), "null"),
        line("hs-n3", "notes", "three", "null"),
    ]));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let kept = invalid_file(&fx);
    assert!(kept.exists(), "a run with a rejected line keeps its raw text");
    // The next run takes hs-n2 and is clean: the kept file goes, replaced by nothing.
    fx.env.fake_pi(&pi_answers(&[line("hs-n2", "notes", "two", "null")]));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(rejected_of(&out), vec![], "{out}");
    assert!(!kept.exists(), "{kept:?}");
    assert!(!fx.digests().read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".jsonl.invalid")));
    let runs = fx.runs();
    assert_eq!(rejected_of(runs.last().unwrap()), vec![], "{runs:?}");
}

/// A fake `pi` whose last message stopped with `length` after `answer` (shell).
fn pi_cut_after(answer: &str) -> String {
    pi_says(answer).replace(r#""stopReason":"stop""#, r#""stopReason":"length""#)
}

#[test]
fn an_answer_cut_at_the_output_limit_keeps_its_complete_lines_and_logs_its_cost() {
    let fx = fx().one_step();
    notes(&fx, 4);
    // Two complete lines, a third cut in the middle (no newline after it).
    fx.env.fake_pi(&pi_cut_after(&format!(
        "cat <<'EOF'\n{}\n{}\nEOF\nprintf '%s' '{{\"id\":\"hs-n3\",\"topic\":\"no'",
        line("hs-n1", "notes", "one", "null"),
        line("hs-n2", "notes", "two", "null"),
    )));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["curated"], 2, "{out}");
    assert!(out["cut"] == json!(true) || out["run"]["cut"] == json!(true), "the cut is noted in the output: {out}");
    assert_eq!(curated_ids(&fx), ["hs-n1", "hs-n2"], "hs-n3 and hs-n4 stay uncurated");
    assert!(rejected_of(&out).iter().all(|(id, _)| id.as_deref() != Some("hs-n3")), "the cut line is dropped, not a rejection: {out}");
    assert!(invalid_file(&fx).exists(), "a cut output keeps its raw text");
    let runs = fx.runs();
    assert_eq!((runs[0]["result"].clone(), runs[0]["tokens_in"].clone(), runs[0]["cost"].clone()), (json!("valid"), json!(733), json!(0.25)));
}

#[test]
fn a_cut_answer_with_no_complete_line_is_distill_failed() {
    let fx = fx().one_step();
    notes(&fx, 2);
    // Text with no newline at all.
    fx.env.fake_pi(&pi_cut_after("printf '%s' '{\"id\":\"hs-n1\",\"topic\":\"no'"));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!(error["details"]["reason"], "output");
    assert!(error["message"].as_str().unwrap().contains("length"), "{error}");
    assert_eq!(curation_rows(&fx), vec![]);
    let runs = fx.runs();
    assert_eq!((runs[0]["result"].clone(), runs[0]["tokens_in"].clone(), runs[0]["cost"].clone()), (json!("failed"), json!(733), json!(0.25)));

    // No text at all.
    fx.env.fake_pi(&pi_cut_after("true"));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!(error["details"]["reason"], "output");
    assert_eq!(curation_rows(&fx), vec![]);

    // A single complete line, cut right after its newline: nothing is dropped, and it is an answer.
    fx.env.fake_pi(&pi_cut_after(&format!("echo '{}'", line("hs-n1", "notes", "one", "null"))));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["curated"], 1, "{out}");
    assert_eq!(curated_ids(&fx), ["hs-n1"]);
}

// ---- The distiller on GX (hs-d562): contract §4.6, §9 ----

#[test]
fn the_distiller_runs_on_gx_with_the_g_model() {
    let fx = fx();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let tiers: Vec<String> = fx.runs().iter().map(|r| r["tier"].as_str().unwrap().to_string()).collect();
    assert_eq!(tiers, ["GX"]);
    let calls = fx.env.calls("pi");
    let at = calls[0].iter().position(|a| a == "--model").expect("--model") + 1;
    assert_eq!(calls[0][at], "github-copilot/gemini-3.8-flash:off", "{:?}", calls[0]);
}

#[test]
fn a_failed_pi_run_fails_the_run_with_no_second_step() {
    let fx = fx();
    fx.insert(CONTEXT, "hs-d1", &ago(90), "Use sqlite.", Draft::Decision { supersedes: None });
    fx.env.fake_pi("echo 'model unavailable' >&2; exit 7");
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!(fx.pi_calls(), 1, "no next step, no higher Tier");
    assert_eq!(fx.runs().len(), 1);
    let events = fx.errors();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["data"]["code"], "distill_failed");
}

// ---- The curation floor (contract §8.1, §8.2, §4.6; hs-d592) ----

/// Sets `context_curation_min_records` in the fixture's config.toml.
fn set_floor(fx: &impl Distill, floor: u32) {
    let path = fx.env().cpd_home().join("config.toml");
    let text: String = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| if l.starts_with("context_curation_min_records = ") { format!("context_curation_min_records = {floor}\n") } else { format!("{l}\n") })
        .collect();
    assert!(text.contains(&format!("context_curation_min_records = {floor}\n")), "{text}");
    fs::write(path, text).unwrap();
}

/// `n` notes in the Context, oldest first.
fn floor_notes(fx: &impl Distill, n: usize) {
    for i in 1..=n {
        insert(fx.env(), CONTEXT, &format!("hs-n{i}"), &ago(100), &format!("Note number {i}."), Draft::Note);
    }
}

fn events_count(fx: &impl Distill) -> i64 {
    fx.env().read(|conn| conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0)))
}

fn files_named(dir: &std::path::Path, names: &[&str], found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_named(&path, names, found);
        } else if names.contains(&path.file_name().unwrap().to_str().unwrap()) {
            found.push(path);
        }
    }
}

/// Nothing a skipped run must not leave: no pi call, no row, no Event (compared with `events_before`), no lock or log file.
fn assert_nothing_done(fx: &impl Distill, events_before: i64) {
    assert_eq!(fx.env().calls("pi").len(), 0, "the fake pi was never called");
    assert_eq!(curation_rows(fx), vec![], "no record_curations row");
    assert_eq!(events_count(fx), events_before, "no Event appended");
    let mut found = Vec::new();
    files_named(&fx.env().cpd_home(), &["distill.lock", "distill.log"], &mut found);
    files_named(&fx.env().home(), &["distill.lock", "distill.log"], &mut found);
    assert_eq!(found, Vec::<PathBuf>::new(), "no lock or log file");
}

fn archive_context(fx: &impl Distill) {
    store(fx.env())
        .write("fixture", |tx| {
            tx.execute("UPDATE contexts SET archived_at = '2026-10-20T10:00:00.000Z' WHERE id = ?1", [CONTEXT])?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
}

#[test]
fn an_archived_context_is_skipped_by_distill() {
    let fx = fast();
    set_floor(&fx, 1);
    floor_notes(&fx, 3);
    archive_context(&fx);
    let before = events_count(&fx);

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out, json!({"ok": true, "context": CONTEXT, "skipped": "archived"}), "{out}");
    assert_nothing_done(&fx, before);

    // Archived is checked first: under the floor it still reports `archived`.
    set_floor(&fx, 50);
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out, json!({"ok": true, "context": CONTEXT, "skipped": "archived"}), "{out}");
    assert_nothing_done(&fx, before);
}

#[test]
fn a_context_under_the_floor_is_skipped_by_distill() {
    let fx = fast();
    set_floor(&fx, 5);
    floor_notes(&fx, 4);
    let before = events_count(&fx);

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out, json!({"ok": true, "context": CONTEXT, "skipped": "young", "records": 4, "floor": 5}), "{out}");
    assert_nothing_done(&fx, before);
}

#[test]
fn a_context_at_the_floor_is_curated_by_distill() {
    let fx = fx().one_step();
    floor_notes(&fx, 5);
    // One above the count: skipped; at the count: curated (a changed floor applies at the next run).
    set_floor(&fx, 6);
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out, json!({"ok": true, "context": CONTEXT, "skipped": "young", "records": 5, "floor": 6}), "{out}");
    assert_eq!(fx.pi_calls(), 0);
    set_floor(&fx, 5);

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert!(out.get("skipped").is_none(), "{out}");
    assert_eq!(fx.pi_calls(), 1);
    assert_eq!(curation_rows(&fx).len(), 5);
    let events = fx.cpd(&["events", "list", "--kind", "context-curated"]).ok()["events"].as_array().unwrap().clone();
    assert_eq!(events.len(), 1, "{events:?}");
}

#[test]
fn the_floor_counts_retired_and_superseded_records() {
    let fx = fx().one_step();
    set_floor(&fx, 5);
    // 4 records: under the floor, skipped.
    floor_notes(&fx, 4);
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["skipped"], "young", "{out}");
    assert_eq!(fx.pi_calls(), 0);

    // The fifth record supersedes hs-n1; hs-n2 is retired by an approved retirement: 5 rows, 3 in force.
    fx.insert(CONTEXT, "hs-d5", &ago(50), "Newer.", Draft::Decision { supersedes: Some(RecordId::parse("hs-n1").unwrap()) });
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at)
                 VALUES ('ctx', 'hs-n2', 'duplicate', 'hs-n3', 'pending', 't')",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
    fx.cpd(&["context", "retire", "--context", CONTEXT, "--approve", "hs-n2"]).ok();

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert!(out.get("skipped").is_none(), "retired and superseded records count: {out}");
    assert_eq!(fx.pi_calls(), 1);
}
