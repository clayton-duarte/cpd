//! The `[tests]` table of a repo's `.config/cpd.toml` (contract §7.7): the commands that build, find and run the
//! named tests of a `test` or `impl` Job. Read from the Lead Branch's tree by [`crate::git::tamper`], next to the
//! `protected` list: one file, one parse, so a table that is not as documented is `config_invalid` wherever it is read.

use serde::Deserialize;

use crate::git::hooks::template_uses;

/// `[tests]`: `build` and `run` are required, `list` is optional (hs-d497); any other key is refused.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RepoTests {
    /// Compiles what the named tests need: non-zero is a build error.
    pub build: String,
    /// Runs the named tests (`{{ tests }}`): zero is pass, non-zero is fail.
    pub run: String,
    /// Zero when the one named test `{{ test }}` exists, non-zero when no test has that name.
    #[serde(default)]
    pub list: Option<String>,
}

impl RepoTests {
    /// What serde cannot say: every command is non-empty and `list` uses the template variable `test`.
    /// The error is a sentence naming the key.
    pub fn validate(&self) -> Result<(), String> {
        for (key, command) in [("build", Some(&self.build)), ("run", Some(&self.run)), ("list", self.list.as_ref())] {
            if command.is_some_and(|c| c.trim().is_empty()) {
                return Err(format!("[tests] `{key}` is empty: a repo with nothing to build says `build = \"true\"`"));
            }
        }
        if let Some(list) = &self.list
            && !template_uses(list, "test")?
        {
            return Err("[tests] `list` must use the template variable `test` (one named test), as `{{ test }}`".to_owned());
        }
        Ok(())
    }
}
