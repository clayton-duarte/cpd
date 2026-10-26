//! The `[mcp]` table of the machine file (contract §8.3 "MCP servers of the machine scope" and "Machine domains";
//! commands.md §22.1 "The machine object"): `$CPD_HOME/policy/host.toml`.
//! The file is also the machine scope (its other domains are read by the scope loader, `load.rs`); this module reads
//! `[mcp.servers.<name>]` only.
//!
//! - [`parse_machine`]: parses the text of one machine file. Every top-level key but `mcp` is left to the scope loader
//!   and ignored here; only `servers` is allowed under `mcp`; each entry is a closed [`McpServer`]
//!   (`deny_unknown_fields`, pi's camelCase key names); names match `^[a-z][a-z0-9-]{0,31}$`; strings are 1 to 500
//!   characters with no NUL; `url` and `oauth.callbackUrl` match the patterns of §8.3; `toolExposure`, when present,
//!   has at least one entry and non-empty keys. Every refusal is `policy_invalid` (`policy::policy_invalid`) with
//!   `details.file` the `file` argument and `details.key` the dotted path of the offending key (null when the file
//!   does not parse).
//!   `headers`, when present, is a table of at least one header name to literal value: a name matches
//!   `^[A-Za-z0-9-]{1,64}$` and is not `authorization`, `proxy-authorization` or `cookie` (any case); a value is a
//!   string of 1 to 500 characters with no NUL, CR or LF, no `${` and no leading `!`.
//! - [`load_machine`]: reads only `host.toml` of the home's `policy/` (no file: no servers, [`MachineSource::Default`];
//!   else [`MachineSource::File`] with its realpath). The names in `policy/` are checked by `check_file_names`.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{de, Deserialize, Deserializer, Serialize};
use toml::{Table, Value};

use super::load::{bad_key, policy_invalid};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use super::resolve::machine_path;

/// One of pi's four exposures, spelled `codemode`, `deferred`, `direct` or `hidden`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Exposure {
    Codemode,
    Deferred,
    Direct,
    Hidden,
}

/// The optional `oauth` table of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McpOauth {
    #[serde(default, deserialize_with = "text", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, deserialize_with = "text", skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, deserialize_with = "callback_url", skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

/// One `[mcp.servers.<name>]` entry; a key left out stays `None` (and out of the JSON).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<Exposure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_exposure: Option<BTreeMap<String, Exposure>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOauth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

/// An entry as read: `url` is checked present afterwards, so that [`bad_key`] can read one key at a time.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawEntry {
    #[serde(default, deserialize_with = "server_url")]
    url: Option<String>,
    #[serde(default, deserialize_with = "text")]
    description: Option<String>,
    exposure: Option<Exposure>,
    #[serde(default, deserialize_with = "tool_exposure")]
    tool_exposure: Option<BTreeMap<String, Exposure>>,
    oauth: Option<McpOauth>,
    #[serde(default, deserialize_with = "headers")]
    headers: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    mcp: Option<RawMcp>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMcp {
    #[serde(default)]
    servers: BTreeMap<String, Table>,
}

/// Strings are 1 to 500 characters with no NUL.
fn valid_text(s: &str) -> Result<(), String> {
    match s.chars().count() {
        1..=500 if !s.contains('\0') => Ok(()),
        _ => Err("a string is 1 to 500 characters with no NUL".to_owned()),
    }
}

fn checked<'de, D: Deserializer<'de>>(d: D, check: impl Fn(&str) -> Result<(), String>) -> Result<Option<String>, D::Error> {
    let s = Option::<String>::deserialize(d)?;
    if let Some(s) = &s {
        valid_text(s).and_then(|()| check(s)).map_err(de::Error::custom)?;
    }
    Ok(s)
}

fn text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    checked(d, |_| Ok(()))
}

fn server_url<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    checked(d, |s| if url_ok(s, true) { Ok(()) } else { Err("not an https URL, or an http URL to localhost".to_owned()) })
}

fn callback_url<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    checked(d, |s| if url_ok(s, false) { Ok(()) } else { Err("not an http URL to localhost".to_owned()) })
}

fn tool_exposure<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeMap<String, Exposure>>, D::Error> {
    let Some(map) = Option::<BTreeMap<String, Exposure>>::deserialize(d)? else { return Ok(None) };
    if map.is_empty() {
        return Err(de::Error::custom("toolExposure has at least one entry"));
    }
    map.keys().try_for_each(|k| valid_text(k)).map_err(de::Error::custom)?;
    Ok(Some(map))
}

/// `headers`: at least one entry; names and values are checked by [`header_name_ok`] and [`header_value_ok`].
fn headers<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BTreeMap<String, String>>, D::Error> {
    let Some(map) = Option::<BTreeMap<String, String>>::deserialize(d)? else { return Ok(None) };
    if map.is_empty() {
        return Err(de::Error::custom("headers has at least one entry"));
    }
    for (name, value) in &map {
        header_name_ok(name).and_then(|()| header_value_ok(value)).map_err(de::Error::custom)?;
    }
    Ok(Some(map))
}

/// `^[A-Za-z0-9-]{1,64}$`, and not a credential header (compared case-insensitively): credentials are OAuth's.
fn header_name_ok(name: &str) -> Result<(), String> {
    if !(1..=64).contains(&name.len()) || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("a header name matches ^[A-Za-z0-9-]{1,64}$".to_owned());
    }
    if ["authorization", "proxy-authorization", "cookie"].iter().any(|c| name.eq_ignore_ascii_case(c)) {
        return Err("a header name is not authorization, proxy-authorization or cookie".to_owned());
    }
    Ok(())
}

/// 1 to 500 characters with no NUL, CR or LF, no `${` and not starting with `!` (pi expands and runs those).
fn header_value_ok(value: &str) -> Result<(), String> {
    valid_text(value)?;
    if value.contains(['\r', '\n']) {
        return Err("a header value has no CR or LF".to_owned());
    }
    if value.contains("${") || value.starts_with('!') {
        return Err("a header value is literal: no ${ and no leading !".to_owned());
    }
    Ok(())
}

/// `^https://[^/@?#\s]+([/?][^\s#]*)?$`, or (`http_local`) `^http://` + a loopback host (contract §8.3).
fn url_ok(url: &str, https: bool) -> bool {
    let (host_ok, rest): (fn(&str) -> bool, &str) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) if https => (|_| true, rest),
        (_, Some(rest)) => (loopback, rest),
        _ => return false,
    };
    let end = rest.find(|c: char| "/@?#".contains(c) || c.is_whitespace()).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    !authority.is_empty()
        && host_ok(authority)
        && (tail.is_empty() || (tail.starts_with(['/', '?']) && !tail.contains(|c: char| c.is_whitespace() || c == '#')))
}

/// `(localhost|127\.0\.0\.1|\[::1\])(:[0-9]{1,5})?`
fn loopback(authority: &str) -> bool {
    let (host, port) = match authority.strip_prefix("[::1]") {
        Some(port) => ("[::1]", port),
        None => authority.split_once(':').map_or((authority, ""), |(h, _)| (h, &authority[h.len()..])),
    };
    let port_ok = match port.strip_prefix(':') {
        None => port.is_empty(),
        Some(digits) => (1..=5).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()),
    };
    port_ok && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// `^[a-z][a-z0-9-]{0,31}$`
pub fn name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Where the machine's servers come from: `default` (no file) or `machine:<realpath>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineSource {
    Default,
    File(PathBuf),
}

impl std::fmt::Display for MachineSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MachineSource::Default => f.write_str("default"),
            MachineSource::File(p) => write!(f, "machine:{}", p.display()),
        }
    }
}

/// The machine scope of this instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub source: MachineSource,
    pub servers: BTreeMap<String, McpServer>,
}

/// Parses the text of the machine file `file` (see the module docs for the rules).
pub fn parse_machine(file: &Path, text: &str) -> Result<BTreeMap<String, McpServer>, CpdError> {
    let mut table: Table = text
        .parse()
        .map_err(|e: toml::de::Error| policy_invalid(file, None, format!("not valid TOML ({})", e.message())))?;
    // The machine file is the machine scope (contract §8.3): its other domains are read by `load_scope_file`; this reads `[mcp]` only.
    table.retain(|k, _| k == "mcp");
    let raw = Value::Table(table.clone()).try_into::<RawFile>().map_err(|e| {
        policy_invalid(file, Some(&bad_key::<RawFile>(&table).join(".")), e.message())
    })?;
    let mut servers = BTreeMap::new();
    for (name, entry) in raw.mcp.map(|m| m.servers).unwrap_or_default() {
        let at = |rest: &[String]| format!("mcp.servers.{name}{}", rest.iter().map(|k| format!(".{k}")).collect::<String>());
        if !name_ok(&name) {
            return Err(policy_invalid(file, Some(&at(&[])), "a server name matches ^[a-z][a-z0-9-]{0,31}$"));
        }
        let parsed = Value::Table(entry.clone())
            .try_into::<RawEntry>()
            .map_err(|e| policy_invalid(file, Some(&at(&bad_key::<RawEntry>(&entry))), e.message()))?;
        let url = parsed.url.ok_or_else(|| policy_invalid(file, Some(&at(&["url".to_owned()])), "url is required"))?;
        servers.insert(
            name,
            McpServer {
                url,
                description: parsed.description,
                exposure: parsed.exposure,
                tool_exposure: parsed.tool_exposure,
                oauth: parsed.oauth,
                headers: parsed.headers,
            },
        );
    }
    Ok(servers)
}

/// The `[mcp]` servers of the machine file of `home` (the machine scope, `policy/host.toml`; contract §8.3).
pub fn load_machine(home: &CpdHome) -> Result<Machine, CpdError> {
    let path = machine_path(home);
    let none = Machine { source: MachineSource::Default, servers: BTreeMap::new() };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(none),
        Err(e) => {
            return Err(CpdError::new(ErrorCode::FileUnreadable, format!("Cannot read the policy file {}: {e}.", path.display()))
                .with("file", path.display().to_string()));
        }
    };
    let real = std::fs::canonicalize(&path).map_err(|e| {
        CpdError::new(ErrorCode::FileUnreadable, format!("Cannot resolve {}: {e}.", path.display()))
            .with("file", path.display().to_string())
    })?;
    let servers = parse_machine(&real, &text)?;
    Ok(if servers.is_empty() { none } else { Machine { source: MachineSource::File(real), servers } })
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
