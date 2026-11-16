//! Reviewed PR tracking includes finished plans, but approval alone is not closure.
use super::review_plan::{finish_closed, open_plan_refs};
use super::team::{GhPr, GhState, Lookup, Review};
use crate::{home::CpdHome, store::Store};
use std::collections::BTreeMap;

fn fixture(phase: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store.write("fixture", |tx| {
        tx.execute_batch(&format!("INSERT OR IGNORE INTO contexts (id,title,created_at,created_on) VALUES ('codereview','Review','t','m');
        INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('l1','lead','codereview','r-pay-812','Lead','/','S3','idle','t','t');
        INSERT INTO plans (id,context,title,phase,lead_session,created_at,pr_subject,locked_at,done_at) VALUES ('r-pay-812','codereview','Review','{phase}','l1','t','pr:acme/pay#812', {}, {});", if phase == "done" { "'t'" } else { "NULL" }, if phase == "done" { "'t'" } else { "NULL" }))?;
        Ok::<(), crate::error::CpdError>(())
    }).unwrap();
    (dir, store)
}

#[test]
fn review_autoarchive_regression_done_plan_remains_tracked() {
    let (_dir, mut store) = fixture("done");
    let refs = open_plan_refs(&mut store).unwrap();
    assert_eq!(
        refs.len(),
        1,
        "done reviews must still be polled for PR closure"
    );
    assert_eq!((&*refs[0].1, refs[0].2), ("acme/pay", 812));
}

#[test]
fn review_autoarchive_regression_approval_is_not_closed() {
    let (_dir, mut store) = fixture("grounding");
    let pr = GhPr {
        title: "PR".into(),
        draft: false,
        state: GhState::Open,
        author: "other".into(),
        decision: Some("APPROVED".into()),
        reviews: vec![Review {
            author: "me".into(),
            state: "APPROVED".into(),
            commit_oid: None,
        }],
        head_oid: None,
        checks_state: None,
    };
    let lookup = Lookup {
        me: "me".into(),
        workflows: BTreeMap::from([(("acme/pay".into(), 812), Some(pr))]),
    };
    assert_eq!(finish_closed(&mut store, &lookup).unwrap().len(), 1);
    let archived: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT archived_at FROM plans WHERE id='r-pay-812'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(archived.is_none(), "approval alone must not archive");
}

#[test]
fn review_feedback_regression_done_approval_opens_no_write() {
    let (_dir, mut store) = fixture("done");
    let pr = GhPr {
        title: "PR".into(),
        draft: false,
        state: GhState::Open,
        author: "other".into(),
        decision: None,
        reviews: vec![Review {
            author: "me".into(),
            state: "APPROVED".into(),
            commit_oid: None,
        }],
        head_oid: None,
        checks_state: None,
    };
    let lookup = Lookup {
        me: "me".into(),
        workflows: BTreeMap::from([(("acme/pay".into(), 812), Some(pr))]),
    };
    let count = |store: &mut Store| -> i64 {
        store
            .read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
            .unwrap()
    };
    let before = count(&mut store);
    super::review_plan::finish_approved(&mut store, &lookup).unwrap();
    assert_eq!(count(&mut store), before);
    assert_eq!(open_plan_refs(&mut store).unwrap().len(), 1);
}
