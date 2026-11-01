//! Argument types shared by several areas (commands.md §1.3, §1.5).
//! An area flattens these into its own clap structs:
//!
//! ```text
//! #[derive(clap::Args)]
//! pub struct ShowArgs {
//!     #[command(flatten)]
//!     plan: PlanArg,
//! }
//! // …
//! let plan = args.plan.require()?;
//! ```

use std::fmt;
use std::str::FromStr;

use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, WorkflowKey, SessionId};
use crate::plan::WorkflowRow;

/// `--plan <id>`, else `CPD_PLAN`, else `plan_required` (exit 2).
#[derive(clap::Args, Clone, Debug, PartialEq)]
pub struct PlanArg {
    /// The Plan [default: $CPD_PLAN]
    #[arg(long = "plan", value_name = "id", env = "CPD_PLAN", hide_env = true)]
    pub plan: Option<PlanId>,
}

impl PlanArg {
    pub fn require(&self) -> Result<&PlanId, CpdError> {
        self.plan.as_ref().ok_or_else(|| {
            CpdError::new(ErrorCode::PlanRequired, "No Plan selected: pass --plan <id> or set CPD_PLAN.")
        })
    }
}

/// `--pr <key>`, selected within a Plan's PRs (commands.md §1.3).
#[derive(clap::Args, Clone, Debug, PartialEq)]
pub struct WorkflowArg {
    /// The PR [default: the Plan's one PR that is not cancelled]
    #[arg(long = "pr", value_name = "key")]
    pub pr: Option<WorkflowKey>,
}

impl WorkflowArg {
    /// The PR among `prs` (the Plan's, `cancelled` ones included): the one
    /// named, else the Plan's only PR that is not `cancelled`.
    /// A key that is not a PR of the Plan is `pr_not_found` (exit 4); without
    /// `--pr`, zero or two or more PRs that are not `cancelled` is
    /// `pr_required` (exit 2).
    pub fn select<'a>(&self, plan: &PlanId, prs: &'a [WorkflowRow]) -> Result<&'a WorkflowRow, CpdError> {
        if let Some(key) = &self.workflow {
            return workflows.iter().find(|p| p.key == *key).ok_or_else(|| {
                CpdError::new(ErrorCode::WorkflowNotFound, format!("No PR {key} in Plan {plan}."))
                    .with("plan", plan.as_str())
                    .with("pr", key.as_str())
            });
        }
        let live: Vec<&WorkflowRow> = workflows.iter().filter(|p| p.status != "cancelled").collect();
        match live.as_slice() {
            [only] => Ok(only),
            [] => Err(CpdError::new(
                ErrorCode::WorkflowRequired,
                format!("Plan {plan} has no PR: add one with cpd pr add, then pass --pr <key>."),
            )
            .with("plan", plan.as_str())
            .with("prs", Vec::<String>::new())),
            many => {
                let keys: Vec<&str> = many.iter().map(|p| p.key.as_str()).collect();
                Err(CpdError::new(
                    ErrorCode::WorkflowRequired,
                    format!("Plan {plan} has {} PRs ({}): pass --pr <key>.", keys.len(), keys.join(", ")),
                )
                .with("plan", plan.as_str())
                .with("prs", keys))
            }
        }
    }
}

/// `--session <id>`, else `CPD_SESSION`, else `session_required` (exit 2).
#[derive(clap::Args, Clone, Debug, PartialEq)]
pub struct SessionArg {
    /// The Session [default: $CPD_SESSION]
    #[arg(long = "session", value_name = "id", env = "CPD_SESSION", hide_env = true)]
    pub session: Option<SessionRef>,
}

impl SessionArg {
    pub fn require(&self) -> Result<&SessionRef, CpdError> {
        self.session.as_ref().ok_or_else(|| {
            CpdError::new(
                ErrorCode::SessionRequired,
                "No Session selected: pass --session <id> or set CPD_SESSION.",
            )
        })
    }
}

/// A Session as typed (commands.md §1.3, hs-a3): a Session id, a Plan id, a
/// bare id prefix, or `<plan>·<prefix>` / `<plan>.<prefix>`. It is only
/// syntactically valid; `session::resolve` turns it into a [`SessionId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRef {
    /// A full id, a Plan id or a bare prefix: all match the pattern of a Session id.
    Plain(SessionId),
    /// `<plan>·<prefix>` or `<plan>.<prefix>`, split at the first separator.
    Scoped { plan: PlanId, prefix: SessionId },
}

impl FromStr for SessionRef {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<SessionRef, CpdError> {
        let Some((plan, prefix)) = s.split_once(['·', '.']) else {
            return SessionId::parse(s).map(SessionRef::Plain);
        };
        match (PlanId::parse(plan), SessionId::parse(prefix)) {
            (Ok(plan), Ok(prefix)) => Ok(SessionRef::Scoped { plan, prefix }),
            // Neither separator is in a Session id's pattern: this is `invalid_session_id`.
            _ => Err(SessionId::parse(s).expect_err("a value with a separator is no Session id")),
        }
    }
}

impl fmt::Display for SessionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionRef::Plain(id) => write!(f, "{id}"),
            SessionRef::Scoped { plan, prefix } => write!(f, "{plan}.{prefix}"),
        }
    }
}

/// Free text (`--title`, `--reason`, record text, questions, answers):
/// 1 to 20 000 characters, no NUL; else `usage` (commands.md §1.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text(String);

impl Text {
    pub const MAX_CHARS: usize = 20_000;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Text {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Text, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > Text::MAX_CHARS || s.contains('\0') {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("Text must be 1 to {} characters with no NUL.", Text::MAX_CHARS),
            )
            .with("chars", chars));
        }
        Ok(Text(s.to_owned()))
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A Context's or Plan's title (`context new`/`rename`, `plan new`/`rename`
/// `--title`): 1 to 200 characters, no control characters; else `usage`
/// (commands.md §3.1, §4.1). Job titles stay [`Text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title(String);

impl Title {
    pub const MAX_CHARS: usize = 200;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Title {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Title, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > Title::MAX_CHARS || s.chars().any(char::is_control) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("A title must be 1 to {} characters with no control characters.", Title::MAX_CHARS),
            )
            .with("chars", chars));
        }
        Ok(Title(s.to_owned()))
    }
}

impl fmt::Display for Title {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A title is free text with a smaller range.
impl From<Title> for Text {
    fn from(title: Title) -> Text {
        Text(title.0)
    }
}

/// A PR's own title (`pr add`/`pr edit` `--title`, contract §2.2 PR, hs-d285): 1 to 72
/// characters on one line, no control characters; else `usage` (commands.md §9.4). The
/// squash merge makes it the commit subject on `main`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowTitle(String);

impl WorkflowTitle {
    pub const MAX_CHARS: usize = 72;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for WorkflowTitle {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<WorkflowTitle, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > WorkflowTitle::MAX_CHARS || s.chars().any(char::is_control) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("A PR title must be 1 to {} characters on one line, with no control characters.", WorkflowTitle::MAX_CHARS),
            )
            .with("chars", chars));
        }
        Ok(WorkflowTitle(s.to_owned()))
    }
}

impl fmt::Display for WorkflowTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A display name (`--name`, `session rename`): 1 to 80 characters, no control
/// characters; else `invalid_name` (exit 2) (commands.md §1.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name(String);

impl Name {
    pub const MAX_CHARS: usize = 80;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Name {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Name, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > Name::MAX_CHARS || s.chars().any(char::is_control) {
            return Err(CpdError::new(
                ErrorCode::InvalidName,
                format!("A name must be 1 to {} characters with no control characters.", Name::MAX_CHARS),
            )
            .with("value", s));
        }
        Ok(Name(s.to_owned()))
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_1_to_200_characters_with_no_control_characters() {
        for ok in ["a", "é".repeat(200).as_str(), "a b"] {
            assert_eq!(ok.parse::<Title>().unwrap().as_str(), ok);
        }
        for bad in ["", "a".repeat(201).as_str(), "a\nb", "a\tb", "a\0b", "a\u{7f}b"] {
            assert_eq!(bad.parse::<Title>().unwrap_err().code(), ErrorCode::Usage, "{bad:?}");
        }
    }

    #[test]
    fn a_pr_title_is_1_to_72_characters_on_one_line() {
        for ok in ["a", "é".repeat(72).as_str(), "a b"] {
            assert_eq!(ok.parse::<WorkflowTitle>().unwrap().as_str(), ok);
        }
        for bad in ["", "a".repeat(73).as_str(), "a\nb", "a\r\nb", "a\tb", "a\0b", "a\u{7f}b"] {
            assert_eq!(bad.parse::<WorkflowTitle>().unwrap_err().code(), ErrorCode::Usage, "{bad:?}");
        }
    }

    #[test]
    fn plan_required_when_none() {
        let err = PlanArg { plan: None }.require().unwrap_err();
        assert_eq!(err.code(), ErrorCode::PlanRequired);
        assert_eq!(err.exit().code(), 2);
        let plan = PlanId::parse("cpd2").unwrap();
        assert_eq!(PlanArg { plan: Some(plan.clone()) }.require().unwrap(), &plan);
    }

    #[test]
    fn pr_selection() {
        let plan = PlanId::parse("cpd2").unwrap();
        let workflow = |key: &str, status: &str| WorkflowRow::fixture(key, "/r", "main", &[], status);
        let arg = |key: Option<&str>| WorkflowArg { pr: key.map(|k| WorkflowKey::parse(k).unwrap()) };
        let workflows = [workflow("a", "cancelled"), workflow("b", "waiting")];
        assert_eq!(arg(None).select(&plan, &workflows).unwrap().key.as_str(), "b");
        assert_eq!(arg(Some("a")).select(&plan, &workflows).unwrap().key.as_str(), "a");
        let err = arg(Some("z")).select(&plan, &workflows).unwrap_err();
        assert_eq!((err.code(), err.exit().code()), (ErrorCode::WorkflowNotFound, 4));
        for many in [&[workflow("a", "waiting"), workflow("b", "open")][..], &[workflow("a", "cancelled")][..], &[][..]] {
            let err = arg(None).select(&plan, many).unwrap_err();
            assert_eq!((err.code(), err.exit().code()), (ErrorCode::WorkflowRequired, 2));
        }
    }

    #[test]
    fn session_required_when_none() {
        let err = SessionArg { session: None }.require().unwrap_err();
        assert_eq!(err.code(), ErrorCode::SessionRequired);
        assert_eq!(err.exit().code(), 2);
    }

    #[test]
    fn text_bounds() {
        assert_eq!("a".parse::<Text>().unwrap().as_str(), "a");
        assert!("é".repeat(Text::MAX_CHARS).parse::<Text>().is_ok());
        for bad in [String::new(), "a".repeat(Text::MAX_CHARS + 1), "a\0b".to_owned()] {
            assert_eq!(bad.parse::<Text>().unwrap_err().code(), ErrorCode::Usage);
        }
    }

    #[test]
    fn name_bounds() {
        assert!("cpd rust lead".parse::<Name>().is_ok());
        assert!("ü".repeat(Name::MAX_CHARS).parse::<Name>().is_ok());
        for bad in [String::new(), "a".repeat(Name::MAX_CHARS + 1), "a\nb".to_owned(), "a\tb".to_owned()] {
            assert_eq!(bad.parse::<Name>().unwrap_err().code(), ErrorCode::InvalidName, "{bad:?}");
        }
    }
}
