//! `PiClassifier` no longer keeps `run/classifier-unreachable.json` (swr-classifier.md §4.4): over a temp
//! `$CPD_HOME` and a scripted `FakeRunner`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::classifier::{Choice, ClassifierPort, Label, PiClassifier};
use crate::clock::ManualClock;
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};

#[test]
fn health_classifier_failure_does_not_write_classifier_unreachable_json() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::create_dir_all(home.run_dir()).unwrap();
    let clock = Arc::new(ManualClock::new());
    let runner = FakeRunner::new((*clock).clone()).once(
        "node",
        |_| true,
        Reply::exit(
            1,
            r#"{"ok":false,"error":{"code":"classifier_unreachable","message":"down","details":{}}}"#,
            "",
        ),
    );
    let port = PiClassifier::new(
        Arc::new(runner),
        PathBuf::from("/pi/lib/classify.ts"),
        PathBuf::from("/agent"),
        &home,
    )
    .with_clock(clock);
    let state = serde_json::json!({});
    let labels = [Label {
        name: "yes",
        definition: "agree",
    }];
    let _ = port.choose(&Choice {
        state: &state,
        question: "ok?",
        labels: &labels,
    });
    assert!(
        !home.run_dir().join("classifier-unreachable.json").exists(),
        "the outage marker file is gone"
    );
}
