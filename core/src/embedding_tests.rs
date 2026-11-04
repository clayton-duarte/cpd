use crate::embedding::{Embedded, EmbeddingPort};
use crate::error::{CpdError, ErrorCode};
use crate::fakes::embedding::FakeEmbedding;

fn embedded(v: f32) -> Embedded {
    Embedded {
        vector: vec![v; 3],
        model: "vertex/gemini-embedding-2".into(),
        tokens: Some(2),
    }
}

#[test]
fn fake_embedding_answers_in_order_and_records_calls() {
    let fake = FakeEmbedding::new(vec![Ok(embedded(1.0)), Ok(embedded(2.0))]);
    assert_eq!(fake.embed("one", 3).unwrap(), embedded(1.0));
    assert_eq!(fake.embed("two", 768).unwrap(), embedded(2.0));
    assert_eq!(fake.embed("three", 3).unwrap(), embedded(2.0));
    assert_eq!(
        fake.calls(),
        vec![
            ("one".to_string(), 3),
            ("two".to_string(), 768),
            ("three".to_string(), 3)
        ]
    );
}

#[test]
fn fake_embedding_returns_a_scripted_error() {
    let error = CpdError::new(ErrorCode::EmbeddingUnreachable, "Down.");
    let fake = FakeEmbedding::new(vec![Err(error.clone())]);
    assert_eq!(fake.embed("x", 3).unwrap_err(), error);
}
