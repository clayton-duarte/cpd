//! `PiClassifier` over a scripted `FakeRunner`, over a temp `$CPD_HOME`: no node, no network, no sleep.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::classifier::{Choice, ClassifierPort, Label, PiClassifier};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::store::Store;

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    runner: FakeRunner,
    port: PiClassifier,
}

fn fixture(replies: Vec<Reply>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::create_dir_all(home.run_dir()).unwrap();
    let mut runner = FakeRunner::new(ManualClock::new());
    for reply in replies {
        runner = runner.once("node", |_| true, reply);
    }
    let port = PiClassifier::new(
        Arc::new(runner.clone()),
        PathBuf::from("/pi/lib/classify.ts"),
        PathBuf::from("/agent"),
        &home,
    );
    Fixture {
        _dir: dir,
        home,
        runner,
        port,
    }
}

impl Fixture {
    fn choose(&self) -> Result<crate::classifier::Chosen, crate::error::CpdError> {
        let state = json!({"k": 1});
        let labels = [
            Label {
                name: "yes",
                definition: "agree",
            },
            Label {
                name: "no",
                definition: "disagree",
            },
        ];
        self.port.choose(&Choice {
            state: &state,
            question: "ok?",
            labels: &labels,
        })
    }
    /// Every `error` Event's `data`.
    fn errors(&self) -> Vec<Value> {
        let mut store = Store::open(&self.home, 5000).unwrap();
        store
            .read(|c| {
                let mut stmt =
                    c.prepare("SELECT data FROM events WHERE kind = 'error' ORDER BY id")?;
                Ok(stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
                    .collect::<Vec<Value>>())
            })
            .unwrap()
    }
}

fn answer(label: &str) -> Reply {
    Reply::ok(&format!(
        r#"{{"ok":true,"label":"{label}","confidence":0.75,"model":"llama.cpp/kev-4b","usage":{{"input_tokens":12,"output_tokens":2}}}}"#
    ))
}

fn unreachable_exit() -> Reply {
    Reply::exit(
        1,
        r#"{"ok":false,"error":{"code":"classifier_unreachable","message":"down","details":{}}}"#,
        "",
    )
}

fn code_of(reply: Reply) -> ErrorCode {
    fixture(vec![reply]).choose().unwrap_err().code()
}

#[test]
fn classifier_adapter_sends_choice_json_on_stdin() {
    let fx = fixture(vec![answer("yes")]);
    fx.choose().unwrap();
    let calls = fx.runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(Path::new(&calls[0].program).file_name().unwrap(), "node");
    assert_eq!(calls[0].args, vec!["/pi/lib/classify.ts".to_owned()]);
    let sent: Value = serde_json::from_slice(calls[0].stdin.as_ref().unwrap()).unwrap();
    assert_eq!(
        sent,
        json!({"state": {"k": 1}, "question": "ok?", "labels": [
            {"name": "yes", "definition": "agree"}, {"name": "no", "definition": "disagree"}]})
    );
    let shown = format!("{:?}{:?}", calls[0].args, calls[0].envs).to_lowercase();
    for secret in ["bearer", "sk-", "api_key"] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
}

#[test]
fn classifier_adapter_maps_answer_to_chosen() {
    let got = fixture(vec![answer("no")]).choose().unwrap();
    assert_eq!(got.label, "no");
    assert_eq!(got.confidence, Some(0.75));
    assert_eq!(got.model, "llama.cpp/kev-4b");
    let usage = got.usage.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (12, 2));
    let bare = fixture(vec![Reply::ok(r#"{"ok":true,"label":"yes","model":"m"}"#)])
        .choose()
        .unwrap();
    assert_eq!((bare.confidence, bare.usage), (None, None));
    let refused = fixture(vec![Reply::exit(
        1,
        r#"{"ok":false,"error":{"code":"classifier_refused","message":"m","details":{"status":500}}}"#,
        "",
    )])
    .choose()
    .unwrap_err();
    assert_eq!(refused.code(), ErrorCode::ClassifierRefused);
    assert_eq!(refused.details().get("status"), Some(&json!(500)));
}

#[test]
fn classifier_adapter_refuses_a_label_outside_the_choice() {
    assert_eq!(code_of(answer("maybe")), ErrorCode::ClassifierBadReply);
    assert_eq!(
        code_of(Reply::ok("not json")),
        ErrorCode::ClassifierBadReply
    );
    assert_eq!(code_of(Reply::ok("")), ErrorCode::ClassifierBadReply);
}

#[test]
fn classifier_adapter_maps_timeout_exit_and_io_to_classifier_unreachable() {
    let slow = answer("yes").takes(Duration::from_secs(3600));
    assert_eq!(code_of(slow), ErrorCode::ClassifierUnreachable);
    assert_eq!(
        code_of(Reply::exit(1, "", "boom")),
        ErrorCode::ClassifierUnreachable
    );
    assert_eq!(
        code_of(Reply::exit(137, "", "")),
        ErrorCode::ClassifierUnreachable
    );
    assert_eq!(
        code_of(Reply::io(ErrorKind::NotFound)),
        ErrorCode::ClassifierUnreachable
    );
    assert_eq!(
        code_of(unreachable_exit()),
        ErrorCode::ClassifierUnreachable
    );
}

#[test]
fn refused_or_bad_reply_raise_no_error_event() {
    let refused = Reply::exit(
        1,
        r#"{"ok":false,"error":{"code":"classifier_refused","message":"m","details":{"status":500}}}"#,
        "",
    );
    let fx = fixture(vec![refused, answer("maybe"), Reply::ok("junk")]);
    for _ in 0..3 {
        fx.choose().unwrap_err();
    }
    assert!(fx.errors().is_empty());
}
