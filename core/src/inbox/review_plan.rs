//! The review Plan of `inbox claim` (commands.md §21.7, FR10): the pure parts, the Plan id candidates, the title and
//! the brief. No store, no I/O.

/// The longest Plan id (`plans.id`, contract §4.4).
const MAX_ID: usize = 24;
/// The longest review Plan title, in characters.
const MAX_TITLE: usize = 100;

/// The short names of the known repositories.
const KNOWN: [(&str, &str); 5] = [
    ("account-management-mobile-app", "mobile"),
    ("cc-api-orc-apollo", "orc"),
    ("card-dashboard", "cd"),
    ("snowflake-analytics", "sfa"),
    ("compass-ui", "cui"),
];

/// The slug of a repository name: a known short name, else the name lower-cased with every run of characters outside
/// `[a-z0-9]` turned into one `-`, trimmed of `-` at both ends. May be empty.
pub fn slug(repo_name: &str) -> String {
    if let Some((_, short)) = KNOWN.iter().find(|(name, _)| *name == repo_name) {
        return (*short).to_owned();
    }
    let mut out = String::new();
    for c in repo_name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// The `attempt`th candidate id (1 is the first): `r-<slug>-<number>`, then `-2`, `-3`, … The slug is cut from the
/// right, then trimmed of a trailing `-`, until the id fits 24 characters; the number and the suffix are never cut.
pub fn plan_id(slug: &str, number: u64, attempt: u32) -> String {
    let tail = if attempt <= 1 {
        format!("-{number}")
    } else {
        format!("-{number}-{attempt}")
    };
    let budget = MAX_ID.saturating_sub(2 + tail.len());
    let cut: String = slug.chars().take(budget).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() {
        format!("r{tail}")
    } else {
        format!("r-{cut}{tail}")
    }
}

/// `Review <slug>#<number> <PR title>`, the title with control characters turned into spaces and trimmed (just
/// `Review <slug>#<number>` when it is empty), the whole cut to 100 characters.
pub fn title(slug: &str, number: u64, pr_title: &str) -> String {
    let cleaned: String = pr_title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.trim();
    let full = if cleaned.is_empty() {
        format!("Review {slug}#{number}")
    } else {
        format!("Review {slug}#{number} {cleaned}")
    };
    full.chars().take(MAX_TITLE).collect()
}

/// What the brief names.
pub struct Brief<'a> {
    /// `<owner>/<repo>` of the item's subject.
    pub repo: &'a str,
    pub number: u64,
    /// The team review cache's channel and the post's `ts_raw`.
    pub channel: &'a str,
    pub ts_raw: &'a str,
    /// The User's home directory.
    pub home: &'a str,
}

impl Brief<'_> {
    /// The pull request's url, derived from the repo and the number.
    pub fn url(&self) -> String {
        format!("https://github.com/{}/pull/{}", self.repo, self.number)
    }

    /// The brief of commands.md §21.7, exact.
    pub fn text(&self) -> String {
        let (repo, number, channel, ts_raw, home) =
            (self.repo, self.number, self.channel, self.ts_raw, self.home);
        let url = self.url();
        let repo_name = repo.split_once('/').map_or(repo, |(_, name)| name);
        format!(
            "Review PR {url} (`{repo}#{number}`).
Slack post: `{channel}/{ts_raw}` (`--slack {channel}/{ts_raw}`).

Read `{home}/cpd-codereview/cpd-code-review/SKILL.md` (v10, `pro-d6`) first and follow it strictly:
1. Turn 1: run `~/cpd-codereview/cpd-code-review/prefetch.sh \"{url}\" \"/tmp/$CPD_PLAN\"`.
2. Never run `cpd context show --context codereview`; never message any Manager or Lead (`cpd session send` / `cpd board post`).
3. Use `{home}/projects/{repo_name}` (`git show`/`git grep`) and `H2` `scout` Jobs (`cpd job add --kind scout --tier H2`) if wide/cross-repo/multi-branch exploration is needed.
4. After the User answers the `Verdict` `ask_user_question`, run 1 bash call (`cpd review submit ... --slack {channel}/{ts_raw} && cpd context note ...`) and stop: after that call your turn is complete, so emit no post-submit report or message."
        )
    }
}

/// The open (phase `grounding`, unarchived) `codereview` Plans that have a `pr_subject`, with the pull request it names
/// (`owner/repo`, number). A subject that does not parse is skipped.
pub(crate) fn open_plan_refs(
    store: &mut crate::store::Store,
) -> Result<Vec<(crate::ids::PlanId, String, u64)>, crate::error::CpdError> {
    let rows: Vec<(String, String)> = store.read(|c| {
        let mut st = c.prepare(
            "SELECT id, pr_subject FROM plans WHERE context = 'codereview' AND phase = 'grounding'
               AND archived_at IS NULL AND deleted_at IS NULL AND pr_subject IS NOT NULL ORDER BY id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, subject)| {
            let (repo, number) = subject.strip_prefix("pr:")?.rsplit_once('#')?;
            Some((
                crate::ids::PlanId::parse(&id).ok()?,
                repo.to_owned(),
                number.parse().ok()?,
            ))
        })
        .collect())
}

/// Whether the pull request is over for the viewer: closed, merged, or the viewer's latest review is APPROVED.
fn is_over(pr: &super::team::GhPr, me: &str) -> bool {
    pr.state != super::team::GhState::Open
        || pr
            .reviews
            .iter()
            .rev()
            .find(|r| r.author == me)
            .is_some_and(|r| r.state == "APPROVED")
}

/// Finishes every open (phase `grounding`, unarchived) `codereview` Plan whose `pr_subject` names a pull request that
/// `lookup` reports CLOSED or MERGED, or APPROVED by the viewer (`lookup.me`) in their latest review (pro-d416, A3), the
/// way `review submit` finishes one: phase `done`, `locked_at`, `done_at` and one quiet `plan-done` Event. A Plan without
/// `pr_subject` is never finished. Returns the finished Plan ids.
pub(crate) fn finish_closed(
    store: &mut crate::store::Store,
    lookup: &super::team::Lookup,
) -> Result<Vec<crate::ids::PlanId>, crate::error::CpdError> {
    let over: Vec<crate::ids::PlanId> = open_plan_refs(store)?
        .into_iter()
        .filter(|(_, repo, number)| {
            lookup
                .workflows
                .get(&(repo.clone(), *number))
                .and_then(Option::as_ref)
                .is_some_and(|pr| is_over(pr, &lookup.me))
        })
        .map(|(id, _, _)| id)
        .collect();
    if over.is_empty() {
        return Ok(over);
    }
    store.write("review plan close", |tx| {
        over.iter()
            .try_for_each(|id| crate::pr::review::finish_plan(tx, id))
    })?;
    Ok(over)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::PlanId;

    #[test]
    fn known_names_have_short_slugs() {
        assert_eq!(slug("account-management-mobile-app"), "mobile");
        assert_eq!(slug("cc-api-orc-apollo"), "orc");
        assert_eq!(slug("card-dashboard"), "cd");
        assert_eq!(slug("snowflake-analytics"), "sfa");
        assert_eq!(slug("compass-ui"), "cui");
    }

    #[test]
    fn other_names_are_lowercased_and_collapsed() {
        assert_eq!(slug("Pay"), "pay");
        assert_eq!(slug("__My.Repo__x"), "my-repo-x");
        assert_eq!(slug("---"), "");
        assert_eq!(slug("é"), "");
    }

    #[test]
    fn the_id_is_r_slug_number_with_suffixes() {
        assert_eq!(plan_id("pay", 812, 1), "r-pay-812");
        assert_eq!(plan_id("pay", 812, 2), "r-pay-812-2");
        assert_eq!(plan_id("", 812, 1), "r-812");
        assert_eq!(plan_id("", 812, 3), "r-812-3");
    }

    #[test]
    fn the_slug_is_cut_to_fit_24_characters_and_trimmed() {
        let long = "a-very-long-repository-name-indeed";
        assert_eq!(plan_id(long, 123_456, 1), "r-a-very-long-rep-123456");
        assert_eq!(plan_id(long, 123_456, 2), "r-a-very-long-r-123456-2");
        // the cut lands on a `-`: it is trimmed
        assert_eq!(plan_id("abcdefgh-ijklmnop", 1, 1), "r-abcdefgh-ijklmnop-1");
        assert_eq!(
            plan_id("abcdefghijklmnopq-rstuv", 123, 1),
            "r-abcdefghijklmnopq-123"
        );
        for attempt in 1..30 {
            assert!(PlanId::parse(&plan_id(long, 12_345, attempt)).is_ok());
        }
    }

    #[test]
    fn the_title_cleans_the_pr_title_and_is_cut() {
        assert_eq!(
            title("pay", 812, "Pay the bills"),
            "Review pay#812 Pay the bills"
        );
        assert_eq!(title("pay", 812, "a\tb\nc "), "Review pay#812 a b c");
        assert_eq!(title("pay", 812, " \n "), "Review pay#812");
        let long = title("pay", 812, &"x".repeat(300));
        assert_eq!(long.chars().count(), 100);
        assert!(long.starts_with("Review pay#812 xxx"));
    }

    #[test]
    fn the_brief_is_exact() {
        let brief = Brief {
            repo: "acme/pay",
            number: 812,
            channel: "C1",
            ts_raw: "1.2",
            home: "/h",
        };
        assert_eq!(brief.url(), "https://github.com/acme/pay/pull/812");
        let text = brief.text();
        assert!(text.starts_with(
            "Review PR https://github.com/acme/pay/pull/812 (`acme/pay#812`).\nSlack post: `C1/1.2` (`--slack C1/1.2`).\n\nRead `/h/cpd-codereview/cpd-code-review/SKILL.md` (v10, `pro-d6`) first and follow it strictly:\n1. Turn 1: run `~/cpd-codereview/cpd-code-review/prefetch.sh \"https://github.com/acme/pay/pull/812\" \"/tmp/$CPD_PLAN\"`.\n"
        ));
        assert!(text.contains("3. Use `/h/projects/pay` (`git show`"));
        assert!(text.ends_with(
            "`cpd review submit ... --slack C1/1.2 && cpd context note ...`) and stop: after that call your turn is complete, so emit no post-submit report or message."
        ));
    }
}
