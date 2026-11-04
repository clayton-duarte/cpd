use serde_json::json;

use crate::classifier::{Choice, Chosen, ClassifierPort, Label, Usage};
use crate::error::{CpdError, ErrorCode};
use crate::fakes::classifier::{ChoiceCall, FakeClassifier};

fn chosen(label: &str) -> Chosen {
    Chosen {
        label: label.into(),
        confidence: Some(0.5),
        usage: Some(Usage {
            input_tokens: 3,
            output_tokens: 1,
        }),
        model: "llama.cpp/kev-4b".into(),
    }
}

#[test]
fn fake_classifier_answers_in_order_and_repeats_the_last() {
    let fake = FakeClassifier::new(vec![Ok(chosen("a")), Ok(chosen("b"))]);
    let state = json!({});
    let labels = [Label {
        name: "a",
        definition: "first",
    }];
    let q = Choice {
        state: &state,
        question: "which?",
        labels: &labels,
    };
    assert_eq!(fake.choose(&q).unwrap(), chosen("a"));
    assert_eq!(fake.choose(&q).unwrap(), chosen("b"));
    assert_eq!(fake.choose(&q).unwrap(), chosen("b"));
}

#[test]
fn fake_classifier_records_every_call() {
    let fake = FakeClassifier::answering(chosen("x"));
    let state = json!({"k": 1});
    let labels = [
        Label {
            name: "x",
            definition: "the x",
        },
        Label {
            name: "y",
            definition: "the y",
        },
    ];
    let q = Choice {
        state: &state,
        question: "pick",
        labels: &labels,
    };
    fake.choose(&q).unwrap();
    fake.choose(&q).unwrap();
    let call = ChoiceCall {
        state: json!({"k": 1}),
        question: "pick".into(),
        labels: vec![("x".into(), "the x".into()), ("y".into(), "the y".into())],
    };
    assert_eq!(fake.calls(), vec![call.clone(), call]);
}

#[test]
fn fake_classifier_returns_a_scripted_error() {
    let error = CpdError::new(ErrorCode::ClassifierRefused, "No.").with("status", 503);
    let fake = FakeClassifier::new(vec![Err(error.clone())]);
    let state = json!({});
    let q = Choice {
        state: &state,
        question: "q",
        labels: &[],
    };
    assert_eq!(fake.choose(&q).unwrap_err(), error);
}

#[test]
fn classifier_and_embedding_error_codes_serialize_to_their_names() {
    let cases = [
        (ErrorCode::ClassifierUnreachable, "classifier_unreachable"),
        (ErrorCode::ClassifierRefused, "classifier_refused"),
        (ErrorCode::ClassifierBadReply, "classifier_bad_reply"),
        (ErrorCode::EmbeddingUnreachable, "embedding_unreachable"),
        (ErrorCode::EmbeddingRefused, "embedding_refused"),
        (ErrorCode::EmbeddingBadReply, "embedding_bad_reply"),
    ];
    let state = json!({});
    let q = Choice {
        state: &state,
        question: "q",
        labels: &[],
    };
    for (code, name) in cases {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(name));
        assert_eq!(code.exit().code(), 1, "{name}");
        // The scripted error comes back from the fake with its code unchanged.
        let error = CpdError::new(code, "scripted");
        let fake = FakeClassifier::new(vec![Err(error.clone())]);
        let value = serde_json::to_value(fake.choose(&q).unwrap_err()).unwrap();
        assert_eq!(value["error"]["code"], json!(name));
    }
}
