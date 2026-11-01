//! `host::exe::asks_to_open_store_with`, `plan::lock::ensure_watch_with` and `inbox::cmd::local_offset_with`
//! on a `FakeRunner` and a `ManualClock`: no process.

use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use serde_json::json;
use time::UtcOffset;

use super::exe::asks_to_open_store_with;
use super::runner::{FakeRunner, Reply};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::ids::PlanId;
use crate::inbox::cmd::local_offset_with;
use crate::plan::lock::ensure_watch_with;

const CPD: &str = "/opt/bin/cpd";
const TEN: Duration = Duration::from_secs(10);

fn fake(reply: Reply, program: &str) -> (FakeRunner, ManualClock) {
    let clock = ManualClock::new();
    (
        FakeRunner::new(clock.clone()).on(program, |_| true, reply),
        clock,
    )
}

fn ask(reply: Reply) -> (Result<(), String>, FakeRunner, ManualClock) {
    let (runner, clock) = fake(reply, "cpd");
    let verdict = asks_to_open_store_with(&runner, Path::new(CPD), TEN);
    (verdict, runner, clock)
}

const OPENS: &str = r#"{"ok":true,"version":"x","store":{"opens":true,"refusal":null,"unknown":["29991231T2359-later"],"pending":[]}}"#;
const REFUSES: &str = r#"{"ok":true,"version":"x","store":{"opens":false,"refusal":{"code":"store_too_new","message":"too new"},"unknown":[],"pending":[]}}"#;

#[test]
fn asking_calls_the_executable_with_version_under_the_timeout() {
    let (verdict, runner, _) = ask(Reply::ok(OPENS));
    assert_eq!(verdict, Ok(()));
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (
            calls[0].program.as_str(),
            calls[0].args.clone(),
            calls[0].deadline
        ),
        (CPD, vec!["--version".to_owned()], TEN)
    );
    assert_eq!(calls[0].stdin, None);
}

#[test]
fn a_missing_database_or_a_store_that_opens_can_be_opened() {
    assert_eq!(
        ask(Reply::ok(r#"{"ok":true,"version":"x","store":null}"#)).0,
        Ok(())
    );
    assert_eq!(ask(Reply::ok(OPENS)).0, Ok(()));
}

#[test]
fn a_store_that_does_not_open_is_refused_with_the_verdicts_message() {
    assert_eq!(
        ask(Reply::ok(REFUSES)).0,
        Err("cannot open the store: too new".to_owned())
    );
}

#[test]
fn malformed_output_is_a_refusal() {
    for stdout in [
        "not json",
        "",
        r#"{"ok":true,"version":"x"}"#,
        r#"{"store":{}}"#,
        r#"{"store":{"opens":"true"}}"#,
        r#"{"store":{"opens":false}}"#,
        r#"{"store":true}"#,
    ] {
        assert!(ask(Reply::ok(stdout)).0.is_err(), "{stdout}");
    }
}

#[test]
fn a_failing_executable_is_a_refusal() {
    let reason = ask(Reply::exit(3, OPENS, "")).0.unwrap_err();
    assert!(reason.contains("--version failed"), "{reason}");
}

#[test]
fn an_executable_that_cannot_run_is_a_refusal() {
    let reason = ask(Reply::io(ErrorKind::NotFound)).0.unwrap_err();
    assert!(reason.contains("cannot run --version"), "{reason}");
}

#[test]
fn an_executable_that_does_not_answer_in_ten_seconds_is_refused() {
    let (verdict, _, clock) = ask(Reply::ok(OPENS).takes(TEN));
    let reason = verdict.unwrap_err();
    assert!(reason.contains("did not answer"), "{reason}");
    assert_eq!(clock.elapsed(), TEN);
}

fn plan() -> PlanId {
    PlanId::parse("my-plan").unwrap()
}

fn watch(
    reply: Reply,
) -> (
    Result<serde_json::Value, crate::error::CpdError>,
    FakeRunner,
) {
    let (runner, _) = fake(reply, "cpd");
    let result = ensure_watch_with(&runner, Path::new("/cpd-home"), &plan());
    (result, runner)
}

#[test]
fn ensure_watch_runs_cpd_watch_ensure_in_the_home_with_cpd_home_set() {
    let (result, runner) = watch(Reply::ok(r#"{"ok":true,"started":true}"#));
    assert_eq!(result.unwrap(), json!({"started": true}));
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "cpd");
    assert_eq!(calls[0].args, ["watch", "--ensure"]);
    assert_eq!(calls[0].cwd.as_deref(), Some(Path::new("/cpd-home")));
    assert!(
        calls[0]
            .envs
            .contains(&("CPD_HOME".to_owned(), Some("/cpd-home".to_owned())))
    );
    assert_eq!(calls[0].stdin, None);
}

#[test]
fn ensure_watch_that_cannot_start_cpd_is_watch_start_failed() {
    let (result, _) = watch(Reply::io(ErrorKind::NotFound));
    let e = result.unwrap_err();
    assert_eq!(e.code(), ErrorCode::WatchStartFailed);
    assert!(e.message().contains("cannot run cpd"), "{}", e.message());
    assert_eq!(e.details()["plan"], "my-plan");
    assert_eq!(e.details()["argv"], json!(["cpd", "watch", "--ensure"]));
}

#[test]
fn ensure_watch_that_fails_carries_the_ensures_error() {
    let (result, _) = watch(Reply::exit(
        1,
        r#"{"ok":false,"error":{"message":"no engine"}}"#,
        "",
    ));
    let e = result.unwrap_err();
    assert_eq!(e.code(), ErrorCode::WatchStartFailed);
    assert!(e.message().contains("no engine"), "{}", e.message());
    assert_eq!(e.details()["error"], json!({"message": "no engine"}));
}

#[test]
fn ensure_watch_that_prints_no_success_value_is_watch_start_failed() {
    let (result, _) = watch(Reply::ok("not json"));
    let e = result.unwrap_err();
    assert_eq!(e.code(), ErrorCode::WatchStartFailed);
    assert!(
        e.message().contains("printed no success value"),
        "{}",
        e.message()
    );
}

fn offset(reply: Reply) -> (Result<UtcOffset, crate::error::CpdError>, FakeRunner) {
    let (runner, _) = fake(reply, "date");
    (local_offset_with(&runner), runner)
}

#[test]
fn local_offset_calls_date_with_plus_percent_z() {
    let (_, runner) = offset(Reply::ok("+0200\n"));
    let calls = runner.calls();
    assert_eq!(
        (calls[0].program.as_str(), calls[0].args.clone()),
        ("date", vec!["+%z".to_owned()])
    );
    assert_eq!(calls[0].stdin, None);
}

#[test]
fn local_offset_reads_a_positive_offset() {
    assert_eq!(
        offset(Reply::ok("+0200\n")).0.unwrap(),
        UtcOffset::from_hms(2, 0, 0).unwrap()
    );
}

#[test]
fn local_offset_reads_a_negative_offset_with_minutes() {
    assert_eq!(
        offset(Reply::ok("-0530\n")).0.unwrap(),
        UtcOffset::from_hms(-5, -30, 0).unwrap()
    );
}

#[test]
fn a_failing_date_is_spawn_failed_with_its_stderr() {
    let e = offset(Reply::exit(1, "", "date: bad\n")).0.unwrap_err();
    assert_eq!(e.code(), ErrorCode::SpawnFailed);
    assert!(
        e.message().contains("`date +%z` failed: date: bad"),
        "{}",
        e.message()
    );
    assert_eq!(e.details()["stderr"], "date: bad");
}

#[test]
fn a_date_that_cannot_run_is_spawn_failed() {
    let e = offset(Reply::io(ErrorKind::NotFound)).0.unwrap_err();
    assert_eq!(e.code(), ErrorCode::SpawnFailed);
    assert!(
        e.message().contains("Cannot run `date +%z`"),
        "{}",
        e.message()
    );
}

#[test]
fn a_date_that_prints_no_offset_is_spawn_failed_with_its_stdout() {
    let e = offset(Reply::ok("CEST\n")).0.unwrap_err();
    assert_eq!(e.code(), ErrorCode::SpawnFailed);
    assert_eq!(e.details()["stdout"], "CEST");
}
