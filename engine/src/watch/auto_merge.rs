//! E18, the pending auto-merge requests (commands.md §9.8b; pro-d115, pro-d117; swr-github.md §9): a posted Workflow whose request
//! is `pending` (deferred because it was stacked, or not yet queued) becomes an `auto_merge` mutation on the outbox. The drain
//! (`GithubSource::execute`) calls GitHub, retries a failure with the outbox's own backoff, settles `pr_posts.auto_merge`, and a
//! refusal is the one `error` Event of a failed mutation. E18 itself never calls GitHub.

use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::pr::posts;
use cpd_core::swr::github::pr_key;
use cpd_core::swr::source::github_write::enqueue;

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E18) {
        engine.log.line("E18: holding: incident mode is on");
        return;
    }
    loop {
        let next = match engine.store.read(posts::next_pending) {
            Ok(Some(next)) => next,
            Ok(None) => {
                engine.clear(Step::E18, &About::Machine);
                return;
            }
            Err(e) => {
                engine.fail(Step::E18, About::Machine, Failure::from(&e));
                return;
            }
        };
        let queued = enqueue(
            &mut engine.store,
            engine.ports.clock.utc(),
            &pr_key(&next.repo, next.number),
            "auto_merge",
            serde_json::json!({"repo": next.repo, "number": next.number}),
            Some(serde_json::json!({"auto_merge": "requested"})),
        );
        match queued {
            Ok(id) => engine.log.line(&format!(
                "E18: queued auto-merge of {}#{} for Workflow {}/{} (mutation {id})",
                next.repo, next.number, next.plan, next.workflow
            )),
            Err(e) => {
                engine.fail(Step::E18, About::Machine, Failure::from(&e));
                return;
            }
        }
    }
}
