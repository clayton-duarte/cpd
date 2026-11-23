//! Trusted acquisition exercises a disposable Store and an in-process host/sink.
//! No process, native runner, or durable host is launched.
use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::handoff::{
    StageHost, StageNativeArgs, StageRequest, StageResult, stage_native_with_host,
};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::store::Store;

const SOURCE: &[u8] = concat!(
    "{\"type\":\"session\",\"version\":3,\"id\":\"native-not-cpd-id\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"cwd\":\"/work\"}\n",
    "{\"type\":\"custom_message\",\"id\":\"a\",\"parentId\":null,\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"customType\":\"cpd\",\"content\":\"notice\",\"display\":true,\"details\":{\"delivered_to\":\"child\"}}\n"
).as_bytes();

#[derive(Default)]
struct Host {
    calls: Vec<StageRequest>,
}
impl StageHost for Host {
    fn stage(&mut self, request: StageRequest) -> StageResult {
        let source_sha256 = request.source_sha256.clone();
        self.calls.push(request);
        StageResult::Staged {
            conversation_id: "root".into(),
            source_sha256,
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("home").into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store.write("fixture", |tx| {
            tx.execute_batch("INSERT INTO contexts (id,title,created_at,created_on) VALUES ('ctx','Ctx','t','m');
                INSERT INTO sessions (id,role,context,name,cwd,tier,state,state_at,created_at)
                VALUES ('ancestor','lead','ctx','Ancestor','/work','O3','stopped','t','t'),
                       ('child','lead','ctx','Child','/work','O3','stopped','t','t');")?;
            Ok(())
        }).unwrap();
        let source = dir.path().join("native.jsonl");
        fs::write(&source, SOURCE).unwrap();
        Self {
            _dir: dir,
            store,
            source,
        }
    }
    fn associate(&mut self, id: &str, file: Option<&str>, fork: Option<&str>) {
        self.store
            .write("associate fixture", |tx| {
                tx.execute(
                    "UPDATE sessions SET file=?1, forked_from=?2 WHERE id=?3",
                    (file, fork, id),
                )?;
                Ok(())
            })
            .unwrap();
    }
    fn run(&self, id: &str, host: &mut Host) -> Result<StageResult, crate::error::CpdError> {
        stage_native_with_host(
            &self.store,
            StageNativeArgs {
                session_id: SessionId::parse(id).unwrap(),
            },
            host,
        )
    }
    fn unchanged(&self) {
        assert_eq!(fs::read(&self.source).unwrap(), SOURCE);
    }
}

#[test]
fn trusted_binding_stages_matching_session() {
    let mut f = Fixture::new();
    let path = f.source.to_str().unwrap().to_owned();
    f.associate("child", Some(&path), Some("ancestor"));
    let mut host = Host::default();
    let digest = format!("{:x}", Sha256::digest(SOURCE));
    assert_eq!(
        f.run("child", &mut host).unwrap(),
        StageResult::Staged {
            conversation_id: "root".into(),
            source_sha256: digest.clone()
        }
    );
    assert_eq!(host.calls.len(), 1);
    let request = &host.calls[0];
    assert_eq!(request.session_id, SessionId::parse("child").unwrap());
    assert_eq!(
        request.forked_from,
        Some(SessionId::parse("ancestor").unwrap())
    );
    assert_eq!(request.source_bytes, SOURCE);
    assert_eq!(request.source_sha256, digest);
    f.unchanged();
}

#[test]
fn trusted_binding_rejects_mismatched_session() {
    let mut f = Fixture::new();
    let path = f.source.to_str().unwrap().to_owned();
    // An ancestor's recorded file containing a child delivery marker is not
    // evidence that the file is the child's own recorded fork transcript.
    f.associate("ancestor", Some(&path), None);
    f.associate("child", Some(&path), Some("ancestor"));
    let mut host = Host::default();
    assert!(matches!(
        f.run("child", &mut host),
        Ok(StageResult::Refused) | Err(_)
    ));
    assert!(host.calls.is_empty(), "no host or sink access on mismatch");
    f.unchanged();
}

#[test]
fn trusted_binding_rejects_unverifiable_session() {
    let mut f = Fixture::new();
    f.associate("child", None, Some("ancestor"));
    let mut host = Host::default();
    assert!(matches!(
        f.run("child", &mut host),
        Ok(StageResult::Refused) | Err(_)
    ));
    assert!(host.calls.is_empty(), "no filename fallback or sink access");
    f.unchanged();
}
