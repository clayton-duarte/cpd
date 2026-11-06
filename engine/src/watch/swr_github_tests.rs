//! The engine's readers of GitHub state (docs/architecture/swr-github.md §6, §9, §10): E8 reads the cache, not the team
//! memo or `gh`; E18's auto-merge is an outbox op; an inbox item does not flicker across a poll race. The ports are `Fakes`
//! (a `ManualClock` at 2026-01-01T00:00:00Z, a recording `FakeRunner`); no `gh`, no process, no sleep.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::testkit::Fx;
use super::{auto_merge, inbox};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ports::{Fakes, Ports};
use cpd_core::swr::cache;
use cpd_core::swr::outbox::{self, Coalesce, Op};

const KEY: &str = "github:pr:acme/pay#801";

/// `gh` answers an empty object to anything: a test asserts the record of calls, not the answer.
struct Gh;

impl Handler for Gh {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::ok("{}")
    }
}

fn fx(config: &str) -> (Fx, Fakes) {
    let mut fx = Fx::with_plan(config);
    let fakes = Fakes::new();
    let _ = (*fakes.runner).clone().route("gh", Arc::new(Gh));
    fx.engine.ports = fakes.ports.clone();
    (fx, fakes)
}

fn gh_calls(fakes: &Fakes) -> Vec<String> {
    fakes
        .runner
        .calls()
        .iter()
        .filter(|c| c.tool == "gh")
        .map(|c| c.args.join(" "))
        .collect()
}

/// The Plan's Workflow `repo` has its pull request `acme/pay#801`, open and authored, with `checks` on its row.
fn open_pull_request(fx: &mut Fx, checks: &str) {
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    authored, mergeable, merge_state, base_ref, default_branch, head_oid)
           VALUES ('acme/pay', 801, 'https://github.com/acme/pay/pull/801', 'T', 0, 'open', 'none', '{checks}', 'opened', 'opened',
                   1, 'mergeable', 'clean', 'main', 'main', 'h1');
         UPDATE workflows SET status = 'open', pr_number = 801, pr_url = 'https://github.com/acme/pay/pull/801',
                pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests WHERE number = 801)
          WHERE plan = 'pay' AND key = 'repo';"
    ));
}

fn items(fx: &mut Fx, kind: &str) -> Vec<(String, String)> {
    fx.engine
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT subject, label FROM inbox_items WHERE kind = ?1 AND closed_at IS NULL ORDER BY id",
            )?;
            Ok(stmt
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

#[test]
fn gh_inbox_review_requested_comes_from_swr_without_the_memo() {
    let (mut fx, fakes) =
        fx("[inbox]\nteam_channel = \"C02JU6C2Z1D\"\nslack_user_id = \"U0USER001\"\n");
    // The pull request the User is asked to review, and its primary post in the team channel, both in SQLite; no
    // `run/team-review.json` and no `gh`.
    let ts = fakes.ports.clock.utc().unix_timestamp() - 3600;
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    authored, review_requested, head_oid)
           VALUES ('acme/pay', 42, 'https://github.com/acme/pay/pull/42', 'Add retries', 0, 'open', 'none', 'passed',
                   'opened', 'opened', 0, 1, 'abc');
         INSERT INTO swr_slack_posts (channel, ts, repo, number, author_is_me, reply_count, reactions_json, eyes_me, thread_tags_me, fetched_at)
           VALUES ('C02JU6C2Z1D', '{ts}.000100', 'acme/pay', 42, 0, 0, '[]', 0, 0, 't');"
    ));

    inbox::run(&mut fx.engine);
    assert_eq!(
        items(&mut fx, "review-requested")
            .into_iter()
            .map(|(subject, _)| subject)
            .collect::<Vec<_>>(),
        ["pr:acme/pay#42"],
        "E8 opens the item from SQLite"
    );

    // The next tick sees the change at once: there is no five-minute memo of a GitHub answer.
    fx.fixture("UPDATE pull_requests SET review_requested = 0 WHERE number = 42;");
    inbox::run(&mut fx.engine);
    assert!(
        items(&mut fx, "review-requested").is_empty(),
        "the memo is gone"
    );
    assert_eq!(
        gh_calls(&fakes),
        Vec::<String>::new(),
        "E8 never calls GitHub"
    );
}

#[test]
fn gh_auto_merge_is_an_outbox_op_for_posted_prs() {
    let (mut fx, fakes) = fx("");
    open_pull_request(&mut fx, "passed");
    fx.fixture(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, auto_merge)
           VALUES ('pay', 'repo', 'C0POLICY1', '1767225600.000100', 'https://slack.test/p1', 't', 't', 'pending');",
    );

    Ports::scope(&fakes.ports, || auto_merge::run(&mut fx.engine));

    let rows: Vec<(String, String, String)> = fx
        .engine
        .store
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT system, target_key, op_kind FROM swr_mutations ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(
        rows,
        [("github".to_owned(), KEY.to_owned(), "auto_merge".to_owned())],
        "E18 enqueues the op; the outbox drain and its retry replace E18's own thread"
    );
    assert_eq!(
        gh_calls(&fakes),
        Vec::<String>::new(),
        "E18 itself never calls GitHub"
    );
}

#[test]
fn gh_inbox_ci_state_does_not_flicker_across_a_poll_race() {
    let (mut fx, fakes) = fx("");
    open_pull_request(&mut fx, "failed");
    let now = fakes.ports.clock.utc();
    let id = outbox::enqueue(
        &mut fx.engine.store,
        now,
        Op {
            system: "github".into(),
            target_key: KEY.into(),
            op_kind: "rerun".into(),
            vars: json!({"run_ids": [9]}),
            patch: Some(json!({"checks": "pending"})),
            context: None,
        },
        Coalesce::LatestWins,
    )
    .unwrap();
    outbox::claim_next(&mut fx.engine.store, now, "github")
        .unwrap()
        .unwrap();
    fakes.clock.advance(Duration::from_secs(10));
    let completed = fakes.ports.clock.utc();
    outbox::complete(&mut fx.engine.store, id, completed).unwrap();

    let label = |fx: &mut Fx| {
        inbox::run(&mut fx.engine);
        items(fx, "pr-ready")
            .into_iter()
            .map(|(_, label)| label.lines().next().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    };
    let mut seen = vec![label(&mut fx)];

    // A poll that started before the rerun completed commits the old GitHub data (checks still failed): the overlay stays.
    cache::commit(
        &mut fx.engine.store,
        KEY,
        "same",
        completed,
        completed - Duration::from_secs(2),
        |_| Ok(()),
    )
    .unwrap();
    seen.push(label(&mut fx));

    // A poll that started after the rerun: the raw row is the truth, and only now does the item change.
    fakes.clock.advance(Duration::from_secs(10));
    cache::commit(
        &mut fx.engine.store,
        KEY,
        "same",
        fakes.ports.clock.utc(),
        completed + Duration::from_secs(1),
        |_| Ok(()),
    )
    .unwrap();
    seen.push(label(&mut fx));

    assert_eq!(
        seen,
        [
            vec!["pay#801 checks running".to_owned()],
            vec!["pay#801 checks running".to_owned()],
            vec!["pay#801 failing".to_owned()],
        ],
        "the colour changes at the overlay's start and at the fence, never in between"
    );
}
