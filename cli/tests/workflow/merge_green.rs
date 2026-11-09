//! `pr merge-green` (commands.md §9.10a): the Manager's merge protocol. A pull request whose checks are not green is refused
//! before any `gh` write (`update-branch`, `merge`). `gh` is a mock.

use crate::common;

use common::fx::Fx;
use common::refused;
use cpd_core::home::CpdHome;

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn merge_green_refuses_when_checks_not_green() {
    let fx = Fx::with_ports();
    fx.fake_gh(cpd_core::host::runner::Reply::ok(r#"{"status":"ahead"}"#));
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    merge_state, authored, base_ref, default_branch, head_oid)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add the thing', 0, 'open', 'none', 'failed', 't', 't',
                   'clean', 1, 'main', 'main', '{SHA}');"
    ));
    refused(&fx.cpd(&["pr", "merge-green", REF]), "checks_not_green", 3);
    let writes: Vec<_> = fx
        .calls("gh")
        .into_iter()
        .filter(|c| {
            format!("{c:?}").contains("update-branch") || format!("{c:?}").contains("merge")
        })
        .collect();
    assert!(writes.is_empty(), "{writes:?}");
}
