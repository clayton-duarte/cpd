//! The machine policy scope (contract §8.3 "The machine scope" and "Machine domains";
//! commands.md §22.1 "The machine object"): `$CPD_HOME/policy/machines/<machine>.toml`,
//! which sets only `[mcp.servers.<name>]`.
//!
//! API the tests pin (the implementer follows this and the contract):
//!
//! - [`parse_machine`]: parses the text of one machine file. Only the top-level table `mcp` is
//!   allowed, only `servers` under it; each entry is a closed [`McpServer`] (`deny_unknown_fields`,
//!   pi's camelCase key names); names match `^[a-z][a-z0-9-]{0,31}$`; strings are 1 to 500
//!   characters with no NUL; `url` and `oauth.callbackUrl` match the patterns of §8.3;
//!   `toolExposure`, when present, has at least one entry and non-empty keys. Every refusal is
//!   `policy_invalid` (`policy::policy_invalid`) with `details.file` the `file` argument and
//!   `details.key` the dotted path of the offending key (null when the file does not parse).
//! - [`load_machine`]: reads only `machines/<machine>.toml` of the home (no file: no servers,
//!   [`MachineSource::Default`]; else [`MachineSource::File`] with its realpath). Other machines'
//!   files are not read, but every entry of `machines/` has its name checked: a name that is not
//!   a valid machine name, or an entry that is not `<name>.toml`, is `policy_invalid`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ids::MachineName;

/// One of pi's four exposures, spelled `codemode`, `deferred`, `direct` or `hidden`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exposure {
    Codemode,
    Deferred,
    Direct,
    Hidden,
}

/// The optional `oauth` table of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct McpOauth {
    pub client_id: Option<String>,
    pub scope: Option<String>,
    pub callback_url: Option<String>,
}

/// One `[mcp.servers.<name>]` entry; a key left out stays `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub url: String,
    pub description: Option<String>,
    pub exposure: Option<Exposure>,
    pub tool_exposure: Option<BTreeMap<String, Exposure>>,
    pub oauth: Option<McpOauth>,
}

/// Where the machine's servers come from: `default` (no file) or `machine:<realpath>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineSource {
    Default,
    File(PathBuf),
}

/// The machine scope of this instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub source: MachineSource,
    pub servers: BTreeMap<String, McpServer>,
}

/// Parses the text of the machine file `file` (see the module docs for the rules).
pub fn parse_machine(_file: &Path, _text: &str) -> Result<BTreeMap<String, McpServer>, CpdError> {
    todo!("the machine policy scope")
}

/// The machine scope of `machine` in `home` (see the module docs for the rules).
pub fn load_machine(_home: &CpdHome, _machine: &MachineName) -> Result<Machine, CpdError> {
    todo!("the machine policy scope")
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
