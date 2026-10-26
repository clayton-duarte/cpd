//! The surfaces of the feedback list in `plan show` (`feedback_open`) and in the `job-merged` text of a `work` Job of an `open` PR
//! (Plan fb-path, DESIGN item 4). The stored list is what the poll wrote into `pull_requests.feedback`: the tests insert it by SQL.

use crate::common;

use std::fs;

use serde_json::{Value, json};

use super::driver_open::{open_pr_live, submit_only, Live};
use super::plan_show::{fx, Pay};

/// One item of the stored list, with the ten keys of DESIGN item 1.
fn item(id: &str, kind: &str) -> Value {
    json!({"id": id, "kind": kind, "last": id, "author": "reviewer", "bot": false, "path": null, "outdated": false,
           "url": format!("https://github.com/acme/api/pull/7#{id}"), "excerpt": "Please change this."})
}

fn store_list(sql_target: &str, list: &Value) -> String {
    format!("UPDATE pull_requests SET feedback = '{list}' WHERE {sql_target};")
}

#[test]
fn plan_show_gives_each_pr_its_open_feedback_count() {
    let fx = fx("dispatch");
    fx.pr("one", "open", &[]);
    fx.env.sql(&store_list("number = 7", &json!([item("PRRT_1", "thread"), item("IC_1", "comment"), item("PRR_1", "review")])));
    fx.pr("two", "waiting", &[]);

    let show = fx.show();

    let prs = show["prs"].as_array().unwrap();
    let open = prs.iter().find(|p| p["key"] == "one").unwrap();
    assert_eq!(open["feedback_open"], json!(3), "{open}");
}

#[test]
fn plan_show_gives_null_for_a_pr_whose_feedback_was_never_read() {
    let fx = fx("dispatch");
    fx.pr("one", "open", &[]);
    fx.pr("two", "waiting", &[]);

    let show = fx.show();

    for pr in show["prs"].as_array().unwrap() {
        let object = pr.as_object().unwrap();
        assert!(object.contains_key("feedback_open"), "{pr}");
        assert_eq!(object["feedback_open"], Value::Null, "{pr}");
    }
}

/// A merged `work` Job `fix` of the open PR `repo` of the live Plan `pay`; returns its `job-merged` text.
fn merged_fix_text(live: &Live) -> String {
    let spec = live.fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "The review asked.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    let added = live.fx.cpd(&["job", "add", "fix", "--title", "Fix the review", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--pr", "repo", "--plan", "pay"]).ok();
    assert_eq!(added["job"]["status"], "queued");
    let (attempt, submitted) = live.run_job("fix", |live, id| {
        live.commit(&live.worktree(id), "fix.txt", "fixed\n", "fix");
        submit_only(live, id)
    });
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    live.value::<String>("SELECT text FROM events WHERE plan = 'pay' AND job = 'fix' AND kind = 'job-merged'")
}

#[test]
fn the_job_merged_text_of_a_work_job_names_the_open_review_comments_left() {
    let live = open_pr_live();
    live.fixture(&store_list("repo = 'acme/pay' AND number = 812", &json!([item("PRRT_1", "thread"), item("IC_1", "comment")])));

    let text = merged_fix_text(&live);

    assert!(
        text.ends_with("PR repo still has 2 open review comments: cpd pr view --pr repo, then cpd pr reply."),
        "{text:?}"
    );
}

#[test]
fn the_job_merged_text_says_nothing_when_no_item_is_open() {
    let live = open_pr_live();
    // Read, none open (`[]`), and then the same as never read (NULL): neither adds a hint. Only the `[]` list is stored here;
    // the never-read case is the default of the fixture. The stored list must exist (the migration) for the test to mean it.
    live.fixture(&store_list("repo = 'acme/pay' AND number = 812", &json!([])));

    let text = merged_fix_text(&live);

    assert!(!text.contains("open review comments") && !text.contains("cpd pr reply"), "{text:?}");
    let _ = common::pay_lookup;
}
