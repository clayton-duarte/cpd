//! The Jira helper's seam (docs/architecture/swr-jira.md §3.4): [`helper`] builds the process that runs
//! `pi/lib/jira-transition.ts` on pi's own MCP client, [`stdin`] is its request, [`answer`] reads the one JSON line it
//! prints. Used by the `jira` source of E24.

use std::process::Output;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::Call;
use crate::policy::{McpServer, load_machine};

/// The helper's deadline: above its 30 s per Jira call.
pub const DEADLINE: Duration = Duration::from_secs(120);

/// One transition to ask of the helper (swr-jira.md §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: String,
    pub target: String,
    /// The cached site id, or `None` to have the helper resolve it.
    pub cloud_id: Option<String>,
    /// The cached authenticated user's account id, or `None` to have the helper resolve it.
    pub account_id: Option<String>,
    pub target_name: String,
    pub later_names: Vec<String>,
    pub estimate_fields: Vec<String>,
}

/// What an `ok` helper did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Moved,
    Unchanged,
}

/// An `ok` answer (swr-jira.md §3.3): the facts of the one read, after the move when there was one.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub done: Done,
    pub cloud_id: String,
    pub account_id: String,
    pub summary: String,
    pub status_name: String,
    pub status_category: String,
    pub estimates: Value,
}

const HELPER_CODES: [ErrorCode; 5] = [
    ErrorCode::JiraAuth,
    ErrorCode::JiraNotConfigured,
    ErrorCode::JiraFailed,
    ErrorCode::JiraRefused,
    ErrorCode::JiraHelperFailed,
];

/// The machine policy's server `atlassian` (core/src/jira.rs:55-82 of the old seam).
pub fn server(home: &CpdHome) -> Result<McpServer, CpdError> {
    let machine = load_machine(home)?;
    machine.servers.get("atlassian").cloned().ok_or_else(|| {
        CpdError::new(
            ErrorCode::JiraNotConfigured,
            "No Atlassian MCP server is configured: add [mcp.servers.atlassian] to policy/host.toml.",
        )
    })
}

/// The `Call` of the helper, with no stdin yet.
pub fn helper(config: &Config) -> Result<Call, CpdError> {
    crate::slack::call(config, "jira-transition.ts", &[], None, DEADLINE)
}

/// The helper's stdin: the request as one JSON object.
pub fn stdin(server: &McpServer, request: &Request) -> Vec<u8> {
    let value = json!({
        "server": server,
        "cloud_id": request.cloud_id,
        "account_id": request.account_id,
        "ticket": request.ticket,
        "target": request.target,
        "target_name": request.target_name,
        "later_names": request.later_names,
        "estimate_fields": request.estimate_fields,
    });
    serde_json::to_vec(&value).expect("a JSON value serializes")
}

fn text(value: &Value, key: &str) -> Result<String, CpdError> {
    value[key].as_str().map(str::to_owned).ok_or_else(|| {
        CpdError::new(
            ErrorCode::JiraHelperFailed,
            format!("The Jira helper's answer has no `{key}`."),
        )
        .with("code", "shape")
    })
}

/// The helper's `ok:true` as an [`Answer`], or its `ok:false` as a `CpdError` (an unknown code is `jira_helper_failed`
/// with `details.code`); no JSON line is `spawn_failed`.
pub fn answer(out: &Output, argv: &[String]) -> Result<Answer, CpdError> {
    let value = crate::slack::answer_with(out, argv, &HELPER_CODES, ErrorCode::JiraHelperFailed)?;
    let done = match value["outcome"].as_str() {
        Some("moved") => Done::Moved,
        Some("unchanged") => Done::Unchanged,
        other => {
            return Err(CpdError::new(
                ErrorCode::JiraHelperFailed,
                format!("The Jira helper answered an unknown outcome {other:?}."),
            )
            .with("code", "outcome"));
        }
    };
    let (status_name, status_category) = match done {
        Done::Moved => (
            text(&value, "to")?,
            if value["target"] == "done" {
                "done"
            } else {
                "indeterminate"
            }
            .to_owned(),
        ),
        Done::Unchanged => (
            text(&value, "status_name")?,
            text(&value, "status_category")?,
        ),
    };
    Ok(Answer {
        done,
        cloud_id: text(&value, "cloud_id")?,
        account_id: text(&value, "account_id")?,
        summary: text(&value, "summary")?,
        status_name,
        status_category,
        estimates: value.get("estimates").cloned().unwrap_or_else(|| json!({})),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use super::*;

    fn output(code: i32, stdout: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn failed(line: &str) -> CpdError {
        answer(&output(3, line), &["node".to_owned()]).unwrap_err()
    }

    fn request() -> Request {
        Request {
            ticket: "A-1".into(),
            target: "in_review".into(),
            cloud_id: None,
            account_id: None,
            target_name: "In Review".into(),
            later_names: vec!["Done".into()],
            estimate_fields: vec!["customfield_10016".into()],
        }
    }

    #[test]
    fn the_stdin_is_the_request_with_the_server() {
        let server = McpServer {
            url: "https://x/mcp".into(),
            description: None,
            exposure: None,
            tool_exposure: None,
            oauth: None,
            headers: None,
        };
        let value: serde_json::Value = serde_json::from_slice(&stdin(&server, &request())).unwrap();
        assert_eq!(
            value,
            json!({"server": {"url": "https://x/mcp"}, "ticket": "A-1", "target": "in_review",
                   "cloud_id": null, "account_id": null, "target_name": "In Review", "later_names": ["Done"], "estimate_fields": ["customfield_10016"]})
        );
    }

    #[test]
    fn a_missing_atlassian_server_is_jira_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let error = server(&home).unwrap_err();
        assert_eq!(error.code(), ErrorCode::JiraNotConfigured);
        assert!(error.message().contains("[mcp.servers.atlassian]"));
    }

    #[test]
    fn each_helper_code_keeps_its_message_and_details() {
        for code in HELPER_CODES {
            let line = json!({"ok": false, "error": {"code": code.as_str(), "message": "m", "details": {"k": 1}}}).to_string();
            let error = failed(&line);
            assert_eq!((error.code(), error.message()), (code, "m"));
            assert_eq!(error.details()["k"], 1);
        }
    }

    #[test]
    fn an_unknown_code_is_jira_helper_failed_with_the_code() {
        let error =
            failed(r#"{"ok":false,"error":{"code":"weird","message":"m","details":{"k":1}}}"#);
        assert_eq!(error.code(), ErrorCode::JiraHelperFailed);
        assert_eq!(
            (&error.details()["code"], &error.details()["k"]),
            (&json!("weird"), &json!(1))
        );
    }

    #[test]
    fn ok_lines_are_answers() {
        let moved = r#"{"ok":true,"ticket":"A-1","target":"done","outcome":"moved","cloud_id":"c","account_id":"u","from":"a","to":"Done","summary":"s","estimates":{}}"#;
        let same = r#"{"ok":true,"ticket":"A-1","target":"done","outcome":"unchanged","cloud_id":"c","account_id":"u","status_name":"b","status_category":"indeterminate","why":"at_target","summary":"s","estimates":{"customfield_1":null}}"#;
        let moved = answer(&output(0, moved), &[]).unwrap();
        assert_eq!(
            (
                moved.done,
                moved.status_name.as_str(),
                moved.status_category.as_str()
            ),
            (Done::Moved, "Done", "done")
        );
        let same = answer(&output(0, same), &[]).unwrap();
        assert_eq!(
            (same.done, same.status_name.as_str(), same.cloud_id.as_str()),
            (Done::Unchanged, "b", "c")
        );
        assert_eq!(same.estimates, json!({"customfield_1": null}));
    }

    #[test]
    fn no_output_is_spawn_failed() {
        let error = answer(&output(0, ""), &["node".to_owned()]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::SpawnFailed);
    }
}
