//! `BifrostEmbedding` over a scripted `FakeRunner`: no node, no network, no sleep.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::clock::ManualClock;
use crate::embedding::{BifrostEmbedding, EmbeddingPort};
use crate::error::ErrorCode;
use crate::host::runner::{Call, FakeRunner, Reply};

const DEADLINE_PAST: Duration = Duration::from_secs(3600);

fn adapter(reply: Reply) -> (BifrostEmbedding, FakeRunner) {
    let runner = FakeRunner::new(ManualClock::new()).on("node", |_| true, reply);
    let port = BifrostEmbedding::new(
        Arc::new(runner.clone()),
        PathBuf::from("/pi/lib/embed.ts"),
        PathBuf::from("/agent"),
    );
    (port, runner)
}

fn vector_reply(len: usize) -> Reply {
    let v = vec!["0.25"; len].join(",");
    Reply::ok(&format!(
        r#"{{"ok":true,"vector":[{v}],"model":"vertex/gemini-embedding-2","tokens":7}}"#
    ))
}

fn helper_error(code: &str) -> Reply {
    Reply::exit(
        1,
        &format!(
            r#"{{"ok":false,"error":{{"code":"{code}","message":"m","details":{{"status":503}}}}}}"#
        ),
        "",
    )
}

fn code_of(reply: Reply) -> ErrorCode {
    adapter(reply).0.embed("t", 3).unwrap_err().code()
}

#[test]
fn embedding_adapter_sends_text_on_stdin_and_no_key_in_argv_envs_or_error() {
    let (port, runner) = adapter(vector_reply(3));
    let got = port.embed("hello wörld", 3).unwrap();
    assert_eq!(got.vector, vec![0.25, 0.25, 0.25]);
    assert_eq!(got.model, "vertex/gemini-embedding-2");
    assert_eq!(got.tokens, Some(7));
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let call: &Call = &calls[0];
    assert_eq!(Path::new(&call.program).file_name().unwrap(), "node");
    assert_eq!(call.args, vec!["/pi/lib/embed.ts".to_owned()]);
    assert_eq!(call.stdin.as_deref(), Some("hello wörld".as_bytes()));
    let shown = format!("{:?}{:?}", call.args, call.envs).to_lowercase();
    for secret in ["bearer", "sk-", "virtual_key", "api_key"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }

    // A failing helper whose stderr holds a key: the error shows none of it.
    let (port, _) = adapter(Reply::exit(
        1,
        "garbage",
        "Authorization: Bearer sk-LEAK123",
    ));
    let err = port.embed("t", 3).unwrap_err();
    let shown = format!("{} {:?}", err.message(), err.details());
    assert!(!shown.contains("sk-LEAK123"), "{shown}");
}

#[test]
fn embedding_adapter_refuses_wrong_dimension_or_non_finite_vector() {
    assert_eq!(code_of(vector_reply(2)), ErrorCode::EmbeddingBadReply);
    assert_eq!(code_of(vector_reply(4)), ErrorCode::EmbeddingBadReply);
    let nan = Reply::ok(r#"{"ok":true,"vector":[0.1,1e999,0.3],"model":"m"}"#);
    assert_eq!(code_of(nan), ErrorCode::EmbeddingBadReply);
    let null = Reply::ok(r#"{"ok":true,"vector":[0.1,null,0.3],"model":"m"}"#);
    assert_eq!(code_of(null), ErrorCode::EmbeddingBadReply);
    assert_eq!(code_of(Reply::ok("not json")), ErrorCode::EmbeddingBadReply);
    assert_eq!(code_of(Reply::ok("")), ErrorCode::EmbeddingBadReply);
}

#[test]
fn embedding_adapter_maps_helper_errors_to_embedding_codes() {
    assert_eq!(
        code_of(helper_error("embedding_unreachable")),
        ErrorCode::EmbeddingUnreachable
    );
    assert_eq!(
        code_of(helper_error("embedding_bad_reply")),
        ErrorCode::EmbeddingBadReply
    );
    let (port, _) = adapter(helper_error("embedding_refused"));
    let err = port.embed("t", 3).unwrap_err();
    assert_eq!(err.code(), ErrorCode::EmbeddingRefused);
    assert_eq!(err.details().get("status"), Some(&serde_json::json!(503)));
}

#[test]
fn embedding_adapter_maps_timeout_exit_and_io_to_embedding_unreachable() {
    let timed_out = vector_reply(3);
    let timed_out = match timed_out {
        Reply::Exit { .. } => timed_out.takes(DEADLINE_PAST),
        other => other,
    };
    assert_eq!(code_of(timed_out), ErrorCode::EmbeddingUnreachable);
    assert_eq!(
        code_of(Reply::exit(1, "", "boom")),
        ErrorCode::EmbeddingUnreachable
    );
    assert_eq!(
        code_of(Reply::exit(137, "", "")),
        ErrorCode::EmbeddingUnreachable
    );
    assert_eq!(
        code_of(Reply::io(ErrorKind::NotFound)),
        ErrorCode::EmbeddingUnreachable
    );
}
