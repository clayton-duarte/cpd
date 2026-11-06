//! Guarantee (b) of docs/architecture/swr-github.md §10: the Review Lead is woken when the claimed pull request's cached head
//! moved since the claim and its cached checks are green, once per head. A "pass" here is one call of the wake after the
//! cache rows a pass would have committed are written. Lag model (swr.md, stale-while-revalidate): a moved PR shows its new
//! `head_oid` after pass N with its checks still `pending`; the fresh checks arrive in pass N+1, so the wake comes in N+1.
//! No `gh`, no process, no sleep.

use serde_json::Value;

use super::swr::wake_review_leads;
use super::testkit::Fx;

const URL: &str = "https://github.com/acme/pay/pull/801";

/// Plan `pay` reviews `acme/pay#801`, claimed at head `h1` (Lead `lead1`); the cache holds the PR at `h1`, `passed`.
fn fx() -> Fx {
    let mut fx = Fx::with_plan_in("grounding", "");
    fx.fixture(&format!(
        "UPDATE plans SET reviewed_repo = 'acme/pay', reviewed_number = 801, reviewed_title = 'T' WHERE id = 'pay';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    authored, mergeable, merge_state, base_ref, default_branch, head_oid)
           VALUES ('acme/pay', 801, '{URL}', 'T', 0, 'open', 'none', 'passed', 'opened', 'opened',
                   0, 'mergeable', 'clean', 'main', 'main', 'h1');
         INSERT INTO inbox_items (kind, urgency, subject, context, plan, label, opened_at, claimed_at, head_oid)
           VALUES ('review-requested', 'blue', 'acme/pay#801', 'ctx', 'pay', 'review', 't', 't', 'h1');"
    ));
    fx
}

/// What one pass commits for the PR: its head and its checks.
fn cache(fx: &mut Fx, head: &str, checks: &str) {
    fx.fixture(&format!(
        "UPDATE pull_requests SET head_oid = '{head}', checks = '{checks}' WHERE number = 801;"
    ));
}

fn pass(fx: &mut Fx) {
    wake_review_leads(&mut fx.engine);
}

fn wakes(fx: &mut Fx) -> Vec<Value> {
    fx.events("session-message")
        .into_iter()
        .filter(|(plan, ..)| plan.as_deref() == Some("pay"))
        .map(|(.., data)| data)
        .collect()
}

#[test]
fn gh_review_lead_is_woken_once_when_the_claimed_head_changes_and_checks_are_green() {
    let mut fx = fx();
    pass(&mut fx);
    assert!(wakes(&mut fx).is_empty(), "the head is the claimed one");

    // Pass N: the new head, its checks not read yet.
    cache(&mut fx, "h2", "pending");
    pass(&mut fx);
    assert!(wakes(&mut fx).is_empty(), "checks are not green in pass N");

    // Pass N+1: the checks of h2 arrive green.
    cache(&mut fx, "h2", "passed");
    pass(&mut fx);
    let sent = wakes(&mut fx);
    assert_eq!(sent.len(), 1, "one wake: {sent:?}");
    assert_eq!(sent[0]["head_oid"], "h2");
    assert_eq!(sent[0]["wake"], true);
    assert_eq!(sent[0]["from"], Value::Null);
    let text = sent[0]["text"].as_str().unwrap();
    for needle in [URL, "h2", "h1"] {
        assert!(text.contains(needle), "{needle} is in {text:?}");
    }
}

#[test]
fn gh_review_lead_is_not_woken_while_checks_are_pending_or_failed() {
    let mut fx = fx();
    cache(&mut fx, "h2", "pending");
    pass(&mut fx);
    cache(&mut fx, "h2", "failed");
    pass(&mut fx);
    cache(&mut fx, "h3", "pending");
    pass(&mut fx);
    assert!(wakes(&mut fx).is_empty(), "{:?}", wakes(&mut fx));
    // Green on the claimed head itself is no change either.
    cache(&mut fx, "h1", "passed");
    pass(&mut fx);
    assert!(wakes(&mut fx).is_empty(), "{:?}", wakes(&mut fx));
}

#[test]
fn gh_review_lead_is_not_woken_twice_for_the_same_head() {
    let mut fx = fx();
    cache(&mut fx, "h2", "passed");
    pass(&mut fx);
    pass(&mut fx);
    assert_eq!(wakes(&mut fx).len(), 1, "a second pass on h2 wakes nothing");

    // Checks flap on the same head: still one wake for h2.
    cache(&mut fx, "h2", "failed");
    pass(&mut fx);
    cache(&mut fx, "h2", "passed");
    pass(&mut fx);
    assert_eq!(
        wakes(&mut fx).len(),
        1,
        "checks changing on h2 wake nothing"
    );

    // A newer green head is a new key.
    cache(&mut fx, "h3", "passed");
    pass(&mut fx);
    let heads: Vec<_> = wakes(&mut fx)
        .iter()
        .map(|d| d["head_oid"].clone())
        .collect();
    assert_eq!(heads, ["h2", "h3"]);
}
