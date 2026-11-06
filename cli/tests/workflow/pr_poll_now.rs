//! A cpd command that changes a pull request on GitHub asks E5 for a poll at once (decision pro-d394, assumption A6): it writes
//! the run file `run/pr-poll-now` after its successful GitHub write, and not after a failed one (no floor, no migration).
//! `pr merge` and `pr rerun` are in `core/src/pr/poll_now_tests.rs`; here are `pr push`, `pr rebase`, `pr open` (the opener's
//! merge, which creates the pull request), and, through the outbox drain that makes their GitHub write, `pr post` (its
//! auto-merge) and `review submit`, in process on the same fixtures as their own tests. `gh`, git and the Gate's `sh` are
//! fakes; no process, no sleep.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::pr_view::{Pay, fx as pay};
use super::workflow::{poll_fx, rebase_fx};
use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::merge::End;
use serde_json::{Value, json};

/// `run/pr-poll-now` of the home under test.
fn file(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("run/pr-poll-now")
}

/// Runs the queued mutations through the source, as the engine's drain does.
fn drain(fx: &Fx) {
    use cpd_core::config::Config;
    use cpd_core::swr::outbox;
    use cpd_core::swr::source::github::GithubSource;
    use cpd_core::swr::source::{Cx, Source};
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = fx.env.store();
    let config = Config::load(&home).unwrap();
    let now = cpd_core::clock::Clock::utc(&*fx.fakes.ports.clock);
    while let Some(m) = outbox::claim_next(&mut store, now, "github").unwrap() {
        let mut cx = Cx {
            store: &mut store,
            ports: &fx.fakes.ports,
            home: &home,
            config: &config,
        };
        let _ = GithubSource::default().execute(&mut cx, &m);
    }
}

/// The bare `origin` of the poll fixture (`workflow::poll_fx`).
fn origin(fx: &Fx) -> String {
    fx.env.root().join("repos/origin.git").display().to_string()
}

// ---------------------------------------------------------------------------------------------------------------- push

#[test]
fn pr_commands_write_poll_now_file_after_github_write_push() {
    let fx = rebase_fx("deploy");
    fx.env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    fx.commit_file(&fx.lead, "more.txt", "more\n", "more");
    assert!(!file(&fx).exists());

    let out = fx.pr("push").ok();

    assert_eq!(out["pushed"], true, "{out}");
    assert!(
        file(&fx).exists(),
        "a push to the pull request's branch asks for a poll"
    );
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_push() {
    let fx = rebase_fx("deploy");
    fx.env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    let clone = fx.env.root().join("repos/clone");
    fx.git(
        fx.env.root(),
        &["clone", "-q", &origin(&fx), clone.to_str().unwrap()],
    );
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fx.commit_file(&clone, "theirs.txt", "theirs\n", "theirs");
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
    fx.commit_file(&fx.lead, "mine.txt", "mine\n", "mine");

    let run = fx.pr("push");

    assert_eq!(run.error()["code"], "push_rejected");
    assert!(!file(&fx).exists(), "nothing was pushed");
}

// -------------------------------------------------------------------------------------------------------------- rebase

#[test]
fn pr_commands_write_poll_now_file_after_github_write_rebase() {
    let fx = rebase_fx("deploy");
    fx.advance_base("base.txt", "base\n");
    assert!(!file(&fx).exists());

    let out = fx.pr("rebase").ok();

    assert_eq!(out["pushed"], true, "{out}");
    assert!(
        file(&fx).exists(),
        "the rebased branch was pushed: a poll is asked for"
    );
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_rebase_push() {
    let fx = rebase_fx("deploy");
    let clone = fx.env.root().join("repos/clone");
    fx.git(
        fx.env.root(),
        &["clone", "-q", &origin(&fx), clone.to_str().unwrap()],
    );
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fx.commit_file(&clone, "theirs.txt", "theirs\n", "theirs");
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);

    let run = fx.pr("rebase");

    assert_eq!(run.error()["code"], "git_failed");
    assert!(!file(&fx).exists(), "the push was refused");
}

// ----------------------------------------------------------------------------------------------------------------- open

#[test]
fn pr_commands_write_poll_now_file_after_github_write_open() {
    let fx = poll_fx("dispatch");
    let (attempt, _, tip) = fx.submitted_opener();
    assert!(!file(&fx).exists());

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert!(
        file(&fx).exists(),
        "`gh pr create` ran: a poll is asked for"
    );
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_open() {
    let fx = poll_fx("dispatch");
    fx.fail("gh: HTTP 401: Bad credentials");
    let (attempt, _, _) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert!(matches!(end, End::Failed { .. }), "{end:?}");
    assert!(!file(&fx).exists(), "nothing was created on GitHub");
}

// ------------------------------------------------------------------------------------------------------------------ post

const OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0POLICY1/p1793040197490209"}"#;

/// Plan `pay`'s one open PR, ready to post, the Slack helper answering [`OK`] (the baseline of `pr_post_auto_merge.rs`).
fn post_fx() -> Pay {
    let pay = pay("open");
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    env.repo_policy(
        "acme",
        "owner",
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n",
    );
    env.repo_policy("acme", "pay", "");
    pay.store()
        .write("fixture", |tx| {
            tx.execute_batch(
                "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
                 UPDATE workflows SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
                 UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]'
                   WHERE number = 812;",
            )
            .map_err(Into::into)
        })
        .unwrap();
    pay.fx.fake_node(Reply::ok(&format!("{OK}\n")));
    pay
}

fn post(pay: &Pay) -> common::Run {
    pay.fx
        .cpd(&["pr", "post", "--workflow", "repo", "--plan", "pay"])
}

#[test]
fn pr_commands_write_poll_now_file_after_github_write_post() {
    let pay = post_fx();
    assert!(!file(&pay.fx).exists());

    let out = post(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "queued", "{out}");
    assert!(!file(&pay.fx).exists(), "the command itself wrote nothing on GitHub");
    drain(&pay.fx);
    assert!(
        file(&pay.fx).exists(),
        "auto-merge was enabled on GitHub: a poll is asked for"
    );
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_post_write() {
    let pay = post_fx();
    pay.gh.auto.lock().unwrap().fail_mutation = true;

    let out = post(&pay).ok();

    assert_eq!(out["auto_merge"]["state"], "queued", "{out}");
    drain(&pay.fx);
    assert!(
        !file(&pay.fx).exists(),
        "the mutation failed: GitHub did not change"
    );
}

// ---------------------------------------------------------------------------------------------------------------- review

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

/// The in-process `gh` of `review submit`'s drain: an `api` call with `POST` answers `post`; any other call exits 9.
struct ReviewGh {
    post: Reply,
}

impl Handler for ReviewGh {
    fn handle(&self, call: &Call) -> Reply {
        match call.args.first().map(String::as_str) {
            Some("api") if call.args.iter().any(|a| a == "POST") => self.post.clone(),
            _ => Reply::exit(9, "", ""),
        }
    }
}

fn submit(post: Reply) -> (Fx, Value) {
    let fx = Fx::with_ports();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, head_oid)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'T', 0, 'open', 'none', 'passed', 't', 't', '{HEAD}')"
    ));
    let _ = (*fx.fakes.runner)
        .clone()
        .route("gh", Arc::new(ReviewGh { post }));
    let input = fx.env.cpd_home().join("review.json");
    fs::write(
        &input,
        json!({"body": "Looks right.", "comments": []}).to_string(),
    )
    .unwrap();
    let run = fx.cpd(&[
        "review",
        "submit",
        "--pr",
        "acme/api#7",
        "--head",
        HEAD,
        "--event",
        "APPROVE",
        "--input",
        input.to_str().unwrap(),
    ]);
    let out = if run.exit == 0 { run.ok() } else { run.error() };
    assert!(!file(&fx).exists(), "the command itself wrote nothing on GitHub");
    drain(&fx);
    (fx, out)
}

#[test]
fn pr_commands_write_poll_now_file_after_github_write_review() {
    let answer = json!({"id": 4242, "html_url": "https://github.com/acme/api/pull/7#pullrequestreview-4242", "state": "APPROVED"});
    let (fx, out) = submit(Reply::ok(&answer.to_string()));

    assert_eq!(out["ok"], true, "{out}");
    assert!(
        file(&fx).exists(),
        "a review changes the decision: a poll is asked for"
    );
}

#[test]
fn pr_commands_write_no_poll_now_file_after_a_failed_review() {
    let (fx, out) = submit(Reply::exit(1, "", "gh: HTTP 422\n"));

    assert_eq!(out["queued"], true, "{out}");
    assert!(!file(&fx).exists(), "the review was not posted");
}
