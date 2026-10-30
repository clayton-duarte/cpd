//! The Jira helper's seam (commands.md §21.11): [`call`] builds the process that runs `pi/lib/jira-transition.ts` on pi's own
//! MCP client, [`answer`] reads the one JSON line it prints. Used by engine step E19.

use std::process::Output;
use std::time::Duration;

use serde_json::json;

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::Call;
use crate::policy::{McpServer, load_machine};
use crate::pr::jira_sync::Target;

/// The three status names of the policy (`pr.jira.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Names {
    pub in_progress: String,
    pub in_review: String,
    pub done: String,
}

/// One transition to ask of the helper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub ticket: String,
    pub target: Target,
    pub names: Names,
    pub estimate_fields: Vec<String>,
}

/// What an `ok` helper did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Moved,
    Unchanged,
}

const HELPER_CODES: [ErrorCode; 5] =
    [ErrorCode::JiraAuth, ErrorCode::JiraNotConfigured, ErrorCode::JiraFailed, ErrorCode::JiraRefused, ErrorCode::JiraHelperFailed];

/// The `Call` of the helper for `request`, with the machine policy's server `atlassian`.
pub fn call(config: &Config, home: &CpdHome, request: &Request, deadline: Duration) -> Result<Call, CpdError> {
    let machine = load_machine(home)?;
    let Some(server) = machine.servers.get("atlassian") else {
        return Err(CpdError::new(
            ErrorCode::JiraNotConfigured,
            "No Atlassian MCP server is configured: add [mcp.servers.atlassian] to policy/host.toml.",
        ));
    };
    crate::slack::call(config, "jira-transition.ts", &[], Some(stdin(server, request)), deadline)
}

/// The helper's stdin: the request as one JSON object.
fn stdin(server: &McpServer, request: &Request) -> Vec<u8> {
    let names = &request.names;
    let value = json!({
        "server": server,
        "ticket": request.ticket,
        "target": request.target.as_str(),
        "names": {"in_progress": names.in_progress, "in_review": names.in_review, "done": names.done},
        "estimate_fields": request.estimate_fields,
    });
    serde_json::to_vec(&value).expect("a JSON value serializes")
}

/// The helper's `ok:true` as [`Done`], or its `ok:false` as a `CpdError` (an unknown code is `jira_helper_failed` with
/// `details.code`); no JSON line is `spawn_failed`.
pub fn answer(out: &Output, argv: &[String]) -> Result<Done, CpdError> {
    let value = crate::slack::answer_with(out, argv, &HELPER_CODES, ErrorCode::JiraHelperFailed)?;
    match value["outcome"].as_str() {
        Some("moved") => Ok(Done::Moved),
        Some("unchanged") => Ok(Done::Unchanged),
        other => Err(CpdError::new(ErrorCode::JiraHelperFailed, format!("The Jira helper answered an unknown outcome {other:?}."))
            .with("code", "outcome")),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use super::*;

    fn output(code: i32, stdout: &str) -> Output {
        Output { status: ExitStatus::from_raw(code << 8), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() }
    }

    fn failed(line: &str) -> CpdError {
        answer(&output(3, line), &["node".to_owned()]).unwrap_err()
    }

    fn request() -> Request {
        Request { ticket: "A-1".into(), target: Target::InReview, names: Names { in_progress: "".into(), in_review: "In Review".into(), done: "Done".into() }, estimate_fields: vec!["customfield_10016".into()] }
    }

    #[test]
    fn the_stdin_is_the_request_with_the_server() {
        let server = McpServer { url: "https://x/mcp".into(), description: None, exposure: None, tool_exposure: None, oauth: None, headers: None };
        let value: serde_json::Value = serde_json::from_slice(&stdin(&server, &request())).unwrap();
        assert_eq!(
            value,
            json!({"server": {"url": "https://x/mcp"}, "ticket": "A-1", "target": "in_review",
                   "names": {"in_progress": "", "in_review": "In Review", "done": "Done"}, "estimate_fields": ["customfield_10016"]})
        );
    }

    #[test]
    fn a_missing_atlassian_server_is_jira_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let config = Config::parse("version = 1\nmachine = \"test\"\n", std::path::Path::new("config.toml")).unwrap().0;
        let error = call(&config, &home, &request(), Duration::from_secs(1)).unwrap_err();
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
        let error = failed(r#"{"ok":false,"error":{"code":"weird","message":"m","details":{"k":1}}}"#);
        assert_eq!(error.code(), ErrorCode::JiraHelperFailed);
        assert_eq!((&error.details()["code"], &error.details()["k"]), (&json!("weird"), &json!(1)));
    }

    #[test]
    fn ok_lines_are_done() {
        let moved = r#"{"ok":true,"ticket":"A-1","target":"done","outcome":"moved","from":"a","to":"b"}"#;
        let same = r#"{"ok":true,"ticket":"A-1","target":"done","outcome":"unchanged","status":"b","why":"at_target"}"#;
        assert_eq!(answer(&output(0, moved), &[]).unwrap(), Done::Moved);
        assert_eq!(answer(&output(0, same), &[]).unwrap(), Done::Unchanged);
    }

    #[test]
    fn no_output_is_spawn_failed() {
        let error = answer(&output(0, ""), &["node".to_owned()]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::SpawnFailed);
    }
}
