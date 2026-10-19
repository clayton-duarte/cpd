//! There is no `review` command: R19 removed the Job-level Reviewer
//! (commands.md §7, §20), so it is an unknown command like any other.

use crate::common;

use common::TestEnv;

#[test]
fn review_decide_is_an_unknown_command() {
    let env = TestEnv::new();
    for argv in [
        &["review", "decide", "api-schema-a1", "--approve"][..],
        &["review"],
    ] {
        let run = env.cpd(argv);
        assert_eq!(run.exit, 2, "cpd {argv:?}: {run:?}");
        assert_eq!(run.error()["code"], "usage", "cpd {argv:?}");
    }
}
