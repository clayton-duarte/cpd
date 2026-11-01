//! A hook's commands (contract §7.3): the three forms of worktrunk, with commands in file order.

use serde::{Deserialize, Serialize};
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// One command of a hook, as written (not yet rendered).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookCommand {
    /// The key it has in a table or block; `None` for `hook = "cmd"`.
    pub name: Option<String>,
    #[serde(rename = "command")]
    pub template: String,
}

/// A hook's commands: blocks run in order, the commands of a block together.
/// `hook = "cmd"` and `[hook]` are one block; `[[hook]]` is one block each.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pipeline {
    pub blocks: Vec<Vec<HookCommand>>,
}

impl Pipeline {
    /// Every command, in file order.
    pub fn commands(&self) -> impl Iterator<Item = &HookCommand> {
        self.blocks.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.commands().next().is_none()
    }
}

/// The pipeline a hook's value is, in any of the three forms (`what` names the key in the error sentence).
/// Read from the `[hooks]` table of a policy file.
pub fn parse_value(value: &Spanned<DeValue<'_>>, what: &str) -> Result<Pipeline, String> {
    let bad = |why: &str| {
        format!(
            "{what} must be a string, a table of strings or an array of tables of strings, {why}."
        )
    };
    let blocks = match value.get_ref() {
        DeValue::String(s) => vec![vec![HookCommand {
            name: None,
            template: s.to_string(),
        }]],
        DeValue::Table(t) => vec![block(t).map_err(|w| bad(&w))?],
        DeValue::Array(items) => items
            .iter()
            .map(|item| match item.get_ref() {
                DeValue::Table(t) => block(t).map_err(|w| bad(&w)),
                other => Err(bad(&format!("not a {} in the array", other.type_str()))),
            })
            .collect::<Result<_, _>>()?,
        other => return Err(bad(&format!("not a {}", other.type_str()))),
    };
    Ok(Pipeline { blocks })
}

/// The commands of one table, in the order their keys appear in the file
/// (`toml` keeps tables sorted by key; the keys' spans restore file order).
fn block(table: &DeTable<'_>) -> Result<Vec<HookCommand>, String> {
    let mut entries: Vec<_> = table.iter().collect();
    entries.sort_by_key(|(k, _)| k.span().start);
    entries
        .into_iter()
        .map(|(k, v)| match v.get_ref() {
            DeValue::String(s) => Ok(HookCommand {
                name: Some(k.get_ref().to_string()),
                template: s.to_string(),
            }),
            other => Err(format!("`{}` is a {}", k.get_ref(), other.type_str())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::Hook;

    /// The `hook` key of a TOML text, as the policy loader reads it.
    fn parse(text: &str, hook: Hook) -> Result<Pipeline, String> {
        let doc = DeTable::parse(text).map_err(|e| e.to_string())?;
        let name = hook.as_str();
        match doc
            .get_ref()
            .iter()
            .find(|(k, _)| k.get_ref().as_ref() == name)
        {
            Some((_, value)) => parse_value(value, name),
            None => Ok(Pipeline::default()),
        }
    }

    fn cmd(name: Option<&str>, template: &str) -> HookCommand {
        HookCommand {
            name: name.map(str::to_owned),
            template: template.to_owned(),
        }
    }

    #[test]
    fn a_string_is_one_unnamed_command() {
        let p = parse("pre-merge = \"make\"\n", Hook::PreMerge).unwrap();
        assert_eq!(p.blocks, vec![vec![cmd(None, "make")]]);
    }

    #[test]
    fn a_table_is_one_block_in_file_order() {
        let p = parse(
            "[pre-merge]\nz = \"1\"\na = \"2\"\nm = \"3\"\n",
            Hook::PreMerge,
        )
        .unwrap();
        assert_eq!(
            p.blocks,
            vec![vec![
                cmd(Some("z"), "1"),
                cmd(Some("a"), "2"),
                cmd(Some("m"), "3")
            ]]
        );
    }

    #[test]
    fn an_array_of_tables_is_a_pipeline() {
        let text = "[[pre-merge]]\ninstall = \"i\"\n\n[[pre-merge]]\nz = \"b\"\na = \"c\"\n";
        let p = parse(text, Hook::PreMerge).unwrap();
        assert_eq!(
            p.blocks,
            vec![
                vec![cmd(Some("install"), "i")],
                vec![cmd(Some("z"), "b"), cmd(Some("a"), "c")]
            ]
        );
    }

    #[test]
    fn other_tables_are_ignored_and_a_missing_one_is_empty() {
        let text = "post-start = 3\n[list]\nfull = true\n";
        assert!(parse(text, Hook::PreMerge).unwrap().is_empty());
    }

    #[test]
    fn wrong_shapes_and_bad_toml_are_errors() {
        for text in [
            "pre-merge = 3",
            "pre-merge = [\"a\"]",
            "[pre-merge]\na = 1",
            "pre-merge = ",
            "[[pre-merge]]\na = [1]",
        ] {
            assert!(parse(text, Hook::PreMerge).is_err(), "{text}");
        }
    }
}
