//! Rendering the `{{ … }}` / `{% … %}` templates of a hook command (contract §7.3).

use std::path::Path;

use minijinja::value::Value;
use minijinja::{Environment, Error, UndefinedBehavior};
use serde_json::{Map, Value as Json};

/// Renders `template` with the variables `vars`. Values are shell-escaped, as
/// worktrunk does. An undefined variable, an unknown filter or function, or a
/// syntax error is an `Err` (a sentence), so nothing runs.
pub fn render(template: &str, vars: &Map<String, Json>) -> Result<String, String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_formatter(format_value);
    env.add_filter("dirname", dirname);
    env.add_filter("basename", basename);
    env.render_str(
        template,
        Value::from_pairs(vars.iter().map(|(k, v)| (k.as_str(), to_value(v)))),
    )
    .map_err(|e| format!("Cannot render `{template}`: {e}"))
}

/// Whether `template` uses the variable `name`; `Err` (a sentence) when it does not parse.
pub fn uses(template: &str, name: &str) -> Result<bool, String> {
    let env = Environment::new();
    let compiled = env
        .template_from_str(template)
        .map_err(|e| format!("Cannot parse `{template}`: {e}"))?;
    Ok(compiled.undeclared_variables(false).contains(name))
}

/// The variables are strings and lists (`args`); `minijinja` here has no serde bridge.
fn to_value(json: &Json) -> Value {
    match json {
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(items) => Value::from(items.iter().map(to_value).collect::<Vec<_>>()),
        Json::Object(map) => Value::from_pairs(map.iter().map(|(k, v)| (k.as_str(), to_value(v)))),
        other => Value::from(other.to_string()),
    }
}

fn dirname(path: String) -> String {
    Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn basename(path: String) -> String {
    Path::new(&path)
        .file_name()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes `value` shell-escaped; a list is its elements, space-joined.
fn format_value(
    out: &mut minijinja::Output,
    _state: &mut minijinja::State,
    value: &Value,
) -> Result<(), Error> {
    let text = match value.try_iter() {
        Ok(items) if value.kind() == minijinja::value::ValueKind::Seq => items
            .map(|v| shell_escape(&v.to_string()))
            .collect::<Vec<_>>()
            .join(" "),
        _ => shell_escape(&value.to_string()),
    };
    std::fmt::Write::write_str(out, &text)
        .map_err(|_| Error::new(minijinja::ErrorKind::WriteFailure, "write failed"))
}

/// `s` as one shell word: unchanged when it only has safe characters, else in single quotes.
pub fn shell_escape(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> Map<String, Json> {
        json!({"branch": "feat/x y", "args": [], "path": "/a/b/c", "empty": ""})
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn renders_and_shell_escapes() {
        assert_eq!(
            render("echo {{ branch }}", &vars()).unwrap(),
            "echo 'feat/x y'"
        );
        assert_eq!(render("{{ path }}", &vars()).unwrap(), "/a/b/c");
        assert_eq!(render("[{{ empty }}]", &vars()).unwrap(), "['']");
        assert_eq!(render("[{{ args }}]", &vars()).unwrap(), "[]");
        assert_eq!(
            render("{{ path | dirname }} {{ path | basename }}", &vars()).unwrap(),
            "/a/b c"
        );
        assert_eq!(render("a'b", &vars()).unwrap(), "a'b");
        assert_eq!(render("{{ \"it's\" }}", &vars()).unwrap(), "'it'\\''s'");
    }

    #[test]
    fn undefined_names_and_worktrunk_filters_fail() {
        for t in [
            "{{ nope }}",
            "{{ branch | sanitize }}",
            "{{ vars.x }}",
            "{{ branch | hash_port }}",
            "{{ worktree_path_of_branch('main') }}",
            "{% if %}",
        ] {
            assert!(render(t, &vars()).is_err(), "{t}");
        }
    }
}
