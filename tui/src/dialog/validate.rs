//! The validation of an `input` dialog (commands.md §13.2, "The dialog kit"): a closed set the callers pass by name,
//! `--validate plan-id|title|name`. Each is the check of the command the typed text goes to, so the popup refuses
//! what that command would refuse: the reason is the command's own message.

use std::str::FromStr;

use cpd_core::args::{Name, Title};
use cpd_core::ids::PlanId;

/// A Plan id's longest length (contract §2.1: `^[a-z][a-z0-9-]{1,23}$`); `PlanId` exports no constant.
const PLAN_ID_MAX: usize = 24;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validator {
    /// A Plan id (contract §2.1): `^[a-z][a-z0-9-]{1,23}$`
    PlanId,
    /// A Context's or Plan's title: 1 to 200 characters
    Title,
    /// A Session's display name: 1 to 80 characters
    Name,
}

impl Validator {
    /// The name on the command line.
    pub(crate) fn as_arg(self) -> &'static str {
        match self {
            Validator::PlanId => "plan-id",
            Validator::Title => "title",
            Validator::Name => "name",
        }
    }

    /// The most characters a value may have: the checks' own limits.
    pub(crate) fn max_len(self) -> usize {
        match self {
            Validator::PlanId => PLAN_ID_MAX,
            Validator::Title => Title::MAX_CHARS,
            Validator::Name => Name::MAX_CHARS,
        }
    }

    /// The accepted value (the text, trimmed), or why it is refused.
    pub(crate) fn check(self, text: &str) -> Result<String, String> {
        let text = text.trim();
        let refused = |e: cpd_core::error::CpdError| e.message().to_owned();
        match self {
            Validator::PlanId => PlanId::parse(text).map(|_| ()).map_err(refused),
            Validator::Title => Title::from_str(text).map(|_| ()).map_err(refused),
            Validator::Name => Name::from_str(text).map(|_| ()).map_err(refused),
        }?;
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_value_is_the_checks_limit() {
        assert_eq!(Validator::PlanId.max_len(), 24);
        assert!(
            Validator::PlanId.check(&"a".repeat(24)).is_ok()
                && Validator::PlanId.check(&"a".repeat(25)).is_err()
        );
        assert_eq!(Validator::Title.max_len(), 200);
        assert_eq!(Validator::Name.max_len(), 80);
    }

    #[test]
    fn a_plan_id_is_the_contracts() {
        assert_eq!(
            Validator::PlanId.check("  pay-4521 "),
            Ok("pay-4521".to_owned())
        );
        for bad in ["", "a", "Pay", "1abc", "has space", &"a".repeat(25)] {
            assert!(Validator::PlanId.check(bad).is_err(), "{bad:?}");
        }
        assert!(Validator::PlanId.check("ab").is_ok());
        assert!(Validator::PlanId.check(&"a".repeat(24)).is_ok());
    }

    #[test]
    fn a_title_is_1_to_200_characters_and_a_name_1_to_80() {
        assert_eq!(
            Validator::Title.check(" A title "),
            Ok("A title".to_owned())
        );
        assert!(Validator::Title.check(&"é".repeat(200)).is_ok());
        for bad in ["", "   ", &"a".repeat(201), "a\tb"] {
            assert!(Validator::Title.check(bad).is_err(), "{bad:?}");
        }
        assert!(Validator::Name.check(&"n".repeat(80)).is_ok());
        for bad in ["", "  ", &"n".repeat(81)] {
            assert!(Validator::Name.check(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_refusal_carries_the_commands_reason() {
        assert!(Validator::Title.check("").unwrap_err().contains("1 to 200"));
        assert!(Validator::Name.check("").unwrap_err().contains("1 to 80"));
        assert!(
            Validator::PlanId
                .check("Bad")
                .unwrap_err()
                .contains("Plan id")
        );
    }

    #[test]
    fn the_names_are_the_command_lines() {
        use clap::ValueEnum;
        for v in Validator::value_variants() {
            let parsed = Validator::from_str(v.as_arg(), false).unwrap();
            assert_eq!(parsed, *v);
        }
    }
}
