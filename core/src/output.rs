//! What a command prints (contract §1.2, commands.md §1.2).
//!
//! A command returns `Result<Output, CpdError>`; [`emit`] prints it as exactly
//! one JSON value on stdout and gives the exit code. Progress and debug text
//! go to stderr only.

use std::io::Write;
use std::process::ExitCode;

use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value};

use crate::error::{CpdError, Exit};

/// A command's successful result.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// Printed as `{"ok": true, …fields}`: the command's fields at the top level.
    Success(Map<String, Value>),
    /// The command already wrote its own stdout, or owns its terminal, and
    /// nothing more is printed; exit 0. Only for the exceptions of
    /// commands.md §1.2: `events follow` (one Event per line), `session run`,
    /// `tui view` and `tui --attach` (amendment 10, commands.md §19).
    /// Their failures are still returned as `Err` and printed as the envelope.
    Owned,
}

impl Output {
    /// The success envelope from a JSON object of fields, for example
    /// `Output::success(json!({"plan": "pay-4521", "already": true}))`.
    ///
    /// Panics if `fields` is not an object or contains `"ok"`: both are bugs
    /// in the command, not input errors.
    pub fn success(fields: Value) -> Output {
        match fields {
            Value::Object(map) => {
                assert!(
                    !map.contains_key("ok"),
                    "a success field may not be named \"ok\""
                );
                Output::Success(map)
            }
            other => panic!("a success envelope needs a JSON object, got {other}"),
        }
    }
}

/// The success envelope, `"ok"` first.
struct SuccessEnvelope<'a>(&'a Map<String, Value>);

impl Serialize for SuccessEnvelope<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len() + 1))?;
        map.serialize_entry("ok", &true)?;
        for (key, value) in self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// Prints `result` as one JSON value and a newline on `stdout`, and returns the
/// exit code of contract §1.2.
pub fn emit(result: Result<Output, CpdError>, stdout: &mut impl Write) -> ExitCode {
    let (line, exit) = match &result {
        Ok(Output::Success(fields)) => (Some(to_line(&SuccessEnvelope(fields))), Exit::Ok),
        Ok(Output::Owned) => (None, Exit::Ok),
        Err(err) => (Some(to_line(err)), err.exit()),
    };
    if let Some(line) = line
        && let Err(io) = stdout
            .write_all(line.as_bytes())
            .and_then(|()| stdout.flush())
    {
        eprintln!("cpd: cannot write the result to stdout: {io}");
        return ExitCode::from(exit.code().max(Exit::Failure.code()));
    }
    ExitCode::from(exit.code())
}

/// One JSON value on one line.
pub fn to_line(value: &impl Serialize) -> String {
    let mut line = serde_json::to_string(value).expect("JSON values always serialise");
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::json;

    fn run(result: Result<Output, CpdError>) -> (String, ExitCode) {
        let mut out = Vec::new();
        let code = emit(result, &mut out);
        (String::from_utf8(out).unwrap(), code)
    }

    #[test]
    fn success_prints_one_value_with_ok_first() {
        let (out, code) = run(Ok(Output::success(
            json!({"plan": "pay-4521", "already": true}),
        )));
        assert_eq!(
            out,
            "{\"ok\":true,\"already\":true,\"plan\":\"pay-4521\"}\n"
        );
        assert_eq!(code, ExitCode::from(0));
    }

    #[test]
    fn success_without_fields_is_just_ok() {
        let (out, _) = run(Ok(Output::success(json!({}))));
        assert_eq!(out, "{\"ok\":true}\n");
    }

    #[test]
    fn failure_prints_the_envelope_and_exits_with_the_codes_exit() {
        for (code, exit) in [
            (ErrorCode::GitFailed, 1),
            (ErrorCode::Usage, 2),
            (ErrorCode::IllegalTransition, 3),
            (ErrorCode::PlanNotFound, 4),
            (ErrorCode::StoreBusy, 5),
        ] {
            let (out, status) = run(Err(
                CpdError::new(code, "Nope.").with("argv", json!(["git"]))
            ));
            assert_eq!(status, ExitCode::from(exit), "{code}");
            assert_eq!(out.lines().count(), 1);
            let value: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(
                value,
                json!({"ok": false, "error": {"code": code.as_str(), "message": "Nope.", "details": {"argv": ["git"]}}})
            );
        }
    }

    #[test]
    fn owned_terminal_prints_nothing_and_exits_zero() {
        let (out, code) = run(Ok(Output::Owned));
        assert_eq!(out, "");
        assert_eq!(code, ExitCode::from(0));
    }

    #[test]
    #[should_panic(expected = "JSON object")]
    fn success_refuses_a_non_object() {
        Output::success(json!([1, 2]));
    }

    #[test]
    #[should_panic(expected = "\"ok\"")]
    fn success_refuses_an_ok_field() {
        Output::success(json!({"ok": false}));
    }

    struct BrokenPipe;
    impl Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn unwritable_stdout_is_a_failure() {
        let code = emit(Ok(Output::success(json!({}))), &mut BrokenPipe);
        assert_eq!(code, ExitCode::from(1));
        let code = emit(
            Err(CpdError::new(ErrorCode::StoreBusy, "Busy.")),
            &mut BrokenPipe,
        );
        assert_eq!(code, ExitCode::from(5));
    }
}
