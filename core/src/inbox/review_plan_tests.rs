//! The review Plan's brief, the Verdict rule (commands.md §21.7, pro-d412).

use super::*;

#[test]
fn brief_tells_the_lead_to_open_the_verdict_with_the_pr_url() {
    let brief = Brief {
        repo: "acme/pay",
        number: 812,
        channel: "C02JU6C2Z1D",
        ts_raw: "1700000000.000100",
        home: "/home/u",
    };
    let url = "https://github.com/acme/pay/pull/812";
    assert!(
        brief.text().contains(&format!(
            "5. Begin the `Verdict` `ask_user_question` with the full PR URL `{url}`: the question's first words are `{url}`."
        )),
        "{}",
        brief.text()
    );
}
