//! The closed-PR fast path must use the safe archive cascade, not the timers.
use super::testkit::Fx;

fn review_tick(fx: &mut Fx) {
    super::archive_leads::run(&mut fx.engine);
}

fn fixture(state: &str) -> Fx {
    let mut fx = Fx::new("");
    fx.on_fake_tmux();
    fx.fixture(&format!("INSERT OR IGNORE INTO contexts (id,title,owner,created_at,created_on) VALUES ('codereview','Review','acme','t','m1');
    INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('review-lead','lead','codereview','review','Lead','/','O3','idle','t','t');
    INSERT INTO plans (id,context,title,phase,lead_session,created_at,locked_at,done_at,pr_subject) VALUES ('review','codereview','Review','done','review-lead','t','t','t','pr:acme/pay#812');
    INSERT INTO pull_requests (repo,number,url,title,draft,state,review_decision,checks,created_at,changed_at,authored,review_requested,merged_at) VALUES ('acme/pay',812,'https://github.com/acme/pay/pull/812','PR',0,'{state}','none','none','t','t',0,1,CASE WHEN '{state}'='merged' THEN 't' END);"));
    fx
}
fn archived(fx: &mut Fx) -> Option<String> {
    fx.query("SELECT archived_at FROM plans WHERE id='review'")
}
#[test]
fn review_autoarchive_regression_closed_and_merged_archive_without_delay() {
    for state in ["closed", "merged"] {
        let mut fx = fixture(state);
        fx.fixture("INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('normal-lead','lead','ctx','normal','Lead','/','O3','idle','t','t'); INSERT INTO plans (id,context,title,phase,lead_session,created_at,locked_at,done_at) VALUES ('normal','ctx','Normal','done','normal-lead','t','t','t');");
        super::archive_leads::run(&mut fx.engine);
        let normal: Option<String> = fx.query("SELECT archived_at FROM plans WHERE id='normal'");
        assert!(
            normal.is_none(),
            "non-codereview plans retain normal delays"
        );
        review_tick(&mut fx);
        assert!(
            archived(&mut fx).is_some(),
            "{state} should archive immediately"
        );
        let lead: Option<String> =
            fx.query("SELECT archived_at FROM sessions WHERE id='review-lead'");
        assert!(lead.is_some(), "archive must cascade to the lead");
        fx.fixture("UPDATE pull_requests SET state='open', merged_at=NULL WHERE repo='acme/pay' AND number=812;");
        review_tick(&mut fx);
        assert!(archived(&mut fx).is_some(), "reopening must not unarchive");
    }
}
#[test]
fn review_autoarchive_regression_working_and_blocked_defer_until_safe() {
    for state in ["working", "blocked"] {
        let mut fx = fixture("closed");
        fx.fixture(&format!(
            "UPDATE sessions SET state='{state}', state_label=CASE WHEN '{state}'='blocked' THEN 'blocked' ELSE NULL END WHERE id='review-lead';"
        ));
        review_tick(&mut fx);
        assert!(archived(&mut fx).is_none(), "{state} must defer");
        fx.fixture("UPDATE sessions SET state='idle', state_label=NULL WHERE id='review-lead';");
        review_tick(&mut fx);
        assert!(archived(&mut fx).is_some(), "safe later tick must archive");
    }
}

#[test]
fn review_autoarchive_regression_inbox_and_chat_defer_until_safe() {
    for obstacle in ["inbox", "chat"] {
        let mut fx = fixture("merged");
        if obstacle == "inbox" {
            fx.fixture("INSERT INTO inbox_items (kind,urgency,subject,context,plan,session,label,opened_at) VALUES ('question','yellow','Q','codereview','review','review-lead','Q','t');");
        } else {
            fx.fixture("INSERT INTO events (txn,at,kind,context,session,text,data) VALUES (cpd_txn(),'t','session-moved','codereview','review-lead','x','{\"from\":\"none\",\"place\":\"chat\"}');");
        }
        review_tick(&mut fx);
        assert!(archived(&mut fx).is_none(), "{obstacle} must defer");
        if obstacle == "inbox" {
            fx.fixture("UPDATE inbox_items SET closed_at='t' WHERE session='review-lead';");
        } else {
            fx.fixture("INSERT INTO events (txn,at,kind,context,session,text,data) VALUES (cpd_txn(),'t','session-moved','codereview','review-lead','x','{\"from\":\"chat\",\"place\":\"parked\"}');");
        }
        review_tick(&mut fx);
        assert!(
            archived(&mut fx).is_some(),
            "{obstacle} cleared: retry must archive"
        );
    }
}

#[test]
fn review_feedback_regression_bad_phase_does_not_starve_other_plan() {
    let mut fx = fixture("closed");
    fx.fixture("INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('bad-lead','lead','codereview','a-bad','Lead','/','O3','idle','t','t'); INSERT INTO plans (id,context,title,phase,lead_session,created_at,pr_subject) VALUES ('a-bad','codereview','Bad','plan','bad-lead','t','pr:acme/pay#812');");
    review_tick(&mut fx);
    assert!(archived(&mut fx).is_some());
    let phase: String = fx.query("SELECT phase FROM plans WHERE id='a-bad'");
    assert_eq!(phase, "plan");
}
