//! The `pr.post` keys' rendering and validation (contract §8.3).

use minijinja::{Environment, UndefinedBehavior};

use super::domain::PostSet;

const PROBE_TITLE: &str = "probe-title";
const PROBE_URL: &str = "https://probe.invalid/pull/1";
const TEMPLATE_MAX: usize = 1000;

/// Renders `template` with the variables `title` and `url` only: strict undefined, no escaping, the
/// text verbatim (minijinja strips a single trailing newline). `Err` is a sentence.
pub fn render_post(template: &str, title: &str, url: &str) -> Result<String, String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.render_str(template, minijinja::context! { title, url })
        .map_err(|e| e.to_string())
}

/// The first bad key of a `[pr.post]` table: `(name, why)`.
pub(super) fn check(set: &PostSet) -> Result<(), (&'static str, String)> {
    if let Some(channel) = &set.channel {
        let ok = channel.is_empty()
            || (channel.len() >= 9
                && channel.starts_with('C')
                && channel[1..].bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
        if !ok {
            return Err(("channel", format!("`{channel}` is not empty or a Slack channel id (C followed by 8 or more capitals or digits)")));
        }
    }
    if let Some(template) = &set.template {
        if template.chars().count() > TEMPLATE_MAX {
            return Err(("template", format!("the template is longer than {TEMPLATE_MAX} characters")));
        }
        let out = render_post(template, PROBE_TITLE, PROBE_URL).map_err(|why| ("template", why))?;
        if !out.contains(PROBE_URL) {
            return Err(("template", "the template does not use `url`".to_owned()));
        }
    }
    if let Some(reaction) = &set.merged_reaction {
        let ok = reaction.is_empty()
            || ((1..=100).contains(&reaction.len())
                && reaction.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'+' | b'-')));
        if !ok {
            return Err(("merged_reaction", format!("`{reaction}` is not empty or a Slack emoji name without colons ([a-z0-9_+-], 1 to 100)")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_and_url_are_the_variables() {
        assert_eq!(render_post("{{ title }}\n{{ url }}", "T", "U").unwrap(), "T\nU");
    }

    #[test]
    fn nothing_is_escaped() {
        assert_eq!(render_post("{{ title }} {{ url }}", "a & <b>", "u").unwrap(), "a & <b> u");
    }

    #[test]
    fn an_unknown_variable_is_an_error() {
        assert!(render_post("{{ nope }} {{ url }}", "T", "U").is_err());
    }

    #[test]
    fn a_template_without_url_is_refused() {
        let set = PostSet { enabled: None, channel: None, template: Some("{{ title }}".to_owned()), merged_reaction: None };
        assert_eq!(check(&set).unwrap_err().0, "template");
    }
}
