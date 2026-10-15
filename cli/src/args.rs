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
use crate::ids::{PlanId, SessionId};

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

/// `--session <id>`, else `CPD_SESSION`, else `session_required` (exit 2).
#[derive(clap::Args, Clone, Debug, PartialEq)]
pub struct SessionArg {
    /// The Session [default: $CPD_SESSION]
    #[arg(long = "session", value_name = "id", env = "CPD_SESSION", hide_env = true)]
    pub session: Option<SessionId>,
}

impl SessionArg {
    pub fn require(&self) -> Result<&SessionId, CpdError> {
        self.session.as_ref().ok_or_else(|| {
            CpdError::new(
                ErrorCode::SessionRequired,
                "No Session selected: pass --session <id> or set CPD_SESSION.",
            )
        })
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
    fn plan_required_when_none() {
        let err = PlanArg { plan: None }.require().unwrap_err();
        assert_eq!(err.code(), ErrorCode::PlanRequired);
        assert_eq!(err.exit().code(), 2);
        let plan = PlanId::parse("cpd2").unwrap();
        assert_eq!(PlanArg { plan: Some(plan.clone()) }.require().unwrap(), &plan);
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
