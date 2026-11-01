//! The `op` calls (commands.md §24 "op calls"). Each returns the JSON fields the command adds to its output
//! (`value`, `items`, `created`, `edited`, `archived`). A value is only ever passed in memory (stdin of `op`),
//! never in argv, and never appears in an error.

use std::io::ErrorKind;
use std::process::Output;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::ports::Ports;

/// The fields an op call adds to the command's output.
pub type Fields = Map<String, Value>;

const ACCOUNT: &str = "my.1password.com";

/// An op call may wait for the 1Password app's per-use prompt (the User's approval), so the deadline is generous.
const DEADLINE: Duration = Duration::from_secs(120);

/// op's stderr substrings (matched case-insensitively) per error code. UNVERIFIED against a real op
/// (commands.md §24.0): the User's smoke run proves them. Checked in order; no match is `secret_op_failed`.
const CLASSES: &[(ErrorCode, &[&str])] = &[
    (
        ErrorCode::SecretNotSignedIn,
        &[
            "not signed in",
            "not currently signed in",
            "session expired",
        ],
    ),
    (
        ErrorCode::SecretNotFound,
        &[
            "isn't an item",
            "isn't a vault",
            "isn't a field",
            "could not be found",
        ],
    ),
];

/// What a failed `op` call is: one place for the classification.
fn classify(exit: Option<i32>, stderr: &str) -> CpdError {
    let lower = stderr.to_lowercase();
    let code = CLASSES
        .iter()
        .find(|(_, needles)| needles.iter().any(|n| lower.contains(n)))
        .map(|(c, _)| *c);
    let tail = tail(stderr.trim_end(), TAIL_LINES);
    match code {
        Some(ErrorCode::SecretNotSignedIn) => CpdError::new(
            ErrorCode::SecretNotSignedIn,
            "1Password is not signed in: sign in with `op signin` (or the 1Password app), then retry",
        ),
        Some(code) => {
            CpdError::new(code, "1Password has no such item, vault or field").with("stderr", tail)
        }
        None => CpdError::new(ErrorCode::SecretOpFailed, "op failed")
            .with("exit", json!(exit))
            .with("stderr", tail),
    }
}

/// Runs `op <args> --account my.1password.com`, with `stdin` (if any) written and closed. Returns stdout on success.
fn run(args: &[&str], stdin: Option<&str>) -> Result<String, CpdError> {
    run_with(&*Ports::current().runner, args, stdin)
}

/// [`run`] on `runner`. The value is `Call::stdin`: the runner writes it and closes the pipe, and a fake records it.
fn run_with(runner: &dyn Runner, args: &[&str], stdin: Option<&str>) -> Result<String, CpdError> {
    let call = Call {
        tool: "op".to_owned(),
        program: "op".to_owned(),
        args: args
            .iter()
            .copied()
            .chain(["--account", ACCOUNT])
            .map(str::to_owned)
            .collect(),
        envs: Vec::new(),
        cwd: None,
        deadline: DEADLINE,
        stdin: stdin.map(|data| data.as_bytes().to_vec()),
        stderr_to: None,
    };
    let result = runner.run(&call);
    let out: Output = result.map_err(|e| match e {
        Failed::Io(e) if e.kind() == ErrorKind::NotFound => CpdError::new(
            ErrorCode::SecretOpMissing,
            "`op` (the 1Password CLI) is not on PATH",
        ),
        Failed::Io(e) => io_failed(e),
        Failed::Timeout(e) => e,
    })?;
    if !out.status.success() {
        return Err(classify(
            out.status.code(),
            &String::from_utf8_lossy(&out.stderr),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn io_failed(e: std::io::Error) -> CpdError {
    CpdError::new(
        ErrorCode::SecretOpFailed,
        format!("op could not be run: {}", e.kind()),
    )
}

fn bad_json(what: &str) -> CpdError {
    CpdError::new(
        ErrorCode::SecretOpFailed,
        format!("op returned {what} that is not the expected JSON"),
    )
}

fn fields(pairs: [(&str, Value); 1]) -> Fields {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// `op item get <item> --vault <vault> --fields label=<field> --reveal`: `{value}`.
pub fn get(vault: &str, item: &str, field: &str) -> Result<Fields, CpdError> {
    get_with(&*Ports::current().runner, vault, item, field)
}

/// [`get`] on `runner`.
pub fn get_with(
    runner: &dyn Runner,
    vault: &str,
    item: &str,
    field: &str,
) -> Result<Fields, CpdError> {
    let label = format!("label={field}");
    let out = run_with(
        runner,
        &[
            "item", "get", item, "--vault", vault, "--fields", &label, "--reveal",
        ],
        None,
    )?;
    let value = out.strip_suffix('\n').unwrap_or(&out);
    Ok(fields([("value", json!(value))]))
}

/// `op item list --vault <vault> --format json`: `{items: [{id, title}]}`.
pub fn list(vault: &str) -> Result<Fields, CpdError> {
    list_with(&*Ports::current().runner, vault)
}

/// [`list`] on `runner`.
pub fn list_with(runner: &dyn Runner, vault: &str) -> Result<Fields, CpdError> {
    let out = run_with(
        runner,
        &["item", "list", "--vault", vault, "--format", "json"],
        None,
    )?;
    let entries: Vec<Value> = serde_json::from_str(&out).map_err(|_| bad_json("a list"))?;
    let items = entries
        .iter()
        .map(|e| json!({"id": e.get("id"), "title": e.get("title")}))
        .collect::<Vec<_>>();
    Ok(fields([("items", json!(items))]))
}

/// The item's JSON, or the classified error.
fn item_json(vault: &str, item: &str) -> Result<Value, CpdError> {
    let out = run(
        &["item", "get", item, "--vault", vault, "--format", "json"],
        None,
    )?;
    serde_json::from_str(&out).map_err(|_| bad_json("an item"))
}

/// Creates the item with one concealed field: `{created: true}`.
pub fn create(vault: &str, item: &str, field: &str, value: &str) -> Result<Fields, CpdError> {
    match item_json(vault, item) {
        Ok(_) => {
            return Err(CpdError::new(
                ErrorCode::SecretExists,
                "the item already exists in the vault",
            )
            .with("vault", vault)
            .with("item", item));
        }
        Err(e) if e.code() == ErrorCode::SecretNotFound => {}
        Err(e) => return Err(e),
    }
    let template = json!({
        "title": item,
        "category": "SECURE_NOTE",
        "fields": [{"id": field, "label": field, "type": "CONCEALED", "value": value}],
    });
    run(
        &["item", "create", "--vault", vault, "-"],
        Some(&template.to_string()),
    )?;
    Ok(fields([("created", json!(true))]))
}

/// Sets (or adds) the field: `{edited: true}`.
pub fn edit(vault: &str, item: &str, field: &str, value: &str) -> Result<Fields, CpdError> {
    let mut doc = item_json(vault, item)?;
    let obj = doc.as_object_mut().ok_or_else(|| bad_json("an item"))?;
    let list = obj.entry("fields").or_insert_with(|| json!([]));
    let list = list.as_array_mut().ok_or_else(|| bad_json("an item"))?;
    match list
        .iter_mut()
        .find(|f| f.get("label").and_then(Value::as_str) == Some(field))
    {
        Some(f) => f["value"] = json!(value),
        None => {
            list.push(json!({"id": field, "label": field, "type": "CONCEALED", "value": value}))
        }
    }
    run(
        &["item", "edit", item, "--vault", vault],
        Some(&doc.to_string()),
    )?;
    Ok(fields([("edited", json!(true))]))
}

/// Archives the item: `{archived: true}`.
pub fn delete(vault: &str, item: &str) -> Result<Fields, CpdError> {
    run(
        &["item", "delete", item, "--vault", vault, "--archive"],
        None,
    )?;
    Ok(fields([("archived", json!(true))]))
}
