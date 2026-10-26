//! The feedback items of a pull request (what a person said and nobody answered yet), derived from GitHub's
//! GraphQL answer. STUBS: the implementation Job fills these in; the signatures stay.

use serde::{Deserialize, Serialize};

use crate::error::CpdError;

/// The last line of every reply `cpd pr reply` posts.
pub const MARKER: &str = "<!-- cpd:reply -->";

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Thread,
    Comment,
    Review,
}

/// One open thing a person said on the pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub last: String,
    pub author: String,
    pub bot: bool,
    pub path: Option<String>,
    pub outdated: bool,
    pub url: String,
    pub excerpt: String,
}

/// One pull request as the comments read saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    pub repo: String,
    pub number: i64,
    pub updated_at: String,
    pub items: Vec<Item>,
}

/// The GraphQL text reading `prs` (`owner/name`, number pairs).
pub fn query(_prs: &[(&str, i64)]) -> String {
    String::new()
}

/// The reads in `answer`, alias `p<i>` for the i-th pair of `prs`.
pub fn parse(_prs: &[(&str, i64)], _answer: &str, _bots: &[String]) -> Result<Vec<Read>, CpdError> {
    Ok(Vec::new())
}

/// The items of `after` whose `last` is the `last` of no item of `before` (`None` counts as empty).
pub fn new_items<'a>(_before: Option<&[Item]>, _after: &'a [Item]) -> Vec<&'a Item> {
    Vec::new()
}
