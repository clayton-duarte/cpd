//! `cpd argv-check` (docs/contract.md §5.6): the parse-only check of the web-to-CLI argv
//! contract. It reads a JSON array of string arrays on stdin, parses each with the real
//! [`Cli`] grammar and touches nothing else — no dispatch, no store, no `$CPD_HOME`.

use std::io::{Read, Write};

use clap::Parser;
use clap::error::ErrorKind;
use serde::Serialize;
use serde_json::json;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;

use crate::cli::Cli;

/// One refused argv: the exact inner array as received and the parser's message.
#[derive(Serialize)]
struct Failure<'a> {
    argv: &'a [String],
    error: String,
}

/// The failure document, `"ok"` first: `{"ok":false,"failures":[…]}`.
#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    failures: &'a [Failure<'a>],
}

/// Parses every argv on stdin. When all parse: [`Output::success`] with no fields, so the one
/// JSON value is exactly `{"ok":true}` and the exit code is 0. When any fails: the
/// `{"ok":false,"failures":[…]}` document on stdout and a non-zero exit, so it never returns.
pub fn run() -> Result<Output, CpdError> {
    let input = read_stdin()?;
    let arrays: Vec<Vec<String>> = serde_json::from_str(&input).map_err(|e| {
        CpdError::new(
            ErrorCode::Usage,
            format!("argv-check: stdin is not a JSON array of string arrays: {e}"),
        )
    })?;
    let failures: Vec<Failure> = arrays
        .iter()
        .filter_map(|args| parse(args).map(|error| Failure { argv: args, error }))
        .collect();
    if failures.is_empty() {
        return Ok(Output::success(json!({})));
    }
    print_failures(&Report {
        ok: false,
        failures: &failures,
    });
    std::process::exit(1);
}

/// Parses one argv (after the program name) with the real parser. `None` when it parses; the
/// parser's message when it does not. `--help` and `--version` are how clap reports success for
/// those, not refusals.
fn parse(args: &[String]) -> Option<String> {
    let argv = std::iter::once("cpd").chain(args.iter().map(String::as_str));
    match Cli::try_parse_from(argv) {
        Ok(_) => None,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            None
        }
        Err(err) => Some(err.to_string()),
    }
}

fn read_stdin() -> Result<String, CpdError> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("argv-check: cannot read stdin: {e}"),
        )
    })?;
    Ok(input)
}

fn print_failures(report: &Report) {
    let line = serde_json::to_string(report).expect("the argv-check report always serialises");
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
}
