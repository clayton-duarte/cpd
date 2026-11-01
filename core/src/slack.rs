//! The Slack helpers' seam (commands.md §21.8): [`call`] builds the process that runs a helper of the pi adapter
//! (`lib/<script>`) on pi's own MCP client, [`answer`] reads the one JSON line it prints. Shared by `pr post` and `pr react`.

use std::process::Output;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::catalog::node;
use crate::config::{Config, locate_pi};
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::Call;

/// The lines of a child's output kept in an error.
const TAIL_LINES: usize = 20;

/// The `Call` of helper `script` with `--name value` arguments `args`, `stdin` and `deadline`.
pub fn call(
    config: &Config,
    script: &str,
    args: &[(&str, &str)],
    stdin: Option<Vec<u8>>,
    deadline: Duration,
) -> Result<Call, CpdError> {
    let pi = locate_pi()?;
    let adapter = config.pi_adapter_dir()?;
    let adapter = std::fs::canonicalize(&adapter).map_err(|e| {
        CpdError::new(
            ErrorCode::AdapterMissing,
            format!("Cannot resolve the pi adapter {}: {e}.", adapter.display()),
        )
        .with("path", adapter.display().to_string())
    })?;
    build(
        &node(),
        &adapter,
        &pi.coding_agent_entry,
        &pi.agent_dir,
        config_home()?,
        script,
        args,
        stdin,
        deadline,
    )
}

fn config_home() -> Result<std::path::PathBuf, CpdError> {
    let root = crate::home::CpdHome::from_env()?.root().to_path_buf();
    std::fs::canonicalize(&root).map_err(|e| {
        CpdError::new(
            ErrorCode::ConfigInvalid,
            format!("Cannot resolve CPD_HOME {}: {e}.", root.display()),
        )
        .with("path", root.display().to_string())
    })
}

#[allow(clippy::too_many_arguments)]
fn build(
    node: &std::path::Path,
    adapter: &std::path::Path,
    entry: &std::path::Path,
    agent_dir: &std::path::Path,
    home: std::path::PathBuf,
    script: &str,
    args: &[(&str, &str)],
    stdin: Option<Vec<u8>>,
    deadline: Duration,
) -> Result<Call, CpdError> {
    let mut argv = vec![
        adapter.join("lib").join(script).display().to_string(),
        "--coding-agent".to_owned(),
        entry.display().to_string(),
    ];
    for (name, value) in args {
        argv.push(format!("--{name}"));
        argv.push((*value).to_owned());
    }
    Ok(Call {
        tool: "node".to_owned(),
        program: node.display().to_string(),
        args: argv,
        envs: vec![
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                Some(agent_dir.display().to_string()),
            ),
            ("CPD_HOME".to_owned(), Some(home.display().to_string())),
            ("CPD_PLAN".to_owned(), None),
            ("CPD_SESSION".to_owned(), None),
            ("CPD_ATTEMPT".to_owned(), None),
        ],
        cwd: Some(home),
        deadline,
        stdin,
        stderr_to: None,
    })
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

/// The helper's `ok:true` value, or its `ok:false` as a `CpdError` with the helper's code (an unknown code is
/// `slack_helper_failed` with the code in `details.code`); no JSON line is `spawn_failed`.
pub fn answer(out: &Output, argv: &[String]) -> Result<Value, CpdError> {
    let known = [
        ErrorCode::SlackAuth,
        ErrorCode::SlackNotConfigured,
        ErrorCode::SlackFailed,
        ErrorCode::SlackRefused,
        ErrorCode::SlackUnparseable,
        ErrorCode::SlackHelperFailed,
    ];
    answer_with(out, argv, &known, ErrorCode::SlackHelperFailed)
}

/// [`answer`] for any helper: `known` are its codes, `fallback` the code of an unknown one.
pub(crate) fn answer_with(
    out: &Output,
    argv: &[String],
    known: &[ErrorCode],
    fallback: ErrorCode,
) -> Result<Value, CpdError> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let printed: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    match printed {
        Some(value) if out.status.success() && value["ok"] == true => Ok(value),
        Some(value) if value["ok"] == false && value["error"]["code"].is_string() => {
            let error = &value["error"];
            let code = error["code"].as_str().unwrap_or_default();
            let known = known.iter().copied().find(|c| c.as_str() == code);
            let message = error["message"].as_str().unwrap_or_default();
            let mut err = CpdError::new(known.unwrap_or(fallback), message);
            match (known, error["details"].as_object()) {
                (Some(_), Some(details)) => {
                    for (k, v) in details {
                        err = err.with(k, v.clone());
                    }
                }
                (None, details) => {
                    for (k, v) in details.into_iter().flatten() {
                        err = err.with(k, v.clone());
                    }
                    err = err.with("code", code);
                }
                (Some(_), None) => {}
            }
            Err(err)
        }
        _ => {
            let message = if out.status.success() && stdout.trim().is_empty() {
                format!("{} exited 0 but printed no result.", argv.join(" "))
            } else {
                format!("{} printed no result.", argv.join(" "))
            };
            Err(CpdError::new(ErrorCode::SpawnFailed, message)
                .with("argv", json!(argv))
                .with("exit", out.status.code())
                .with("stdout", tail(&stdout))
                .with("stderr", tail(&String::from_utf8_lossy(&out.stderr))))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
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

    #[test]
    fn the_call_is_the_helper_on_pis_entry() {
        let call = build(
            Path::new("/n/node"),
            Path::new("/adapter"),
            Path::new("/pi/dist/index.js"),
            Path::new("/pi"),
            PathBuf::from("/home"),
            "slack-post.ts",
            &[("channel", "C0POLICY1")],
            Some(b"hi".to_vec()),
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(call.tool, "node");
        assert_eq!(call.program, "/n/node");
        assert_eq!(
            call.args,
            [
                "/adapter/lib/slack-post.ts",
                "--coding-agent",
                "/pi/dist/index.js",
                "--channel",
                "C0POLICY1"
            ]
        );
        assert_eq!(
            call.envs,
            [
                ("PI_CODING_AGENT_DIR".to_owned(), Some("/pi".to_owned())),
                ("CPD_HOME".to_owned(), Some("/home".to_owned())),
                ("CPD_PLAN".to_owned(), None),
                ("CPD_SESSION".to_owned(), None),
                ("CPD_ATTEMPT".to_owned(), None),
            ]
        );
        assert_eq!(call.cwd, Some(PathBuf::from("/home")));
        assert_eq!(call.stdin, Some(b"hi".to_vec()));
        assert_eq!(call.deadline, Duration::from_secs(60));
    }

    #[test]
    fn an_ok_line_is_its_value() {
        let line = r#"{"ok":true,"channel":"C1","ts":"1.2","link":"l"}"#;
        assert_eq!(
            answer(&output(0, &format!("{line}\n")), &[]).unwrap()["ts"],
            "1.2"
        );
    }

    #[test]
    fn each_known_code_keeps_its_message_and_details() {
        for code in [
            ErrorCode::SlackAuth,
            ErrorCode::SlackNotConfigured,
            ErrorCode::SlackFailed,
            ErrorCode::SlackRefused,
            ErrorCode::SlackUnparseable,
            ErrorCode::SlackHelperFailed,
        ] {
            let line = json!({"ok": false, "error": {"code": code.as_str(), "message": "m", "details": {"k": 1}}}).to_string();
            let error = failed(&line);
            assert_eq!((error.code(), error.message()), (code, "m"));
            assert_eq!(error.details()["k"], 1);
        }
    }

    #[test]
    fn an_unknown_code_is_slack_helper_failed_with_the_code() {
        let error =
            failed(r#"{"ok":false,"error":{"code":"weird","message":"m","details":{"k":1}}}"#);
        assert_eq!(error.code(), ErrorCode::SlackHelperFailed);
        assert_eq!(
            (&error.details()["code"], &error.details()["k"]),
            (&json!("weird"), &json!(1))
        );
    }

    #[test]
    fn no_output_is_spawn_failed() {
        let error = answer(&output(0, ""), &["node".to_owned(), "x".to_owned()]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::SpawnFailed);
        assert!(error.message().contains("exited 0 but printed no result"));
    }

    #[test]
    fn text_that_is_not_json_is_spawn_failed() {
        let error = answer(&output(1, "boom\n"), &["node".to_owned()]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::SpawnFailed);
        assert_eq!(error.details()["stdout"], "boom");
    }
}
