//! `cpd context distill` (commands.md §3.12, contract §4.6 "The distill run") and the engine's debounced
//! rebuild (contract §3.7 E9), run as the built binary with a fake `pi` on `PATH`.

mod common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv};
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

/// Shell for a fake `pi`: `reply` reads the digest text on stdin and prints pi's `--mode json` stream for a
/// one-shot run (shapes recorded from pi 1.3.0, see `RECORDED`) whose final assistant message holds it, with
/// usage 2 input + 100 cache-read + 631 cache-write = 733 tokens in, 4 out, cost 0.25.
const REPLY: &str = r##"reply() {
  text=$(sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/$/\\n/' | tr -d '\n')
  printf '%s\n' '{"type":"session","version":3,"id":"x","timestamp":"2026-10-19T01:59:25.851Z","cwd":"/"}' '{"type":"agent_start"}'
  printf '{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"%s"}],"usage":{"input":2,"output":4,"cacheRead":100,"cacheWrite":631,"reasoning":1,"totalTokens":737,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0.25}},"stopReason":"stop"}}\n' "$text"
  printf '%s\n' '{"type":"agent_settled","aborted":false}'
}"##;

/// A fake `pi` that prints `digest` (shell) as the final assistant message of a JSON stream.
fn says(digest: &str) -> String {
    format!("{REPLY}\n{{\n{digest}\n}} | reply")
}

/// A fake `pi` that answers the distiller's call with `first` and the repair turn's call (its argv names
/// `@missing.md`) with `repair`, both shell printing the text.
fn says_then_repairs(first: &str, repair: &str) -> String {
    format!("{REPLY}\ncase \"$*\" in\n*@missing.md*) {{\n{repair}\n}} | reply ;;\n*) {{\n{first}\n}} | reply ;;\nesac")
}

/// The repair turn's answer that cites every record of `missing.md`.
const ADDS_MISSING: &str = r###"echo "## Additions"
while IFS= read -r line; do
  case "$line" in "["*) echo "- $line" ;; esac
done < missing.md"###;

/// A real `pi --mode json` run of one turn answering `OK` (pi 1.3.0; the system message is left out, the
/// rest is verbatim): 2 input + 731 cache-write = 733 tokens in, 4 out, cost 0.00467875.
const RECORDED: &str = r##"cat <<'EOF'
{"type":"session","version":3,"id":"01a151e2-939a-7161-b027-0b67b600ab0c","timestamp":"2026-10-19T01:59:25.851Z","cwd":"/tmp/d2probe"}
{"type":"agent_start"}
{"type":"turn_start"}
{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"<file name=\"/tmp/d2probe/p.md\">\nsay ok\n\n</file>\n"}],"timestamp":1792375166181}}
{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"<file name=\"/tmp/d2probe/p.md\">\nsay ok\n\n</file>\n"}],"timestamp":1792375166181}}
{"type":"message_start","message":{"role":"assistant","content":[],"api":"anthropic-messages","provider":"github-copilot","model":"claude-opus-5","usage":{"input":2,"output":1,"cacheRead":0,"cacheWrite":731,"totalTokens":734,"cost":{"input":1e-05,"output":2.5e-05,"cacheRead":0,"cacheWrite":0.00456875,"total":0.00460375},"cacheWrite1h":0},"stopReason":"pending","timestamp":1792375166218,"responseId":"msg_011CgAmYt1THcx6U18TiHg6J"}}
{"type":"message_update","usage":{"input":2,"output":4,"cacheRead":0,"cacheWrite":731,"totalTokens":737,"cost":{"input":1e-05,"output":0.0001,"cacheRead":0,"cacheWrite":0.00456875,"total":0.00467875},"cacheWrite1h":0,"reasoning":0},"assistantMessageEvent":{"type":"text_end","contentIndex":0,"content":"OK"}}
{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"OK"}],"api":"anthropic-messages","provider":"github-copilot","model":"claude-opus-5","usage":{"input":2,"output":4,"cacheRead":0,"cacheWrite":731,"totalTokens":737,"cost":{"input":1e-05,"output":0.0001,"cacheRead":0,"cacheWrite":0.00456875,"total":0.00467875},"cacheWrite1h":0,"reasoning":0},"stopReason":"stop","timestamp":1792375166218,"responseId":"msg_011CgAmYt1THcx6U18TiHg6J","rawStopReason":"end_turn","durationMs":1177,"thinkingLevel":"high"}}
{"type":"turn_end","message":{"role":"assistant","content":[{"type":"text","text":"OK"}],"api":"anthropic-messages","provider":"github-copilot","model":"claude-opus-5","usage":{"input":2,"output":4,"cacheRead":0,"cacheWrite":731,"totalTokens":737,"cost":{"input":1e-05,"output":0.0001,"cacheRead":0,"cacheWrite":0.00456875,"total":0.00467875},"cacheWrite1h":0,"reasoning":0},"stopReason":"stop","timestamp":1792375166218,"responseId":"msg_011CgAmYt1THcx6U18TiHg6J","rawStopReason":"end_turn","durationMs":1177,"thinkingLevel":"high"},"toolResults":[]}
{"type":"agent_settled","aborted":false}
EOF"##;

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
             [watch]\ntick_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_pi(&says(CITES_ALL));
    // The engine's E5 runs `cpd pr poll` from `PATH`, through a `gh` that finds no pull requests.
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
            .filter(|n| !["prompt.md", "records.md", "distill.log", "runs.jsonl", "repair-prompt.md", "digest.md", "missing.md", "problem.md"].contains(&n.as_str()))
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

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_valid_digest_lands_as_upto_seq_and_the_directory_holds_one() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    let d2 = fx.decide("Use flock.");
    let q = fx.cpd(&["context", "ask", "Which port?", "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned();
    let d3 = fx.cpd(&["context", "decide", "Use sqlite for sure.", "--supersedes", &d1, "--context", CONTEXT]).ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
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
    // cpd's index, after the topic sections, lists every record in force, oldest first, and nothing superseded.
    let (_, index) = text.rsplit_once("\n## Record index\n\n").expect("cpd's index");
    let lines: Vec<&str> = index.lines().collect();
    assert_eq!(lines.len(), 3, "{index}");
    assert!(lines[0].starts_with(&format!("- [{d2}] decision (")), "{index}");
    assert!(lines[1].starts_with(&format!("- [{q}] question (")), "{index}");
    assert!(lines[2].starts_with(&format!("- [{d3}] decision (")), "{index}");
    assert_eq!(fx.digest_files(), [format!("{upto}.md")], "older digests and temporary files are gone");
    assert!(fx.errors().is_empty());

    // The distiller's argv and inputs: print mode, no session, no extension, skill, context file or tool; two files.
    let level = Tier::parse("S2").unwrap().level();
    let model = format!("p/sonnet:{level}");
    let calls = fx.env.calls("pi");
    assert_eq!(
        calls,
        [vec![
            "--mode", "json", "--no-session", "--no-extensions", "--no-skills", "--no-context-files", "--no-tools", "--model", &model, "--",
            "@prompt.md", "@records.md",
        ]]
    );
    let records = fs::read_to_string(fx.digests().join("records.md")).unwrap();
    assert!(records.contains("Which port?") && records.contains("Use sqlite for sure.") && !records.contains("Use sqlite.\n"), "{records}");
    // It opens with the checklist the prompt asks the model to copy: each decision and open question in force, in
    // file order, with the first words of its text, then the records.
    let (checklist, blocks) = records.split_once("\n# Records\n\n").expect("a checklist, then the records");
    let lines: Vec<&str> = checklist.lines().collect();
    assert_eq!(lines[0], "# Checklist: 3 decisions and open questions in force", "{checklist}");
    assert!(lines[2].starts_with("Your digest may take at most ") && lines[2].ends_with(" bytes: the program adds its index to it."), "{checklist}");
    assert_eq!(lines[4..], [format!("[{d2}] decision: Use flock."), format!("[{q}] question: Which port?"), format!("[{d3}] decision: Use sqlite for sure.")], "{checklist}");
    assert!(blocks.starts_with(&format!("[{d2}] decision (")), "{blocks}");
    assert!(fs::read_to_string(fx.digests().join("prompt.md")).unwrap().contains("32 KB"));

    let shown = fx.cpd(&["context", "show", "--digest", "--context", CONTEXT]).ok();
    assert_eq!((shown["digest"]["upto_seq"].clone(), shown["markdown"].clone()), (json!(upto), json!(text)));

    // Nothing newer: nothing runs.
    let again = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(fx.pi_calls(), 1);

    // The run's cost is in the output and in the one line of the runs log (tokens in count the cached prompt).
    let run = &out["run"];
    assert_eq!((run["tokens_in"].clone(), run["tokens_out"].clone(), run["cost"].clone(), run["model"].clone()), (json!(733), json!(4), json!(0.25), json!(model)), "{out}");
    assert!(run["seconds"].as_f64().is_some_and(|s| s >= 0.0), "{run}");
    assert_eq!(run.as_object().unwrap().len(), 5, "{run}");
    let runs = fx.runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let line = &runs[0];
    assert_eq!((line["context"].clone(), line["upto_seq"].clone(), line["result"].clone()), (json!(CONTEXT), json!(upto), json!("valid")));
    assert_eq!((line["tokens_in"].clone(), line["tokens_out"].clone(), line["cost"].clone(), line["model"].clone()), (json!(733), json!(4), json!(0.25), json!(model)));
    assert_eq!(line["seconds"], run["seconds"]);
    let started = line["started_at"].as_str().unwrap();
    time::OffsetDateTime::parse(started, &time::format_description::well_known::Rfc3339).expect("started_at is RFC 3339");
    assert!(started.ends_with('Z'), "{started}");
    assert_eq!(line["kind"], "distill");
    assert_eq!(line.as_object().unwrap().len(), 10, "{line}");
}

#[test]
fn a_recorded_pi_transcript_gives_the_digest_and_one_correct_line() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi(RECORDED);
    // "OK" cites no decision, so the digest is invalid; what matters is that it is the final assistant message.
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let upto = fx.highest_seq();
    // The digest misses one decision, so a repair turn ran too (the recording answers it with the same "OK").
    assert_eq!(fs::read_to_string(fx.digests().join(format!("{upto}.md.invalid"))).unwrap(), "OK\n\nOK\n");
    let runs = fx.runs();
    assert_eq!(runs.len(), 2, "{runs:?}");
    for (line, kind) in runs.iter().zip(["distill", "repair"]) {
        assert_eq!((line["tokens_in"].clone(), line["tokens_out"].clone(), line["cost"].clone()), (json!(733), json!(4), json!(0.00467875)), "{line}");
        assert_eq!((line["result"].clone(), line["upto_seq"].clone(), line["kind"].clone()), (json!("invalid"), json!(upto), json!(kind)));
    }
}

#[test]
fn the_distiller_gets_no_cpd_variable() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi(&format!("env > \"$HOME/pi.env\"\n{}", says(CITES_ALL)));
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
fn an_invalid_digest_is_kept_with_one_error_event_and_the_previous_valid_one_stays() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let before = fx.digest_files();
    let kept = fs::read_to_string(fx.digests().join(&before[0])).unwrap();

    let d2 = fx.decide("Use flock.");
    fx.env.fake_pi(&says("echo '# Digest'; echo 'Cites a ghost [hs-d99] and nothing else.'"));
    let run = fx.cpd(&["context", "distill", "--context", CONTEXT]);
    let error = refused(&run, "distill_invalid", 1);
    let details = &error["details"];
    assert_eq!(details["context"], CONTEXT);
    assert_eq!(details["upto_seq"], fx.highest_seq());
    assert_eq!(details["limit"], 32768);
    assert_eq!(details["stray"], json!(["hs-d99"]));
    assert!(details["missing"].as_array().unwrap().iter().any(|id| *id == json!(d2)), "{details}");
    assert!(details["log"].as_str().unwrap().ends_with("distill.log"));
    let upto = fx.highest_seq();
    let invalid = fx.digests().join(format!("{upto}.md.invalid"));
    assert_eq!(details["invalid"], invalid.display().to_string());
    assert_eq!(fs::read_to_string(&invalid).unwrap(), "# Digest\nCites a ghost [hs-d99] and nothing else.\n", "the model's text, without an index");

    let mut expected = before.clone();
    expected.push(format!("{upto}.md.invalid"));
    assert_eq!(fx.digest_files(), expected, "the previous digest stays, the invalid one is kept and no .md.tmp is left");
    assert_eq!(fs::read_to_string(fx.digests().join(&before[0])).unwrap(), kept);
    let events = fx.errors();
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(event["data"]["code"], "distill_invalid");
    assert_eq!(event["data"]["details"]["stray"], json!(["hs-d99"]));
    assert_eq!(event["data"]["details"]["invalid"], invalid.display().to_string());
    assert_eq!((event["context"].clone(), event["plan"].clone(), event["session"].clone()), (Value::Null, Value::Null, Value::Null));

    // A rerun runs again, and a good answer then lands and removes the kept invalid digest.
    fx.env.fake_pi(&says(CITES_ALL));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(fx.digest_files(), [format!("{}.md", fx.highest_seq())]);
    assert_eq!(fx.errors().len(), 1);
    let results: Vec<Value> = fx.runs().iter().map(|r| r["result"].clone()).collect();
    assert_eq!(results, [json!("valid"), json!("invalid"), json!("valid")], "every run that started pi is logged, in order");
}

#[test]
fn the_next_invalid_digest_replaces_the_kept_one() {
    let fx = fx();
    fx.decide("Use sqlite.");
    fx.env.fake_pi(&says("echo 'first failure'"));
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let first = format!("{}.md.invalid", fx.highest_seq());
    assert_eq!(fx.digest_files(), [first.clone()]);

    fx.decide("Use flock.");
    fx.env.fake_pi(&says("echo 'second failure'"));
    refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    let second = format!("{}.md.invalid", fx.highest_seq());
    assert_ne!(first, second);
    assert_eq!(fx.digest_files(), [second.clone()], "one per Context");
    assert_eq!(fs::read_to_string(fx.digests().join(second)).unwrap(), "second failure\n\nsecond failure\n", "the repair turn's text, appended");
}

/// The `kind` and `result` of every logged run, in order.
fn kinds(fx: &Fx) -> Vec<(String, String)> {
    fx.runs().iter().map(|r| (r["kind"].as_str().unwrap().to_owned(), r["result"].as_str().unwrap().to_owned())).collect()
}

#[test]
fn a_digest_that_misses_citations_is_repaired_once_with_an_addition_cpd_appends() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    let d2 = fx.decide("Use flock.");
    let d3 = fx.decide("Use tmux.");
    fx.env.fake_pi(&says_then_repairs(&format!("echo '# Digest'; echo 'Storage [{d1}].'"), ADDS_MISSING));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let upto = fx.highest_seq();
    assert_eq!(out["distilled"], true, "{out}");
    let text = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    let (topic, index) = text.split_once("\n## Record index\n").unwrap();
    assert!(topic.starts_with("# Digest\nStorage ["), "the digest is kept as written: {topic}");
    let (_, additions) = topic.split_once("\n## Additions\n").expect("the addition is appended to the topic sections");
    assert!(additions.contains(&format!("[{d2}]")) && additions.contains(&format!("[{d3}]")) && !additions.contains(&format!("[{d1}]")), "{additions}");
    assert_eq!(index.lines().filter(|l| l.starts_with("- [")).count(), 3);
    assert_eq!(fx.digest_files(), [format!("{upto}.md")], "no .md.invalid is left");
    assert!(fx.errors().is_empty());

    // The repair turn's argv and inputs: the digest as written, the missing records only, their ids.
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1][calls[1].len() - 4..], ["@repair-prompt.md", "@digest.md", "@missing.md", "@problem.md"]);
    assert_eq!(calls[1][..calls[1].len() - 4], calls[0][..calls[0].len() - 2], "the same flags and model");
    let dir = fx.digests();
    let missing = fs::read_to_string(dir.join("missing.md")).unwrap();
    assert!(missing.contains(&format!("[{d2}] decision")) && missing.contains("Use tmux.") && !missing.contains("Use sqlite."), "{missing}");
    let problem = fs::read_to_string(dir.join("problem.md")).unwrap();
    assert!(problem.contains(&format!("- {d2}")) && problem.contains(&format!("- {d3}")) && !problem.contains(&d1), "{problem}");
    assert_eq!(fs::read_to_string(dir.join("digest.md")).unwrap(), format!("# Digest\nStorage [{d1}].\n"));
    assert!(fs::read_to_string(dir.join("repair-prompt.md")).unwrap().contains("## Additions"));

    // Two logged runs, each with its own kind; the repair's figures are in the output too.
    assert_eq!(kinds(&fx), [("distill".to_owned(), "invalid".to_owned()), ("repair".to_owned(), "valid".to_owned())]);
    assert_eq!(out["repair"]["tokens_in"], 733, "{out}");
    assert_eq!(out["run"].as_object().unwrap().len(), 5);
}

#[test]
fn a_repair_that_is_still_invalid_fails_the_run_and_keeps_the_repaired_text() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    let d2 = fx.decide("Use flock.");
    fx.env.fake_pi(&says_then_repairs(&format!("echo '# Digest'; echo 'Storage [{d1}].'"), "echo '## Additions'; echo 'Nothing to add.'"));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(error["details"]["missing"], json!([d2]), "{error}");
    let upto = fx.highest_seq();
    let kept = fs::read_to_string(fx.digests().join(format!("{upto}.md.invalid"))).unwrap();
    assert_eq!(kept, format!("# Digest\nStorage [{d1}].\n\n## Additions\nNothing to add.\n"), "the last text tried");
    assert_eq!(fx.digest_files(), [format!("{upto}.md.invalid")]);
    assert_eq!(fx.pi_calls(), 2, "at most one repair");
    assert_eq!(fx.errors().len(), 1);
    assert_eq!(kinds(&fx), [("distill".to_owned(), "invalid".to_owned()), ("repair".to_owned(), "invalid".to_owned())]);
}

#[test]
fn a_stray_id_or_too_many_missing_get_no_repair() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    fx.decide("Use flock.");
    fx.env.fake_pi(&says_then_repairs(&format!("echo '# Digest'; echo '[{d1}] [hs-d99]'"), ADDS_MISSING));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(error["details"]["stray"], json!(["hs-d99"]));
    assert_eq!(fx.pi_calls(), 1, "a stray id is not repaired");
    assert_eq!(kinds(&fx), [("distill".to_owned(), "invalid".to_owned())]);

    // 18 missing: over the limit of the repair. 16 are repaired.
    let fx = fx_with_decisions(18);
    fx.env.fake_pi(&says_then_repairs("echo '# Digest'; echo 'Nothing cited.'", ADDS_MISSING));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(error["details"]["missing"].as_array().unwrap().len(), 18);
    assert_eq!(fx.pi_calls(), 1, "too many missing are not repaired");

    let fx = fx_with_decisions(16);
    fx.env.fake_pi(&says_then_repairs("echo '# Digest'; echo 'Nothing cited.'", ADDS_MISSING));
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    assert_eq!(fx.pi_calls(), 2);
}

#[test]
fn a_repair_that_fails_as_pi_is_distill_failed_and_leaves_no_digest() {
    let fx = fx();
    let d1 = fx.decide("Use sqlite.");
    fx.decide("Use flock.");
    let first = says(&format!("echo '# Digest'; echo '[{d1}]'"));
    fx.env.fake_pi(&format!("case \"$*\" in *@missing.md*) echo gone >&2; exit 9 ;; esac\n{first}"));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_failed", 1);
    assert_eq!(error["details"]["reason"], "exit", "{error}");
    assert!(fx.digest_files().is_empty(), "{:?}", fx.digest_files());
    assert_eq!(kinds(&fx), [("distill".to_owned(), "invalid".to_owned()), ("repair".to_owned(), "failed".to_owned())]);
}

/// A Context of `n` decisions in force and a fake `pi` that is not set yet.
fn fx_with_decisions(n: u32) -> Fx {
    let fx = fx();
    for i in 1..=n {
        fx.insert(CONTEXT, &format!("hs-d{i}"), &ago(60), &format!("Decision {i}."), Draft::Decision { supersedes: None });
    }
    fx
}

#[test]
fn a_decision_cited_only_in_the_index_is_invalid() {
    let fx = fx();
    let d = fx.decide("Use sqlite.");
    // The model writes its own index, as before D-a, and nothing in a topic section.
    fx.env.fake_pi(&says(&format!("echo '# Digest'; echo 'Nothing cited.'; echo; echo '## Record index'; echo '- [{d}] decision'")));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert_eq!(error["details"]["missing"], json!([d]), "{error}");
    let kept = fs::read_to_string(error["details"]["invalid"].as_str().unwrap()).unwrap();
    assert_eq!(kept, "# Digest\nNothing cited.\n\n\n# Digest\nNothing cited.\n", "the model's index is kept in neither run");
}

#[test]
fn notes_and_answers_need_no_topic_citation_but_cpds_index_lists_them() {
    let fx = fx();
    let d = fx.decide("Use sqlite.");
    let n = fx.cpd(&["context", "note", "A note.", "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned();
    let q = fx.cpd(&["context", "ask", "Which port?", "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned();
    let a = fx.cpd(&["context", "answer", &q, "Port 80.", "--context", CONTEXT]).ok()["record"]["id"].as_str().unwrap().to_owned();
    fx.env.fake_pi(&says(&format!("echo '# Digest'; echo 'Only the decision [{d}].'")));
    let out = fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let text = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    let (topic, index) = text.split_once("\n## Record index\n").unwrap();
    assert!(!topic.contains(&n) && !topic.contains(&a) && !topic.contains(&q), "{text}");
    for (id, kind) in [(&d, "decision"), (&n, "note"), (&q, "question"), (&a, "answer")] {
        assert!(index.contains(&format!("- [{id}] {kind} (")), "{id} {index}");
    }
    assert!(fx.errors().is_empty());
}

#[test]
fn a_digest_over_the_limit_is_invalid() {
    let fx = fx();
    let d = fx.decide("Use sqlite.");
    fx.env.fake_pi(&says(&format!("echo '[{d}]'; i=0; while [ $i -lt 400 ]; do printf '%0100d\\n' 0; i=$((i+1)); done")));
    let error = refused(&fx.cpd(&["context", "distill", "--context", CONTEXT]), "distill_invalid", 1);
    assert!(error["details"]["bytes"].as_u64().unwrap() > 32768, "{error}");
    assert_eq!((error["details"]["missing"].clone(), error["details"]["stray"].clone()), (json!([]), json!([])));
    assert_eq!(fx.digest_files(), [format!("{}.md.invalid", fx.highest_seq())]);
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
    assert!(fx.digest_files().is_empty(), "no .md.tmp is left");
    assert_eq!(fx.errors().len(), 1);
    assert_eq!(fx.runs()[0]["result"], "failed");
}

#[test]
fn an_answer_cut_at_the_output_limit_fails_the_run_but_its_cost_is_logged() {
    let fx = fx();
    fx.decide("Use sqlite.");
    // pi exits 0 in JSON mode when the final message stopped with `length`.
    fx.env.fake_pi(&says("echo 'half'").replace(r#""stopReason":"stop""#, r#""stopReason":"length""#));
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
    assert_eq!(fs::read_to_string(fx.digests().join(&fx.digest_files()[0])).unwrap(), text, "one topic text, one index, not two");
    assert_eq!(fx.pi_calls(), 1, "pi ran once, for the first digest");
    assert_eq!(fx.runs().len(), 1, "the carried-forward run ran no pi and logs nothing");
    assert!(out.get("run").is_none(), "{out}");
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
    // A digest that leaves out a decision or an open question is refused, naming it.
    let required = |id: &&String| matches!(id.as_bytes()[3], b'd' | b'q');
    for id in ids.iter().filter(required) {
        let error = refused(&run(id), "distill_invalid", 1);
        assert_eq!(error["details"]["missing"], json!([id]), "{error}");
        assert!(fx.digest_files().iter().all(|n| n.ends_with(".md.invalid")), "{:?}", fx.digest_files());
    }
    // One that leaves out every note and answer is accepted: cpd's index lists them (D-a).
    let others: Vec<&String> = ids.iter().filter(|id| !required(id)).collect();
    assert!(!others.is_empty(), "the audit lists notes or answers");
    let out = run(&others.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")).ok();
    let text = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    let (topic, index) = text.rsplit_once("\n## Record index\n").unwrap();
    for id in &others {
        assert!(!topic.contains(&format!("[{id}]")) && index.contains(&format!("- [{id}] ")), "{id}");
    }
    fx.insert(CONTEXT, "hs-n99999", &ago(1), "A later note, to run again.", Draft::Note);
    // One that cites them all is accepted, and cpd's index lists them all.
    let out = run("").ok();
    let text = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    for id in &ids {
        assert!(text.contains(&format!("[{id}]")), "{id}");
        assert!(text.contains(&format!("- [{id}] ")), "{id}");
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
    fx.cpd(&["context", "new", "old", "--title", "Old", "--owner", "acme"]).ok();
    fx.insert("old", "hs-d1", &ago(30), "An old decision.", Draft::Decision { supersedes: None });
    fx.insert("old", "hs-n2", &ago(11), "An old note.", Draft::Note);
    fx.cpd(&["context", "new", "fresh", "--title", "Fresh", "--owner", "acme"]).ok();
    fx.insert("fresh", "hs-d3", &ago(30), "A decision.", Draft::Decision { supersedes: None });
    fx.insert("fresh", "hs-n4", &ago(9), "A note.", Draft::Note);
    fx.cpd(&["context", "new", "archived", "--title", "Archived", "--owner", "acme"]).ok();
    fx.insert("archived", "hs-d5", &ago(60), "A decision.", Draft::Decision { supersedes: None });
    fx.cpd(&["context", "archive", "archived"]).ok();
    fx.cpd(&["context", "new", "quiet", "--title", "Quiet", "--owner", "acme"]).ok();
    fx.insert("quiet", "hs-d6", &ago(60), "A decision.", Draft::Decision { supersedes: None });

    // `quiet`'s run is alive (its lock is held): E9 starts none for it.
    let home = fx.home();
    fs::create_dir_all(home.root().join("locks")).unwrap();
    let held = Flock::lock(File::create(home.distill_lock(&ContextId::parse("quiet").unwrap())).unwrap(), FlockArg::LockExclusiveNonblock).unwrap();

    // A slow pi, so a second start would overlap the first.
    fx.env.fake_pi(&format!("sleep 3\n{}", says(CITES_ALL)));
    let engine = start_engine(&fx);
    let digest = |c: &str| home.context_digest_dir(&ContextId::parse(c).unwrap());
    let has_digest = |c: &str| fs::read_dir(digest(c)).is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".md") && e.file_name() != "prompt.md" && e.file_name() != "records.md"));
    wait_until("the digest of `old`", || has_digest("old"));
    // Several more ticks: no second run of `old`, no run of the others.
    std::thread::sleep(Duration::from_secs(4));
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "one run in all: {calls:?}");
    assert!(!has_digest("fresh") && !has_digest("archived") && !has_digest("quiet"));
    let log = fs::read_to_string(home.root().join("run/context-digests/old/distill.log")).unwrap();
    assert!(log.contains("\"distilled\":true"), "the run's stdout is in the log: {log}");

    // The lock is free now: `quiet` is rebuilt, exactly once.
    drop(held);
    wait_until("the digest of `quiet`", || has_digest("quiet"));
    std::thread::sleep(Duration::from_secs(3));
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
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!((fx.pi_calls(), fx.errors().len()), (1, 1), "not repeated at every tick");

    fx.insert(CONTEXT, "hs-d2", &ago(20), "Another.", Draft::Decision { supersedes: None });
    fx.env.fake_pi(&says(CITES_ALL));
    wait_until("the digest", || fx.digest_files() == [format!("{}.md", fx.highest_seq())]);
    stop_engine(engine);
}

/// Writes a decision and `notes` notes into `context`, all stamped now.
fn write_now(fx: &Fx, context: &str, first: u32, notes: u32) {
    let now = ago(0);
    fx.insert(context, &format!("hs-d{first}"), &now, "A decision.", Draft::Decision { supersedes: None });
    for n in 1..=notes {
        fx.insert(context, &format!("hs-n{}", first + n), &now, "A note.", Draft::Note);
    }
}

#[test]
fn twenty_newer_records_start_a_run_at_once_and_never_two_at_once() {
    let fx = fx();
    write_now(&fx, CONTEXT, 100, 19); // 20 records, written seconds ago
    // A slow pi, so a second start would overlap the first.
    fx.env.fake_pi(&format!("sleep 3\n{}", says(CITES_ALL)));
    let engine = start_engine(&fx);
    wait_until("the digest", || fx.digest_files() == [format!("{}.md", fx.highest_seq())]);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(fx.pi_calls(), 1, "one run in all");
    stop_engine(engine);
    assert!(fx.errors().is_empty());
}

#[test]
fn nineteen_newer_records_wait_for_the_quiet_period_and_the_twentieth_starts_the_run() {
    let fx = fx();
    write_now(&fx, CONTEXT, 100, 18); // 19 records, written seconds ago
    let engine = start_engine(&fx);
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!((fx.pi_calls(), fx.digest_files()), (0, Vec::<String>::new()), "19 wait for the quiet period");
    fx.insert(CONTEXT, "hs-n200", &ago(0), "The twentieth.", Draft::Note);
    wait_until("the digest", || fx.digest_files() == [format!("{}.md", fx.highest_seq())]);
    stop_engine(engine);
    assert_eq!(fx.pi_calls(), 1);
}

#[test]
fn the_twenty_count_runs_from_the_latest_digest() {
    let fx = fx();
    write_now(&fx, CONTEXT, 100, 19);
    fx.cpd(&["context", "distill", "--context", CONTEXT]).ok();
    let calls = fx.pi_calls();
    write_now(&fx, CONTEXT, 300, 18); // 19 records newer than the digest
    let engine = start_engine(&fx);
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(fx.pi_calls(), calls, "19 newer than the digest wait");
    fx.insert(CONTEXT, "hs-n400", &ago(0), "The twentieth.", Draft::Note);
    wait_until("the second digest", || fx.pi_calls() == calls + 1);
    stop_engine(engine);
}
