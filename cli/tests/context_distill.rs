//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run") and the engine's debounced
//! rebuild (contract §3.7 E9), run as the built binary with a fake `pi` on `PATH`.

mod common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Child, Stdio};

use common::{CPD, Run, TestEnv, refused, wait_until};
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

/// A fake `pi` whose digest is the header line of every record in `records.md` (each cites its record), minus
/// those of the ids in `$OMIT` (space separated).
const CITES_ALL: &str = r##"echo "# Digest"
while IFS= read -r line; do
  case "$line" in "["*) id=${line%%]*}; id=${id#[}
    case " $OMIT " in *" $id "*) ;; *) echo "- $line" ;; esac ;;
  esac
done < records.md"##;

struct Fx {
    env: TestEnv,
}

fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = 1\npr_poll_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_pi(CITES_ALL);
    // The engine's E5 runs `cpd pr poll` from `PATH` at every tick, through a `gh` that finds no pull requests: the
    // poll searches count whole ticks (`wait_whole_ticks`).
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    let fx = Fx { env };
    fx.cpd(&["context", "new", CONTEXT, "--title", "Ctx"]).ok();
    fx
}

fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

impl Fx {
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
            .filter(|n| !["prompt.md", "records.md", "distill.log"].contains(&n.as_str()))
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
        self.store()
            .read(|conn| Ok(conn.query_row("SELECT max(seq) FROM context_records WHERE context = ?1", [CONTEXT], |r| r.get::<_, i64>(0))?))
            .unwrap() as u64
    }

    fn pi_calls(&self) -> usize {
        self.env.calls("pi").len()
    }

    /// The `error` Events of `context distill`.
    fn errors(&self) -> Vec<Value> {
        let events = self.cpd(&["events", "list", "--kind", "error"]).ok()["events"].as_array().unwrap().clone();
        events.into_iter().filter(|e| e["data"]["source"] == "context distill").collect()
    }
}

#[test]
fn a_valid_digest_lands_as_upto_seq_and_the_directory_holds_one() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    let _ = fx.decide("Use flock.");
    let q = fx.cpd(&["context", "ask", "Which port?", "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned();
    fx.cpd(&["context", "decide", "Use sqlite for sure.", "--supersedes", &d1, "--context", CONTEXT]).ok();
    fs::create_dir_all(fx.digests()).unwrap();
    fs::write(fx.digests().join("1.md.tmp"), "left over").unwrap();
    fs::write(fx.digests().join("2.md"), "an older digest").unwrap();

    let upto = fx.highest_seq();
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!((out["distilled"].clone(), out["records_in_force"].clone()), (json!(true), json!(3)), "{out}");
    let file = fx.digests().join(format!("{upto}.md"));
    assert_eq!(out["digest"]["upto_seq"], upto);
    assert_eq!(out["digest"]["file"], file.display().to_string());
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains(&format!("[{q}]")) && !text.contains(&format!("[{d1}]")), "{text}");
    assert_eq!(fx.digest_files(), [format!("{upto}.md")], "older digests and temporary files are gone");
    assert!(fx.errors().is_empty());

    // The distiller's argv and inputs: print mode, no session, no extension, skill, context file or tool; two files.
    let level = Tier::parse("S2").unwrap().level();
    let model = format!("p/sonnet:{level}");
    let calls = fx.env.calls("pi");
    assert_eq!(
        calls,
        [vec![
            "--print", "--no-session", "--no-extensions", "--no-skills", "--no-context-files", "--no-tools", "--model", &model, "--",
            "@prompt.md", "@records.md",
        ]]
    );
    let records = fs::read_to_string(fx.digests().join("records.md")).unwrap();
    assert!(records.contains("Which port?") && records.contains("Use sqlite for sure.") && !records.contains("Use sqlite.\n"), "{records}");
    assert!(fs::read_to_string(fx.digests().join("prompt.md")).unwrap().contains("32 KB"));

    let shown = fx.cpd(&["context", "show", "--digest", "--context", CONTEXT]).ok();
    assert_eq!((shown["digest"]["upto_seq"].clone(), shown["markdown"].clone()), (json!(upto), json!(text)));

    // Nothing newer: nothing runs.
    let again = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(fx.pi_calls(), 1);
}

#[test]
fn the_distiller_gets_no_cpd_variable() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi(&format!("env > \"$HOME/pi.env\"\n{CITES_ALL}"));
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
fn an_invalid_digest_is_discarded_with_one_error_event_and_the_previous_one_stays() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let before = fx.digest_files();
    let kept = fs::read_to_string(fx.digests().join(&before[0])).unwrap();

    let d2 = fx.decide("Use flock.");
    fx.env.fake_pi("echo '# Digest'; echo 'Cites a ghost [hs-d99] and nothing else.'");
    let run = fx.cpd(&["context", "distill", "--context", CONTEXT]);
    let error = refused(&run, "distill_invalid", 1);
    let details = &error["details"];
    assert_eq!(details["context"], CONTEXT);
    assert_eq!(details["upto_seq"], fx.highest_seq());
    assert_eq!(details["limit"], 32768);
    assert_eq!(details["stray"], json!(["hs-d99"]));
    assert!(details["missing"].as_array().unwrap().iter().any(|id| *id == json!(d2)), "{details}");
    assert!(details["log"].as_str().unwrap().ends_with("distill.log"));

    assert_eq!(fx.digest_files(), before, "the previous digest stays and no .md.tmp is left");
    assert_eq!(fs::read_to_string(fx.digests().join(&before[0])).unwrap(), kept);
    let events = fx.errors();
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(event["data"]["code"], "distill_invalid");
    assert_eq!(event["data"]["details"]["stray"], json!(["hs-d99"]));
    assert_eq!((event["context"].clone(), event["plan"].clone(), event["session"].clone()), (Value::Null, Value::Null, Value::Null));

    // A rerun runs again, and a good answer then lands.
    fx.env.fake_pi(CITES_ALL);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(fx.digest_files(), [format!("{}.md", fx.highest_seq())]);
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn a_digest_over_the_limit_is_invalid() {
    let fx = fx();
    let d = fx.decide("Use sqlite.");
    fx.env.fake_pi(&format!("echo '[{d}]'; i=0; while [ $i -lt 400 ]; do printf '%0100d\\n' 0; i=$((i+1)); done"));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert!(error["details"]["bytes"].as_u64().unwrap() > 32768, "{error}");
    assert_eq!((error["details"]["missing"].clone(), error["details"]["stray"].clone()), (json!([]), json!([])));
    assert!(fx.digest_files().is_empty());
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn a_failing_pi_is_distill_failed_with_its_stderr_tail() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi("echo 'half an answer'; echo 'model unavailable' >&2; exit 7");
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    let details = &error["details"];
    assert_eq!((details["reason"].clone(), details["exit"].clone()), (json!("exit"), json!(7)), "{details}");
    assert_eq!(details["tail"], "model unavailable");
    assert_eq!(details["upto_seq"], fx.highest_seq());
    assert!(fx.digest_files().is_empty(), "no .md.tmp is left");
    let events = fx.errors();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["code"], "distill_failed");
    assert_eq!(events[0]["context"], Value::Null);
    let log = fs::read_to_string(details["log"].as_str().unwrap()).unwrap();
    assert!(log.contains("model unavailable"));
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
}

#[test]
fn only_plan_records_since_the_digest_carry_it_forward_without_pi() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let old = fx.digest_files();
    let text = fs::read_to_string(fx.digests().join(&old[0])).unwrap();
    let plan = PlanRef { id: PlanId::parse("alpha").unwrap(), machine: MachineName::parse("hs").unwrap(), jira: None };
    fx.insert(CONTEXT, "hs-p1", &ago(0), "Plan alpha.", Draft::Plan(plan));

    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(out["distilled"], false, "{out}");
    assert_eq!(out["digest"]["upto_seq"], fx.highest_seq());
    assert_eq!(fx.digest_files(), [format!("{}.md", fx.highest_seq())]);
    assert_eq!(fs::read_to_string(fx.digests().join(&fx.digest_files()[0])).unwrap(), text);
    assert_eq!(fx.pi_calls(), 1, "pi ran once, for the first digest");
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

/// D7 (hs-d192): the ids `docs/research/unplanned.md` lists, in the MISSING items' headings and the Records
/// column of "Owned by a record only".
fn unplanned_ids() -> Vec<String> {
    let doc = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/unplanned.md")).expect("read unplanned.md");
    let ids_in = |text: &str| -> Vec<String> {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .filter(|w| {
                w.strip_prefix("hs-").is_some_and(|rest| {
                    rest.len() > 1 && rest.as_bytes()[0].is_ascii_lowercase() && rest[1..].bytes().all(|b| b.is_ascii_digit())
                })
            })
            .map(str::to_owned)
            .collect()
    };
    let missing = doc.split("\n## MISSING\n").nth(1).expect("a MISSING section").split("\n## ").next().unwrap();
    let owned = doc.split("\n## Owned by a record only").nth(1).expect("an Owned section").split("\n## ").next().unwrap();
    let mut ids = Vec::new();
    for line in missing.lines().filter(|l| l.starts_with("**")) {
        ids.extend(ids_in(line.rsplit("**").next().unwrap()));
    }
    for row in owned.lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Work") && !l.starts_with("|---")) {
        ids.extend(ids_in(row.split('|').nth(2).unwrap()));
    }
    ids.sort();
    ids.dedup();
    ids
}

#[test]
fn nothing_the_audit_lists_is_lost() {
    let ids = unplanned_ids();
    assert!(ids.len() >= 10, "the audit's ids were not found: {ids:?}");
    let fx = fx();
    for id in &ids {
        let record = RecordId::parse(id).unwrap();
        let draft = match id.as_bytes()[3] {
            b'd' => Draft::Decision { supersedes: None },
            b'n' => Draft::Note,
            b'q' => Draft::Question,
            b'a' => {
                // An answer needs a question; the audit's own question is in force too.
                fx.insert(CONTEXT, "hs-q9999", &ago(60), "A question.", Draft::Question);
                Draft::Answer { question: RecordId::parse("hs-q9999").unwrap() }
            }
            other => panic!("the audit lists {id}, of kind {}", other as char),
        };
        assert_eq!(record.to_string(), *id);
        fx.insert(CONTEXT, id, &ago(60), &format!("Text of {id}."), draft);
    }
    let run = |omit: &str| {
        let out = fx.env.command(CPD).args(["context", "distill", "--context", CONTEXT]).env("OMIT", omit).output().unwrap();
        Run { exit: out.status.code().unwrap(), stdout: String::from_utf8(out.stdout).unwrap(), stderr: String::new() }
    };
    // A digest that leaves any of them out is refused, naming it.
    for id in &ids {
        let error = refused(&run(id), "distill_invalid", 1);
        assert_eq!(error["details"]["missing"], json!([id]), "{error}");
        assert!(fx.digest_files().is_empty());
    }
    // One that cites them all is accepted.
    let out = run("").ok();
    let text = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    for id in &ids {
        assert!(text.contains(&format!("[{id}]")), "{id}");
    }
}

/// A running `cpd watch`; killed when the test ends.
struct Engine(Option<Child>);

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn start_engine(fx: &Fx) -> Engine {
    Engine(Some(fx.env.command(CPD).arg("watch").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()))
}

fn stop_engine(mut engine: Engine) {
    let mut child = engine.0.take().unwrap();
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(child.id() as i32), nix::sys::signal::Signal::SIGTERM).unwrap();
    child.wait().unwrap();
}

#[test]
fn the_engine_starts_a_run_only_after_the_quiet_period_and_never_two_at_once() {
    let fx = fx();
    // `old`: its newest record is 11 minutes old. `fresh`: written now (the commands stamp the time).
    fx.cpd(&["context", "new", "old", "--title", "Old"]).ok();
    fx.insert("old", "hs-d1", &ago(30), "An old decision.", Draft::Decision { supersedes: None });
    fx.insert("old", "hs-n2", &ago(11), "An old note.", Draft::Note);
    fx.cpd(&["context", "new", "fresh", "--title", "Fresh"]).ok();
    fx.insert("fresh", "hs-d3", &ago(30), "A decision.", Draft::Decision { supersedes: None });
    fx.insert("fresh", "hs-n4", &ago(9), "A note.", Draft::Note);
    fx.cpd(&["context", "new", "archived", "--title", "Archived"]).ok();
    fx.insert("archived", "hs-d5", &ago(60), "A decision.", Draft::Decision { supersedes: None });
    fx.cpd(&["context", "archive", "archived"]).ok();
    fx.cpd(&["context", "new", "quiet", "--title", "Quiet"]).ok();
    fx.insert("quiet", "hs-d6", &ago(60), "A decision.", Draft::Decision { supersedes: None });

    // `quiet`'s run is alive (its lock is held): E9 starts none for it.
    let home = fx.home();
    fs::create_dir_all(home.root().join("locks")).unwrap();
    let held = Flock::lock(File::create(home.distill_lock(&ContextId::parse("quiet").unwrap())).unwrap(), FlockArg::LockExclusiveNonblock).unwrap();

    // A slow pi, so a second start would overlap the first.
    fx.env.fake_pi(&format!("sleep 3\n{CITES_ALL}"));
    let engine = start_engine(&fx);
    let digest = |c: &str| home.context_digest_dir(&ContextId::parse(c).unwrap());
    let has_digest = |c: &str| fs::read_dir(digest(c)).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".md") && e.file_name() != "prompt.md" && e.file_name() != "records.md"));
    wait_until("the digest of `old`", || has_digest("old"));
    // Several more whole ticks: no second run of `old`, no run of the others.
    fx.env.wait_whole_ticks(3);
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "one run in all: {calls:?}");
    assert!(!has_digest("fresh") && !has_digest("archived") && !has_digest("quiet"));
    let log = fs::read_to_string(home.root().join("run/context-digests/old/distill.log")).unwrap();
    assert!(log.contains("\"distilled\":true"), "the run's stdout is in the log: {log}");

    // The lock is free now: `quiet` is rebuilt, exactly once.
    drop(held);
    wait_until("the digest of `quiet`", || has_digest("quiet"));
    fx.env.wait_whole_ticks(2);
    assert_eq!(fx.env.calls("pi").len(), 2);
    assert!(!has_digest("fresh") && !has_digest("archived"));
    stop_engine(engine);
    assert!(fx.errors().is_empty());
}

#[test]
fn the_engine_tries_a_failed_seq_once_until_a_newer_record_arrives() {
    let fx = fx();
    fx.insert(CONTEXT, "hs-d1", &ago(60), "A decision.", Draft::Decision { supersedes: None });
    fx.env.fake_pi("echo 'no model' >&2; exit 1");
    let engine = start_engine(&fx);
    wait_until("the failed run's Event", || fx.errors().len() == 1);
    fx.env.wait_whole_ticks(3);
    assert_eq!((fx.pi_calls(), fx.errors().len()), (1, 1), "not repeated at every tick");

    fx.insert(CONTEXT, "hs-d2", &ago(20), "Another.", Draft::Decision { supersedes: None });
    fx.env.fake_pi(CITES_ALL);
    wait_until("the digest", || fx.digest_files() == [format!("{}.md", fx.highest_seq())]);
    stop_engine(engine);
}
