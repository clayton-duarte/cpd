//! Red tests for the `touches_unchanged` guard at `attempt submit` (bug b69, INC-2197): every declared `touches` file
//! must have a diff, else the submit is refused `touches_unchanged` (exit 3, Attempt stays `running`) naming the paths.
//! A directory entry needs one changed path under it. Explore and open Jobs are exempt. The Lead waives a path with a
//! required reason. The fixture is shared with `touches_tests`: in-process fake git, temp `$CPD_HOME`.

use super::touches_tests::{Spec, refused, rig};
use crate::error::ErrorCode;
use crate::job::approve::waive_untouched;
use serde_json::json;

const TWO: &str = "[\"docs/research/x.md\", \"src/app.rs\"]";

#[test]
fn declared_file_without_diff_is_refused() {
    let mut rig = rig(Spec {
        touches: TWO,
        changed: &[("src/app.rs", "// changed\n")],
        ..Spec::default()
    });
    let e = refused(rig.submit(), ErrorCode::TouchesUnchanged);
    assert_eq!(
        e.details()["paths"],
        json!(["docs/research/x.md"]),
        "{:?}",
        e.details()
    );
    assert!(e.message().contains("docs/research/x.md"), "{e:?}");
    assert_eq!(rig.attempt("status").as_deref(), Some("running"));
}

#[test]
fn all_declared_files_changed_is_accepted() {
    let changed = [
        ("docs/research/x.md", "# new\n"),
        ("src/app.rs", "// changed\n"),
    ];
    let mut ok = rig(Spec {
        touches: TWO,
        changed: &changed,
        ..Spec::default()
    });
    ok.submit().expect("every declared file has a diff");
    // Contrast: the same Job with one file untouched is refused.
    let mut bad = rig(Spec {
        touches: TWO,
        changed: &changed[1..],
        ..Spec::default()
    });
    refused(bad.submit(), ErrorCode::TouchesUnchanged);
}

#[test]
fn directory_prefix_touch_matching_any_change_is_accepted() {
    let mut ok = rig(Spec {
        touches: "[\"docs/research/\"]",
        changed: &[("docs/research/y.md", "# y\n")],
        ..Spec::default()
    });
    ok.submit()
        .expect("a changed path sits under the directory");
    let mut bad = rig(Spec {
        touches: "[\"docs/research/\"]",
        changed: &[],
        ..Spec::default()
    });
    refused(bad.submit(), ErrorCode::TouchesUnchanged);
}

#[test]
fn explore_and_open_jobs_are_exempt() {
    for kind in ["update", "open"] {
        let mut r = rig(Spec {
            kind,
            ..Spec::default()
        });
        r.submit()
            .unwrap_or_else(|e| panic!("{kind} Job is exempt: {e:?}"));
    }
    let mut work = rig(Spec::default());
    refused(work.submit(), ErrorCode::TouchesUnchanged);
}

#[test]
fn waived_path_is_accepted_with_reason() {
    let mut r = rig(Spec::default());
    let plan = r.plan.clone();
    refused(r.submit(), ErrorCode::TouchesUnchanged);
    waive_untouched(
        &mut r.env,
        &plan,
        "a",
        "docs/research/x.md",
        "already correct",
    )
    .unwrap();
    r.submit().expect("the waived path is accepted");
}

#[test]
fn waiver_requires_a_reason() {
    let mut r = rig(Spec::default());
    let plan = r.plan.clone();
    let e = waive_untouched(&mut r.env, &plan, "a", "docs/research/x.md", "  ")
        .expect_err("an empty reason is refused");
    assert_eq!(e.code(), ErrorCode::Usage, "{e:?}");
    refused(r.submit(), ErrorCode::TouchesUnchanged);
}

/// `cpd job approve-untouched a --path … --reason … --plan <id>`, parsed as the CLI parses it, run on the rig's env.
#[test]
fn approve_untouched_command_reaches_the_waiver() {
    use clap::Parser;
    #[derive(Parser)]
    struct Argv {
        #[command(flatten)]
        args: crate::job::ApproveTouchesArgs,
    }
    // SAFETY: nextest gives each test a process of its own.
    unsafe { std::env::remove_var("CPD_ATTEMPT") };
    let mut r = rig(Spec::default());
    refused(r.submit(), ErrorCode::TouchesUnchanged);
    let argv = Argv::try_parse_from([
        "x",
        "a",
        "--path",
        "docs/research/x.md",
        "--reason",
        "already correct",
        "--plan",
        r.plan.as_str(),
    ])
    .unwrap();
    crate::job::approve::approve_untouched_in(&mut r.env, argv.args).unwrap();
    r.submit().expect("the waived path is accepted");
}

/// A Job that only merges the base is not credited with the paths the merge brought: its declared touch stays unchanged.
#[test]
fn a_base_merge_does_not_satisfy_a_declared_touch() {
    let mut r = rig(Spec {
        touches: "[\"src/from_base.rs\"]",
        changed: &[],
        base_ahead: &[("src/from_base.rs", "// from the base\n")],
        brings_base: true,
        ..Spec::default()
    });
    let e = refused(r.submit(), ErrorCode::TouchesUnchanged);
    assert_eq!(e.details()["paths"], json!(["src/from_base.rs"]));
}

/// A Job that deliberately reverts an earlier Job's file to the base branch's version (it deletes a file the Lead
/// Branch added and `main` lacks) has changed the declared path: it is accepted without a waiver.
#[test]
fn deleting_a_file_an_earlier_job_added_satisfies_the_touch() {
    let mut r = rig(Spec {
        touches: "[\"declared.txt\"]",
        lead_only: &[("declared.txt", "earlier\n")],
        deleted: &["declared.txt"],
        ..Spec::default()
    });
    r.submit().expect("the reversion is the Job's own change");
}

/// A waiver is its own field: it never authorises a write outside the touches, even a waived directory.
#[test]
fn a_waiver_does_not_authorise_an_out_of_scope_write() {
    let mut r = rig(Spec {
        changed: &[
            ("docs/research/x.md", "# Findings\n\nEdited.\n"),
            ("src/app.rs", "// stray\n"),
        ],
        ..Spec::default()
    });
    let plan = r.plan.clone();
    waive_untouched(&mut r.env, &plan, "a", "src/", "narrowed later").unwrap();
    assert_eq!(
        r.job("touches_approved"),
        None,
        "the waiver is not an approval"
    );
    let e = refused(r.submit(), ErrorCode::TouchesViolation);
    assert_eq!(e.details()["paths"], json!(["src/app.rs"]));
}
