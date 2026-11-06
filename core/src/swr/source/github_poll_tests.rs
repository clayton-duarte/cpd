//! The `auto_merge` executor asks E5 for an immediate poll (`run/pr-poll-now`) after a successful enable, and after nothing else
//! (swr-github.md §9; decision pro-d394). `gh` is a recording `FakeRunner` route; the run dir is the rig's; no sleep.

use std::sync::Arc;

use serde_json::json;

use super::github::GithubSource;
use super::{Cx, ExecOutcome, Source};
use crate::clock::Clock;
use crate::config::Config;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::store::Store;
use crate::swr::outbox::{self, Coalesce, Op};

use super::github_tests::{Rig as PollRig, mentions, pr as gh_pr};
use crate::inbox::team::{Review, lookup_from_store, seed_cache};

const KEY: &str = "github:pr:acme/pay#801";

/// GitHub's side: the read says auto-merge is `allowed`, the mutation is refused with `refuse`, or `gh` fails when `fail`.
struct Gh {
    allowed: bool,
    refuse: bool,
    fail: bool,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let text = call.args.join(" ");
        if self.fail {
            return Reply::exit(1, "", "gh: HTTP 502\n");
        }
        if text.contains("enablePullRequestAutoMerge") {
            return if self.refuse {
                Reply::ok(&json!({"data": null, "errors": [{"message": "refused"}]}).to_string())
            } else {
                Reply::ok(&json!({"data": {"enablePullRequestAutoMerge": {"pullRequest": {"autoMergeRequest": {"enabledAt": "t"}}}}}).to_string())
            };
        }
        Reply::ok(&json!({"data": {"repository": {"autoMergeAllowed": self.allowed, "squashMergeAllowed": true,
            "mergeCommitAllowed": false, "rebaseMergeAllowed": false, "pullRequest": {"id": "PR_1", "autoMergeRequest": null}}}}).to_string())
    }
}

/// Executes one queued `auto_merge` against `gh`; whether `run/pr-poll-now` exists after it, and the outcome.
fn run(gh: Gh) -> (bool, ExecOutcome) {
    let rig = Rig::new();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(gh));
    let now = rig.fakes.clock.utc();
    // The rows the pass stored for the pull request and its repo: the executor reads them from the cache (swr-github.md §9).
    let fetched = crate::clock::timestamp(now);
    rig.sql(&format!(
        "INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 1, 1, 0, 0, 1, '{fetched}');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested)
           VALUES ('acme/pay', 801, 'https://github.com/acme/pay/pull/801', 'Add retries', 0, 'open', 'none', 'none', '{fetched}', '{fetched}', 0, 0);"
    ));
    let mut store = Store::open(&rig.home, 5000).unwrap();
    outbox::enqueue(
        &mut store,
        now,
        Op {
            system: "github".into(),
            target_key: KEY.into(),
            op_kind: "auto_merge".into(),
            vars: json!({"repo": "acme/pay", "number": 801}),
            patch: None,
            context: None,
        },
        Coalesce::LatestWins,
    )
    .unwrap();
    let m = outbox::claim_next(&mut store, now, "github")
        .unwrap()
        .unwrap();
    let config = Config::load(&rig.home).unwrap();
    let outcome = {
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    };
    (
        rig.home.run_dir().join(crate::pr::POLL_NOW).exists(),
        outcome,
    )
}

#[test]
fn gh_auto_merge_requests_a_poll_after_a_successful_enable() {
    let (polled, outcome) = run(Gh {
        allowed: true,
        refuse: false,
        fail: false,
    });
    assert_eq!(outcome, ExecOutcome::Done);
    assert!(polled, "an enabled auto-merge asks for a poll at once");
}

#[test]
fn gh_auto_merge_requests_no_poll_after_a_failed_enable() {
    for (gh, name) in [
        (
            Gh {
                allowed: true,
                refuse: false,
                fail: true,
            },
            "gh failure",
        ),
        (
            Gh {
                allowed: true,
                refuse: true,
                fail: false,
            },
            "refused",
        ),
        (
            Gh {
                allowed: false,
                refuse: false,
                fail: false,
            },
            "repo lacks auto-merge",
        ),
    ] {
        let (polled, _) = run(gh);
        assert!(!polled, "{name}: nothing changed on GitHub");
    }
}

// ---- decision pro-d453: Slack PRs stay polled, latestReviews, the real author, the Danger freeze --------------------------

const SLACK_CHANNEL: &str = "C1";

/// A Slack post of `acme/pay#number`, `age` seconds before the rig's clock.
fn slack_post(rig: &mut PollRig, number: i64, age: i64) {
    let ts = rig.t0.unix_timestamp() as f64 - age as f64;
    let post = json!({"repo": "acme/pay", "number": number, "ts": ts, "author": "Ryan",
                      "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    seed_cache(&mut rig.store, SLACK_CHANNEL, &[post]);
}

/// Whether `q` selects `...Heavy` for `pullRequest(number: n)`.
fn heavy_in(q: &str, n: i64) -> bool {
    q.contains(&format!("pullRequest(number: {n}) {{ ...Light ...Heavy"))
}

#[test]
fn gh_poll_keeps_polling_an_open_slack_pr_every_pass() {
    let mut rig = PollRig::new();
    let p = gh_pr(1);
    rig.seed(&p, false);
    rig.seed_detail(&p);
    rig.state().world.insert(("acme/pay".into(), 1), p);
    slack_post(&mut rig, 1, 100);
    for (i, secs) in [0, 10, 20].into_iter().enumerate() {
        rig.pass(secs).unwrap();
        let q = rig.graphql();
        assert!(
            mentions(&q[i], 1),
            "pass {i}: an open Slack PR the User neither authored nor was asked to review is read: {}",
            q[i]
        );
        assert_eq!(
            q[i].matches("pullRequest(number: 1)").count(),
            1,
            "once per document"
        );
    }
}

#[test]
fn gh_poll_stops_polling_a_slack_pr_once_it_is_merged_or_closed() {
    let mut rig = PollRig::new();
    let (mut merged, mut closed) = (gh_pr(1), gh_pr(2));
    for p in [&merged, &closed] {
        rig.seed(p, false);
        rig.seed_detail(p);
    }
    merged.state = "MERGED";
    closed.state = "CLOSED";
    rig.state().world.insert(("acme/pay".into(), 1), merged);
    rig.state().world.insert(("acme/pay".into(), 2), closed);
    let mut posts = vec![];
    for n in [1, 2] {
        let ts = rig.t0.unix_timestamp() as f64 - 100.0;
        posts.push(json!({"repo": "acme/pay", "number": n, "ts": ts, "author": "Ryan",
                          "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false}));
    }
    seed_cache(&mut rig.store, SLACK_CHANNEL, &posts);

    rig.pass(0).unwrap();
    let q = rig.graphql();
    assert!(
        mentions(&q[0], 1) && mentions(&q[0], 2),
        "while open on our side, both are read: {}",
        q[0]
    );
    assert_eq!(rig.pr_state(1).as_deref(), Some("merged"));
    assert_eq!(rig.pr_state(2).as_deref(), Some("closed"));
    rig.pass(10).unwrap();
    let q = rig.graphql();
    assert!(
        !mentions(&q[1], 1) && !mentions(&q[1], 2),
        "a merged or closed PR stays out: {}",
        q[1]
    );
}

#[test]
fn gh_poll_polls_a_slack_pr_outside_the_window_never() {
    let mut rig = PollRig::new();
    let (inside, outside) = (gh_pr(1), gh_pr(2));
    for p in [&inside, &outside] {
        rig.seed(p, false);
        rig.seed_detail(p);
        rig.state()
            .world
            .insert(("acme/pay".into(), p.number), p.clone());
    }
    let t = rig.t0.unix_timestamp() as f64;
    let posts = [
        json!({"repo": "acme/pay", "number": 1, "ts": t - 100.0, "author": "Ryan", "primary": true, "reactions": [],
               "claim": null, "eyes_me": null, "thread_tags_me": false}),
        json!({"repo": "acme/pay", "number": 2, "ts": t - 8.0 * 24.0 * 3600.0, "author": "Ryan", "primary": true,
               "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false}),
        // And a post of a PR with no row at all, also too old.
        json!({"repo": "acme/pay", "number": 3, "ts": t - 9.0 * 24.0 * 3600.0, "author": "Ryan", "primary": true,
               "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false}),
    ];
    seed_cache(&mut rig.store, SLACK_CHANNEL, &posts);
    for (i, secs) in [0, 10].into_iter().enumerate() {
        rig.pass(secs).unwrap();
        let q = rig.graphql();
        assert!(
            mentions(&q[i], 1),
            "the post inside the window is polled: {}",
            q[i]
        );
        assert!(
            !mentions(&q[i], 2) && !mentions(&q[i], 3),
            "a post outside the window is not: {}",
            q[i]
        );
    }
}

#[test]
fn gh_poll_light_node_carries_latest_reviews_with_the_commit_oid() {
    let mut rig = PollRig::new();
    let mut p = gh_pr(1);
    p.reviews = vec![("bob", "CHANGES_REQUESTED", "a0")];
    rig.state().light_only = true;
    rig.state().reviews = vec![p];
    rig.pass(0).unwrap();
    let q = &rig.graphql()[0];
    let light = q
        .split("fragment Light")
        .nth(1)
        .unwrap()
        .split("fragment Heavy")
        .next()
        .unwrap();
    assert!(
        light.contains("latestReviews(first: 20) { nodes { state author { __typename login } commit { oid } } }"),
        "the Light fragment selects latestReviews with the commit oid: {light}"
    );
    assert!(!light.contains("latestOpinionatedReviews"), "{light}");
    let got = lookup_from_store(&mut rig.store, &[("acme/pay".into(), 1)]).unwrap();
    let reviews = &got.workflows[&("acme/pay".to_owned(), 1)]
        .as_ref()
        .unwrap()
        .reviews;
    assert_eq!(
        reviews,
        &vec![Review {
            author: "bob".into(),
            state: "CHANGES_REQUESTED".into(),
            commit_oid: Some("a0".into())
        }],
        "no Heavy read happened, and the reader still has the reviewer's state and the reviewed commit"
    );
}

#[test]
fn gh_poll_stores_the_real_author_login_of_a_teammates_pr() {
    let mut rig = PollRig::new();
    let mut p = gh_pr(1);
    p.author = "ryan";
    rig.state().light_only = true;
    rig.state().reviews = vec![p];
    rig.pass(0).unwrap();
    let got = lookup_from_store(&mut rig.store, &[("acme/pay".into(), 1)]).unwrap();
    assert_eq!(
        got.workflows[&("acme/pay".to_owned(), 1)]
            .as_ref()
            .unwrap()
            .author,
        "ryan",
        "a teammate's PR keeps its author, never `someone`"
    );
}

/// A PR `n` of `ada` whose head has a failing Danger as the given kind.
fn danger(n: i64, status_context: bool) -> super::github_tests::Pr {
    let mut p = gh_pr(n);
    if status_context {
        p.contexts = vec![("Danger", "FAILURE")];
    } else {
        p.checks.push(("danger", "FAILURE"));
    }
    p
}

#[test]
fn gh_poll_excuses_a_failing_danger_status_context_under_the_label() {
    let mut rig = PollRig::new();
    rig.source.excused_labels = vec!["Danger-Freeze".into()];
    let (mut context, mut run) = (danger(1, true), danger(2, false));
    context.labels = vec!["danger-freeze"];
    run.labels = vec!["danger-freeze"];
    rig.state().authored = vec![context, run];
    rig.pass(0).unwrap();
    for n in [1, 2] {
        assert_eq!(
            rig.text(&format!(
                "SELECT checks FROM pull_requests WHERE number = {n}"
            ))
            .as_deref(),
            Some("passed"),
            "PR {n}: the failing Danger is excused under the label"
        );
    }
}

#[test]
fn gh_poll_does_not_excuse_danger_without_the_label() {
    let mut rig = PollRig::new();
    rig.source.excused_labels = vec!["danger-freeze".into()];
    let (bare, other) = (danger(1, true), {
        let mut p = danger(2, false);
        p.labels = vec!["not-the-freeze"];
        p
    });
    rig.state().authored = vec![bare, other];
    rig.pass(0).unwrap();
    for n in [1, 2] {
        assert_eq!(
            rig.text(&format!(
                "SELECT checks FROM pull_requests WHERE number = {n}"
            ))
            .as_deref(),
            Some("failed"),
            "PR {n}: a failing Danger without the excused label stays failed (pro-d447)"
        );
    }
}

#[test]
fn gh_poll_hydrates_an_excused_label_pr_whose_rollup_failed_once_per_head() {
    let mut rig = PollRig::new();
    rig.source.excused_labels = vec!["danger-freeze".into()];
    let (mut frozen, mut plain) = (danger(1, true), danger(2, true));
    frozen.labels = vec!["danger-freeze"];
    plain.labels = vec![];
    rig.state().light_only = true;
    // A teammate's PRs: in the review-requested search only, never the User's own.
    rig.state().reviews = vec![frozen.clone(), plain.clone()];
    rig.state().world.insert(("acme/pay".into(), 1), frozen);
    rig.state().world.insert(("acme/pay".into(), 2), plain);

    rig.pass(0).unwrap();
    assert!(
        !heavy_in(&rig.graphql()[0], 1),
        "pass 0 discovers it, light only"
    );
    rig.pass(10).unwrap();
    let q = rig.graphql();
    assert!(
        heavy_in(&q[1], 1),
        "pass 1 reads the contexts of the failed, excused-label PR: {}",
        q[1]
    );
    assert!(
        !heavy_in(&q[1], 2),
        "a failed PR without the label is not hydrated: {}",
        q[1]
    );
    assert_eq!(
        rig.text("SELECT checks FROM pull_requests WHERE number = 1")
            .as_deref(),
        Some("passed"),
        "the hydrate excused the Danger"
    );
    rig.pass(20).unwrap();
    assert!(
        !heavy_in(&rig.graphql()[2], 1),
        "once hydrated for its head it is not read again"
    );
    assert_eq!(rig.graphql().len(), 3, "one graphql call per pass");
}
