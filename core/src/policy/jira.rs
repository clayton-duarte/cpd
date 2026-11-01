//! The `pr.jira` keys' validation (contract §8.3).

use super::domain::JiraSet;

const NAME_MAX: usize = 80;

fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if name.chars().count() > NAME_MAX {
        return Some(format!(
            "the status name is longer than {NAME_MAX} characters"
        ));
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
    for (key, value) in [
        ("in_progress", &set.in_progress),
        ("in_review", &set.in_review),
        ("done", &set.done),
    ] {
        if let Some(why) = value.as_deref().and_then(name_problem) {
            return Err((key, why));
        }
    }
    if let Some(ids) = &set.estimate_fields {
        for (i, id) in ids.iter().enumerate() {
            if !is_field_id(id) {
                return Err((
                    "estimate_fields",
                    format!("{id:?} is not a Jira custom field id (customfield_ and digits)"),
                ));
            }
            if ids[..i].contains(id) {
                return Err(("estimate_fields", format!("{id:?} is listed twice")));
            }
        }
    }
    Ok(())
}

fn is_field_id(id: &str) -> bool {
    id.strip_prefix("customfield_")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str) -> Result<(), (&'static str, String)> {
        check(&JiraSet {
            in_progress: None,
            in_review: Some(name.to_owned()),
            done: None,
            estimate_fields: None,
        })
    }

    #[test]
    fn empty_and_a_plain_name_are_valid() {
        assert!(one("").is_ok());
        assert!(one("In Review").is_ok());
        assert!(
            check(&JiraSet {
                in_progress: None,
                in_review: None,
                done: None,
                estimate_fields: None
            })
            .is_ok()
        );
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

    fn fields(ids: &[&str]) -> Result<(), (&'static str, String)> {
        check(&JiraSet {
            in_progress: None,
            in_review: None,
            done: None,
            estimate_fields: Some(ids.iter().map(|s| (*s).to_owned()).collect()),
        })
    }

    #[test]
    fn custom_field_ids_are_valid() {
        assert!(fields(&[]).is_ok());
        assert!(fields(&["customfield_10016", "customfield_1"]).is_ok());
    }

    #[test]
    fn a_bad_or_repeated_field_id_is_refused() {
        for bad in [
            "",
            "10016",
            "customfield_",
            "customfield_1a",
            "Customfield_1",
            " customfield_1",
            "customfield_1\n",
        ] {
            assert_eq!(fields(&[bad]).unwrap_err().0, "estimate_fields", "{bad:?}");
        }
        assert_eq!(
            fields(&["customfield_1", "customfield_1"]).unwrap_err().0,
            "estimate_fields"
        );
    }
}
