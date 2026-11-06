//! Endpoint health of the classifier (swr-classifier.md §4): the adapter records on `swr_endpoints`; the engine's
//! `due_report` raises the outage once, after ten minutes. A temp `$CPD_HOME`, a scripted `FakeRunner`, a
//! `ManualClock`: no node, no network, no sleep.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::endpoint;
use crate::classifier::{Choice, ClassifierPort, Label, PiClassifier};
use crate::clock::{Clock, ManualClock};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::store::Store;

const TEN: Duration = Duration::from_secs(600);

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    clock: Arc<ManualClock>,
    port: PiClassifier,
}

fn fixture(replies: Vec<Reply>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    std::fs::create_dir_all(home.run_dir()).unwrap();
    let clock = Arc::new(ManualClock::new());
    let mut runner = FakeRunner::new((*clock).clone());
    for reply in replies {
        runner = runner.once("node", |_| true, reply);
    }
    let port = PiClassifier::new(
        Arc::new(runner),
        PathBuf::from("/pi/lib/classify.ts"),
        PathBuf::from("/agent"),
        &home,
    )
    .with_clock(clock.clone());
    Fixture {
        _dir: dir,
        home,
        clock,
        port,
    }
}

fn down() -> Reply {
    Reply::exit(
        1,
        r#"{"ok":false,"error":{"code":"classifier_unreachable","message":"down","details":{}}}"#,
        "",
    )
}

fn up() -> Reply {
    Reply::ok(r#"{"ok":true,"label":"yes","model":"m"}"#)
}

impl Fixture {
    fn choose(&self) {
        let state = serde_json::json!({});
        let labels = [Label {
            name: "yes",
            definition: "agree",
        }];
        let _ = self.port.choose(&Choice {
            state: &state,
            question: "ok?",
            labels: &labels,
        });
    }
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }
    fn errors(&self) -> i64 {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM events WHERE kind = 'error'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
    fn due(&self) -> bool {
        endpoint::due_report(&mut self.store(), "ollama.lab", self.clock.utc()).unwrap()
    }
}

#[test]
fn health_classifier_outage_raises_one_error_event_per_outage() {
    let fx = fixture(vec![down(), down(), down(), down()]);
    let t0 = fx.clock.utc();
    fx.choose();
    let row = endpoint::get(&mut fx.store(), "ollama.lab")
        .unwrap()
        .expect("the ollama.lab row");
    assert_eq!(row.failing_since, Some(t0));
    assert_eq!(
        fx.errors(),
        0,
        "the adapter raises no Event; the engine does"
    );
    assert!(!fx.due());

    fx.clock.advance(TEN / 2);
    fx.choose();
    assert!(!fx.due(), "five minutes is not an outage yet");

    fx.clock.advance(TEN / 2);
    fx.choose();
    assert!(fx.due(), "ten minutes of failure is reported");
    assert!(!fx.due(), "once per outage");
    fx.choose();
    assert!(
        !fx.due(),
        "a later failure of the same outage is not reported again"
    );

    assert_eq!(fx.errors(), 0, "the adapter never raises the Event");
}

#[test]
fn health_classifier_success_clears_the_outage() {
    let fx = fixture(vec![down(), up(), down()]);
    fx.choose();
    fx.clock.advance(TEN);
    assert!(fx.due());
    fx.choose();
    let row = endpoint::get(&mut fx.store(), "ollama.lab")
        .unwrap()
        .expect("the row");
    assert_eq!(row.failing_since, None);
    assert_eq!(row.reported_at, None);
    assert_eq!(row.error_count, 0);
    assert_eq!(row.status.as_deref(), Some("success"));
    assert_eq!(row.last_ok_at, Some(fx.clock.utc()));

    fx.choose();
    fx.clock.advance(TEN);
    assert!(fx.due(), "the next outage is reported again");
}

#[test]
fn health_bifrost_failure_is_recorded_on_the_bifrost_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    let unreachable: Result<(), CpdError> = Err(CpdError::new(
        ErrorCode::EmbeddingUnreachable,
        "bifrost down".to_owned(),
    ));
    super::health::record_result(&home, "bifrost", t0, &unreachable);
    let mut store = Store::open(&home, 5000).unwrap();
    let row = endpoint::get(&mut store, "bifrost").unwrap().expect("row");
    assert_eq!(row.status.as_deref(), Some("error"));
    assert_eq!(row.failing_since, Some(t0));
    assert_eq!(row.error_count, 1);
    assert!(endpoint::get(&mut store, "ollama.lab").unwrap().is_none());

    let refused: Result<(), CpdError> =
        Err(CpdError::new(ErrorCode::JudgeRefused, "500".to_owned()));
    super::health::record_result(&home, "bifrost", t0 + Duration::from_secs(1), &refused);
    let row = endpoint::get(&mut store, "bifrost").unwrap().expect("row");
    assert_eq!(
        row.failing_since, None,
        "a refusal means the endpoint answered"
    );
    assert_eq!(row.status.as_deref(), Some("success"));

    let bad: Result<(), CpdError> = Err(CpdError::new(
        ErrorCode::EmbeddingBadReply,
        "no line".to_owned(),
    ));
    super::health::record_result(&home, "bifrost", t0 + Duration::from_secs(2), &bad);
    let row = endpoint::get(&mut store, "bifrost").unwrap().expect("row");
    assert_eq!(row.error_count, 0, "a bad reply is not recorded");
    assert_eq!(row.status.as_deref(), Some("success"));
}
