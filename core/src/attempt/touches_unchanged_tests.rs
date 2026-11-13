//! Red tests for the `touches_unchanged` guard at `attempt submit` (bug b69, INC-2197): every declared `touches` file
//! must have a diff, else the submit is refused `touches_unchanged` (exit 3, Attempt stays `running`) naming the paths.
//! A directory entry needs one changed path under it. Explore and open Jobs are exempt. The Lead waives a path with a
//! required reason. The fixture is shared with `touches_tests`: in-process fake git, temp `$CPD_HOME`.

use super::submit::waive_untouched;
use super::touches_tests::{Spec, refused, rig};
use crate::error::ErrorCode;
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
