//! `secret::op::get_with` and `list_with` on a [`FakeRunner`]: the 1Password CLI call and how its answer is mapped.
//! `create` and `edit` feed stdin, which a fake never reads: they are tested at the layer above.

use std::io::ErrorKind;
use std::time::Duration;

use serde_json::json;

use super::op::{get_with, list_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::host::runner::{FakeRunner, Reply};

const PROGRAM: &str = "op";

fn fake(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on(PROGRAM, |_| true, reply)
}

#[test]
fn get_with_asks_the_runner_for_item_get_with_the_account() {
    let runner = fake(Reply::ok("s3cret\n"));
    get_with(&runner, "Vault", "Item", "token").unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool, PROGRAM);
    assert_eq!(calls[0].program, PROGRAM);
    assert_eq!(
        calls[0].args,
        [
            "item",
            "get",
            "Item",
            "--vault",
            "Vault",
            "--fields",
            "label=token",
            "--reveal",
            "--account",
            "my.1password.com"
        ]
    );
}

#[test]
fn get_with_gives_the_call_the_deadline_of_two_minutes() {
    let runner = fake(Reply::ok("v"));
    get_with(&runner, "V", "I", "f").unwrap();
    assert_eq!(runner.calls()[0].deadline, Duration::from_secs(120));
}

#[test]
fn get_with_returns_the_value_without_its_trailing_newline() {
    let runner = fake(Reply::ok("s3cret\n"));
    let fields = get_with(&runner, "V", "I", "f").unwrap();
    assert_eq!(fields["value"], json!("s3cret"));
}

#[test]
fn list_with_asks_for_the_vaults_items_as_json_and_returns_id_and_title() {
    let runner = fake(Reply::ok(
        r#"[{"id":"a1","title":"One","extra":1},{"id":"b2","title":"Two"}]"#,
    ));
    let fields = list_with(&runner, "Vault").unwrap();
    assert_eq!(
        runner.calls()[0].args,
        [
            "item",
            "list",
            "--vault",
            "Vault",
            "--format",
            "json",
            "--account",
            "my.1password.com"
        ]
    );
    assert_eq!(
        fields["items"],
        json!([{"id": "a1", "title": "One"}, {"id": "b2", "title": "Two"}])
    );
}

#[test]
fn list_with_maps_output_that_is_not_json_to_secret_op_failed() {
    let runner = fake(Reply::ok("nope"));
    let err = list_with(&runner, "V").unwrap_err();
    assert_eq!(err.code(), ErrorCode::SecretOpFailed);
    assert_eq!(
        err.message(),
        "op returned a list that is not the expected JSON"
    );
}

#[test]
fn get_with_maps_a_cli_that_is_not_installed_to_secret_op_missing() {
    let runner = fake(Reply::io(ErrorKind::NotFound));
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert_eq!(err.code(), ErrorCode::SecretOpMissing);
}

#[test]
fn get_with_maps_any_other_io_error_to_secret_op_failed() {
    let runner = fake(Reply::io(ErrorKind::PermissionDenied));
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert_eq!(err.code(), ErrorCode::SecretOpFailed);
    assert_eq!(err.message(), "op could not be run: permission denied");
}

#[test]
fn get_with_maps_a_signed_out_cli_to_secret_not_signed_in() {
    let runner = fake(Reply::exit(1, "", "[ERROR] You are not signed in."));
    assert_eq!(
        get_with(&runner, "V", "I", "f").unwrap_err().code(),
        ErrorCode::SecretNotSignedIn
    );
}

#[test]
fn get_with_maps_a_missing_item_to_secret_not_found() {
    let runner = fake(Reply::exit(
        1,
        "",
        "[ERROR] \"I\" isn't an item in the \"V\" vault.",
    ));
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert_eq!(err.code(), ErrorCode::SecretNotFound);
    assert!(
        err.details()["stderr"]
            .as_str()
            .unwrap()
            .contains("isn't an item")
    );
}

#[test]
fn get_with_maps_any_other_failure_to_secret_op_failed_with_the_exit_and_stderr() {
    let runner = fake(Reply::exit(2, "", "boom"));
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert_eq!(err.code(), ErrorCode::SecretOpFailed);
    assert_eq!(err.details()["exit"], 2);
    assert_eq!(err.details()["stderr"], "boom");
}

#[test]
fn get_with_maps_a_scripted_timeout_to_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        PROGRAM,
        |_| true,
        Reply::ok("v").takes(Duration::from_secs(300)),
    );
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.details()["tool"], PROGRAM);
    assert_eq!(err.details()["deadline_seconds"], 120);
    assert_eq!(clock.elapsed(), Duration::from_secs(120));
}

#[test]
fn a_value_never_appears_in_the_error_of_a_failed_call() {
    let runner = fake(Reply::exit(1, "s3cret", "boom"));
    let err = get_with(&runner, "V", "I", "f").unwrap_err();
    assert!(!err.to_json().to_string().contains("s3cret"));
}
