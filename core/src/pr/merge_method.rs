//! The merge methods a repo allows and the one cpd merges with (commands.md §9.10): the first
//! allowed of squash, merge commit, rebase.

/// The three flags of `gh repo view --json squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct MergeMethods {
    #[serde(rename = "squashMergeAllowed")]
    pub squash: bool,
    #[serde(rename = "mergeCommitAllowed")]
    pub merge: bool,
    #[serde(rename = "rebaseMergeAllowed")]
    pub rebase: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Squash,
    Merge,
    Rebase,
}

impl Method {
    /// The `method` of `pr merge`'s output and the `gh pr merge` flag without its dashes.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Squash => "squash",
            Method::Merge => "merge",
            Method::Rebase => "rebase",
        }
    }

    /// GraphQL's `PullRequestMergeMethod`.
    pub fn graphql(self) -> &'static str {
        match self {
            Method::Squash => "SQUASH",
            Method::Merge => "MERGE",
            Method::Rebase => "REBASE",
        }
    }
}

impl MergeMethods {
    /// The first allowed of squash, merge, rebase; `None` when all are off.
    pub fn choose(&self) -> Option<Method> {
        if self.squash {
            Some(Method::Squash)
        } else if self.merge {
            Some(Method::Merge)
        } else if self.rebase {
            Some(Method::Rebase)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_allowed_of_squash_merge_rebase_is_chosen() {
        for bits in 0..8u8 {
            let m = MergeMethods { squash: bits & 4 != 0, merge: bits & 2 != 0, rebase: bits & 1 != 0 };
            let want = match bits {
                4..=7 => Some(Method::Squash),
                2 | 3 => Some(Method::Merge),
                1 => Some(Method::Rebase),
                _ => None,
            };
            assert_eq!(m.choose(), want, "{m:?}");
        }
    }

    #[test]
    fn a_method_has_a_flag_and_a_graphql_spelling() {
        let all = [(Method::Squash, "squash", "SQUASH"), (Method::Merge, "merge", "MERGE"), (Method::Rebase, "rebase", "REBASE")];
        for (m, flag, gql) in all {
            assert_eq!((m.as_str(), m.graphql()), (flag, gql));
        }
    }
}
