//! Review submit records the validated PR without replacing an existing association.
use super::review::submit_plan_tracking_stub;
use crate::{home::CpdHome, ids::PlanId, store::Store};

#[test]
fn review_autoarchive_regression_submit_backfills_both_verdicts() {
    for verdict in ["APPROVE", "REQUEST_CHANGES"] {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store.write("fixture", |tx| {
            tx.execute_batch("INSERT OR IGNORE INTO contexts (id,title,created_at,created_on) VALUES ('codereview','Review','t','m');
              INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('l1','lead','codereview','review','Lead','/','S3','idle','t','t'); INSERT INTO plans (id,context,title,phase,lead_session,created_at,pr_subject) VALUES ('review','codereview','Review','grounding','l1','t',NULL);")?;
            Ok::<(), crate::error::CpdError>(())
        }).unwrap();
        let plan = PlanId::parse("review").unwrap();
        store
            .write("submit", |tx| {
                submit_plan_tracking_stub(tx, &plan, "acme/pay#812", verdict)
            })
            .unwrap();
        let subject: Option<String> = store
            .read(|c| {
                Ok(
                    c.query_row("SELECT pr_subject FROM plans WHERE id='review'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(subject.as_deref(), Some("pr:acme/pay#812"), "{verdict}");
        let error = store
            .write("submit conflict", |tx| {
                submit_plan_tracking_stub(tx, &plan, "acme/other#1", verdict)
            })
            .unwrap_err();
        assert!(error.message().contains("different pull request"));
        let subject: Option<String> = store
            .read(|c| {
                Ok(
                    c.query_row("SELECT pr_subject FROM plans WHERE id='review'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(
            subject.as_deref(),
            Some("pr:acme/pay#812"),
            "existing subject must not change"
        );
    }
}
