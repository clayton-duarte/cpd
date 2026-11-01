//! The id of a new Context or Plan, derived from its title (commands.md §13.2): ids are unique and immutable and
//! need not be readable, the title is the label, so the User types only the title. Pure.

/// The longest Context id (`^[a-z][a-z0-9-]{1,39}$`, contract §2.1).
pub const CONTEXT_MAX: usize = 40;
/// The longest Plan id (`^[a-z][a-z0-9-]{1,23}$`, contract §2.1).
pub const PLAN_MAX: usize = 24;

/// The lowercase ASCII slug of `title`: runs of other characters become one `-`, leading non-letters are dropped,
/// a trailing `-` is dropped.
fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            if out.is_empty() && !c.is_ascii_alphabetic() {
                continue;
            }
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    out
}

/// `text` cut to `max` characters (it is ASCII), without a trailing `-`.
fn cut(text: &str, max: usize) -> &str {
    text[..text.len().min(max)].trim_end_matches('-')
}

/// The id for `title`, at most `max` characters: its slug, with `-2`, `-3`, … appended (the slug cut to make room)
/// while `taken` says the id is in use. `None` when the title has fewer than two letters or digits after its first
/// letter, which no id can be made of.
pub fn derive(title: &str, max: usize, taken: impl Fn(&str) -> bool) -> Option<String> {
    let slug = slug(title);
    let base = cut(&slug, max);
    if base.len() < 2 {
        return None;
    }
    if !taken(base) {
        return Some(base.to_owned());
    }
    (2..)
        .map(|n| format!("{}-{n}", cut(&slug, max - 1 - n.to_string().len())))
        .find(|id| !taken(id))
}

#[cfg(test)]
mod tests {
    use cpd_core::ids::{ContextId, PlanId};

    use super::*;

    fn free(title: &str, max: usize) -> Option<String> {
        derive(title, max, |_| false)
    }

    #[test]
    fn a_title_becomes_a_lowercase_ascii_slug() {
        assert_eq!(
            free("Building cpd!", CONTEXT_MAX).as_deref(),
            Some("building-cpd")
        );
        assert_eq!(
            free("  Pay   4521 -- the API  ", CONTEXT_MAX).as_deref(),
            Some("pay-4521-the-api")
        );
        assert_eq!(
            free("Café über", CONTEXT_MAX).as_deref(),
            Some("caf-ber"),
            "other characters are runs of `-`"
        );
        assert_eq!(free("a_b.c/d", CONTEXT_MAX).as_deref(), Some("a-b-c-d"));
    }

    #[test]
    fn leading_non_letters_are_dropped() {
        assert_eq!(free("2024 plan", CONTEXT_MAX).as_deref(), Some("plan"));
        assert_eq!(free("--- 12 x1 y", CONTEXT_MAX).as_deref(), Some("x1-y"));
        assert_eq!(free("é ship", CONTEXT_MAX).as_deref(), Some("ship"));
    }

    #[test]
    fn a_title_with_no_id_in_it_gives_none() {
        for title in ["", "   ", "1234", "!!!", "a", "a!", "é", "12 a"] {
            assert_eq!(free(title, PLAN_MAX), None, "{title:?}");
        }
        assert_eq!(free("ab", PLAN_MAX).as_deref(), Some("ab"));
    }

    #[test]
    fn the_id_is_cut_to_the_pattern_and_never_ends_in_a_dash() {
        let long = "abcdefghij klmnopqrstuvwxyz and more words after it";
        assert_eq!(
            free(long, PLAN_MAX).as_deref(),
            Some("abcdefghij-klmnopqrstuvw")
        );
        assert_eq!(free(long, PLAN_MAX).unwrap().len(), PLAN_MAX);
        assert_eq!(
            free("abcdefghijklmnopqrstuvw xyz", PLAN_MAX).as_deref(),
            Some("abcdefghijklmnopqrstuvw"),
            "the cut dash goes"
        );
        assert_eq!(
            free(&long.repeat(3), CONTEXT_MAX).unwrap().len(),
            CONTEXT_MAX
        );
    }

    #[test]
    fn what_is_derived_is_an_id() {
        for title in [
            "Building cpd!",
            "2024 plan",
            &"x".repeat(300),
            "ab",
            &"word ".repeat(40),
        ] {
            assert!(
                ContextId::parse(&free(title, CONTEXT_MAX).unwrap()).is_ok(),
                "{title:?}"
            );
            assert!(
                PlanId::parse(&free(title, PLAN_MAX).unwrap()).is_ok(),
                "{title:?}"
            );
        }
    }

    #[test]
    fn a_taken_id_gets_2_3_and_so_on() {
        let taken = |ids: &'static [&'static str]| move |id: &str| ids.contains(&id);
        assert_eq!(
            derive("Ship it", PLAN_MAX, taken(&["ship-it"])).as_deref(),
            Some("ship-it-2")
        );
        assert_eq!(
            derive("Ship it", PLAN_MAX, taken(&["ship-it", "ship-it-2"])).as_deref(),
            Some("ship-it-3")
        );
        assert_eq!(
            derive("Ship it", PLAN_MAX, taken(&["ship-it", "ship-it-3"])).as_deref(),
            Some("ship-it-2"),
            "the first free one"
        );
        assert_eq!(
            derive("Ship it", PLAN_MAX, taken(&["other"])).as_deref(),
            Some("ship-it")
        );
    }

    #[test]
    fn the_suffix_fits_in_the_pattern() {
        let title = "abcdefghij klmnopqrstuvwxyz";
        let taken = |id: &str| id == "abcdefghij-klmnopqrstuvw";
        assert_eq!(
            derive(title, PLAN_MAX, taken).as_deref(),
            Some("abcdefghij-klmnopqrstu-2")
        );
        let ten = |id: &str| id.len() > 2 && !id.ends_with("-10");
        let id = derive(title, PLAN_MAX, ten).unwrap();
        assert_eq!((id.len(), id.ends_with("-10")), (PLAN_MAX, true), "{id}");
        assert!(PlanId::parse(&id).is_ok());
        assert_eq!(
            derive("ab", PLAN_MAX, |id| id == "ab").as_deref(),
            Some("ab-2")
        );
    }
}
