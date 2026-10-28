//! The `pr.jira` keys' validation (contract §8.3).

use super::domain::JiraSet;

const NAME_MAX: usize = 80;

fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if name.chars().count() > NAME_MAX {
        return Some(format!("the status name is longer than {NAME_MAX} characters"));
    }
    if name.chars().any(char::is_control) {
        return Some("the status name holds a control character".to_owned());
    }
    if name.trim() != name {
        return Some("the status name has leading or trailing whitespace".to_owned());
    }
    None
}

/// The first bad key of a `[pr.jira]` table: `(name, why)`.
pub(super) fn check(set: &JiraSet) -> Result<(), (&'static str, String)> {
    for (key, value) in [("in_progress", &set.in_progress), ("in_review", &set.in_review), ("done", &set.done)] {
        if let Some(why) = value.as_deref().and_then(name_problem) {
            return Err((key, why));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str) -> Result<(), (&'static str, String)> {
        check(&JiraSet { in_progress: None, in_review: Some(name.to_owned()), done: None })
    }

    #[test]
    fn empty_and_a_plain_name_are_valid() {
        assert!(one("").is_ok());
        assert!(one("In Review").is_ok());
        assert!(check(&JiraSet { in_progress: None, in_review: None, done: None }).is_ok());
    }

    #[test]
    fn eighty_characters_are_valid_and_eighty_one_are_not() {
        assert!(one(&"a".repeat(80)).is_ok());
        assert_eq!(one(&"a".repeat(81)).unwrap_err().0, "in_review");
    }

    #[test]
    fn padding_is_refused() {
        assert!(one(" Done").is_err());
        assert!(one("Done ").is_err());
    }

    #[test]
    fn a_control_character_is_refused() {
        assert!(one("In\tReview").is_err());
        assert!(one("In\nReview").is_err());
    }
}
