//! Tests of the `[mcp]` table of the machine file (contract §8.3 "MCP servers of the machine scope", "Machine domains").

use super::*;
use crate::error::ErrorCode;
use crate::policy::{check_file_names, parse_scope, Domain};

const SLACK: &str = r#"
[mcp.servers.slack]
url = "https://mcp.slack.com/mcp"
description = "Slack: read channels and threads, post messages, and manage reactions."
exposure = "hidden"

[mcp.servers.slack.oauth]
clientId = "1601185624273.8899143856786"
callbackUrl = "http://localhost:3118/callback"
scope = "channels:history chat:write reactions:write"

[mcp.servers.slack.toolExposure]
slack_read_channel = "direct"
slack_read_thread = "direct"
slack_add_reaction = "direct"
slack_get_reactions = "direct"
slack_search_emojis = "direct"
slack_read_user_profile = "direct"
slack_send_message = "direct"
"#;

fn file() -> &'static Path {
    Path::new("/h/policy/machines/pro.toml")
}

fn parse(text: &str) -> Result<BTreeMap<String, McpServer>, CpdError> {
    parse_machine(file(), text)
}

fn entry(body: &str) -> String {
    format!("[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\n{body}\n")
}

fn named(name: &str) -> String {
    format!("[mcp.servers.{name}]\nurl = \"https://mcp.slack.com/mcp\"\n")
}

fn accepted(text: &str) {
    if let Err(e) = parse(text) {
        panic!("refused {text:?}: {e:?}");
    }
}

/// Refused as `policy_invalid`, naming the file; the `key` detail contains `key` when given.
fn refused(text: &str, key: Option<&str>) {
    let err = parse(text).expect_err(text);
    assert_eq!(err.code(), ErrorCode::PolicyInvalid, "{text:?}: {err:?}");
    assert_eq!(err.exit().code(), 1);
    assert_eq!(err.details()["file"], file().display().to_string(), "{text:?}");
    if let Some(key) = key {
        let got = err.details()["key"].as_str().unwrap_or_else(|| panic!("no key detail for {text:?}: {err:?}"));
        assert!(got.contains(key), "{text:?}: key {got:?} lacks {key:?}");
    }
}

fn pro() -> HostName {
    HostName::parse("pro").unwrap()
}

fn home() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

fn write(home: &CpdHome, rel: &str, text: &str) {
    let path = home.root().join("policy").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

// a. no file, empty file

#[test]
fn no_file_means_no_servers_from_default() {
    let (_d, home) = home();
    let m = load_machine(&home, &pro()).unwrap();
    assert_eq!(m.source, MachineSource::Default);
    assert!(m.servers.is_empty());
}

#[test]
fn an_empty_file_means_no_servers_from_default() {
    let (_d, home) = home();
    write(&home, "machines/pro.toml", "");
    let m = load_machine(&home, &pro()).unwrap();
    assert_eq!(m.source, MachineSource::Default);
    assert!(m.servers.is_empty());
}

// b. pro's slack entry

#[test]
fn the_slack_entry_loads_with_every_field_typed() {
    let servers = parse(SLACK).unwrap();
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["slack"]);
    let s = &servers["slack"];
    assert_eq!(s.url, "https://mcp.slack.com/mcp");
    assert_eq!(s.description.as_deref(), Some("Slack: read channels and threads, post messages, and manage reactions."));
    assert_eq!(s.exposure, Some(Exposure::Hidden));
    let tools = s.tool_exposure.as_ref().unwrap();
    assert_eq!(tools.len(), 7);
    assert!(tools.values().all(|e| *e == Exposure::Direct));
    assert_eq!(tools["slack_send_message"], Exposure::Direct);
    let oauth = s.oauth.as_ref().unwrap();
    assert_eq!(oauth.client_id.as_deref(), Some("1601185624273.8899143856786"));
    assert_eq!(oauth.callback_url.as_deref(), Some("http://localhost:3118/callback"));
    assert_eq!(oauth.scope.as_deref(), Some("channels:history chat:write reactions:write"));
}

#[test]
fn the_slack_file_loads_from_the_machines_directory_with_its_realpath() {
    let (_d, home) = home();
    write(&home, "machines/pro.toml", SLACK);
    let m = load_machine(&home, &pro()).unwrap();
    let real = std::fs::canonicalize(home.root().join("policy/machines/pro.toml")).unwrap();
    assert_eq!(m.source, MachineSource::File(real));
    assert_eq!(m.servers.len(), 1);
}

#[test]
fn a_key_left_out_stays_out() {
    let s = &parse(&named("a")).unwrap()["a"];
    assert_eq!((&s.description, &s.exposure, &s.tool_exposure, &s.oauth), (&None, &None, &None, &None));
}

// c. refused keys and tables

#[test]
fn a_headers_key_is_refused() {
    refused(&entry("headers = { A = \"b\" }"), Some("headers"));
}

#[test]
fn a_command_key_is_refused() {
    refused(&entry("command = \"x\""), Some("command"));
}

#[test]
fn an_env_key_is_refused() {
    refused(&entry("env = { A = \"b\" }"), Some("env"));
}

#[test]
fn a_client_secret_key_is_refused() {
    refused(&entry("clientSecret = \"x\""), Some("clientSecret"));
}

#[test]
fn an_enabled_key_is_refused() {
    refused(&entry("enabled = true"), Some("enabled"));
}

#[test]
fn an_auth_key_is_refused() {
    refused(&entry("auth = \"oauth\""), Some("auth"));
}

#[test]
fn an_unknown_key_inside_oauth_is_refused() {
    refused(&entry("[mcp.servers.slack.oauth]\nclientSecret = \"x\""), Some("clientSecret"));
}

#[test]
fn an_entry_with_no_url_is_refused() {
    refused("[mcp.servers.slack]\ndescription = \"d\"\n", None);
}

/// The other domains of a machine file are the machine scope's (read by the scope loader); `parse_machine` ignores them.
#[test]
fn a_top_level_table_other_than_mcp_is_accepted_and_ignored() {
    let servers = parse(&format!("[pr]\ngate = \"local\"\n{}", named("a"))).unwrap();
    assert_eq!(servers.keys().collect::<Vec<_>>(), ["a"]);
    accepted("[pr]\ngate = \"local\"\n");
}

#[test]
fn an_unknown_key_under_mcp_is_refused() {
    refused("[mcp]\nbogus = 1\n", Some("bogus"));
}

// d. names

#[test]
fn an_uppercase_name_is_refused() {
    refused(&named("Slack"), Some("Slack"));
}

#[test]
fn a_name_with_an_underscore_is_refused() {
    refused(&named("slack_x"), Some("slack_x"));
}

#[test]
fn a_name_starting_with_a_digit_is_refused() {
    refused(&named("\"1slack\""), Some("1slack"));
}

#[test]
fn a_33_character_name_is_refused() {
    refused(&named(&format!("s{}", "a".repeat(32))), None);
}

#[test]
fn a_one_character_name_is_accepted() {
    accepted(&named("a"));
}

#[test]
fn a_32_character_name_is_accepted() {
    accepted(&named(&format!("s{}", "a0-".repeat(11).chars().take(31).collect::<String>())));
}

// e. url and callbackUrl

fn url(u: &str) -> String {
    format!("[mcp.servers.a]\nurl = \"{u}\"\n")
}

fn callback(u: &str) -> String {
    format!("{}[mcp.servers.a.oauth]\ncallbackUrl = \"{u}\"\n", url("https://h/mcp"))
}

#[test]
fn plain_http_to_a_remote_host_is_refused() {
    refused(&url("http://example.com/mcp"), Some("url"));
}

#[test]
fn an_ftp_url_is_refused() {
    refused(&url("ftp://x"), Some("url"));
}

#[test]
fn a_url_with_userinfo_is_refused() {
    refused(&url("https://u@h/mcp"), Some("url"));
}

#[test]
fn text_that_is_not_a_url_is_refused() {
    refused(&url("not a url"), Some("url"));
}

#[test]
fn a_url_with_no_host_is_refused() {
    refused(&url("https://"), Some("url"));
}

#[test]
fn an_https_url_is_accepted() {
    accepted(&url("https://mcp.slack.com/mcp"));
}

#[test]
fn http_to_localhost_is_accepted() {
    accepted(&url("http://localhost:8080/mcp"));
}

#[test]
fn http_to_127_0_0_1_is_accepted() {
    accepted(&url("http://127.0.0.1/mcp"));
}

#[test]
fn http_to_ipv6_loopback_is_accepted() {
    accepted(&url("http://[::1]:9/mcp"));
}

#[test]
fn an_https_callback_url_is_refused() {
    refused(&callback("https://localhost/cb"), Some("callbackUrl"));
}

#[test]
fn a_callback_url_to_a_remote_host_is_refused() {
    refused(&callback("http://example.com/cb"), Some("callbackUrl"));
}

#[test]
fn a_localhost_callback_url_is_accepted() {
    accepted(&callback("http://localhost:3118/callback"));
}

// f. exposures and strings

#[test]
fn the_four_exposures_are_accepted_for_exposure() {
    for e in ["codemode", "deferred", "direct", "hidden"] {
        accepted(&entry(&format!("exposure = \"{e}\"")));
    }
}

#[test]
fn the_four_exposures_are_accepted_for_a_tool() {
    for e in ["codemode", "deferred", "direct", "hidden"] {
        accepted(&entry(&format!("[mcp.servers.slack.toolExposure]\nt = \"{e}\"")));
    }
}

#[test]
fn a_bad_exposure_is_refused() {
    for e in ["Direct", "bogus", "codemode-deferred"] {
        refused(&entry(&format!("exposure = \"{e}\"")), Some("exposure"));
    }
}

#[test]
fn a_bad_tool_exposure_value_is_refused() {
    for e in ["Direct", "bogus", "codemode-deferred"] {
        refused(&entry(&format!("[mcp.servers.slack.toolExposure]\nt = \"{e}\"")), Some("toolExposure"));
    }
}

#[test]
fn an_empty_tool_exposure_table_is_refused() {
    refused(&entry("[mcp.servers.slack.toolExposure]"), Some("toolExposure"));
}

#[test]
fn an_empty_tool_name_is_refused() {
    refused(&entry("[mcp.servers.slack.toolExposure]\n\"\" = \"direct\""), Some("toolExposure"));
}

#[test]
fn a_tool_pattern_with_a_star_is_accepted() {
    accepted(&entry("[mcp.servers.slack.toolExposure]\n\"slack_*\" = \"hidden\""));
}

#[test]
fn an_empty_string_is_refused() {
    refused(&entry("description = \"\""), Some("description"));
}

#[test]
fn a_501_character_string_is_refused() {
    refused(&entry(&format!("description = \"{}\"", "a".repeat(501))), Some("description"));
}

#[test]
fn a_1_character_string_is_accepted() {
    accepted(&entry("description = \"a\""));
}

#[test]
fn a_500_character_string_is_accepted() {
    accepted(&entry(&format!("description = \"{}\"", "a".repeat(500))));
}

// g. owner and repo files

#[test]
fn an_owner_file_with_mcp_is_refused_and_points_to_the_machine_file() {
    let err = parse_scope(Path::new("/h/policy/owners/acme.toml"), "[mcp]\n").unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert!(err.message().contains("machine"), "{}", err.message());
    // `mcp` is no domain of the policy.
    assert!(Domain::from_name("mcp").is_none());
}

#[test]
fn a_repo_file_with_mcp_servers_is_refused_and_points_to_the_machine_file() {
    let text = "[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\n";
    let err = parse_scope(Path::new("/h/policy/repos/acme/web.toml"), text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert!(err.message().contains("machine"), "{}", err.message());
}

// h. only this machine's file is read

#[test]
fn another_machines_broken_file_is_no_error() {
    let (_d, home) = home();
    write(&home, "machines/pro.toml", SLACK);
    write(&home, "machines/other.toml", "this is {{ not toml");
    assert_eq!(load_machine(&home, &pro()).unwrap().servers.len(), 1);
}

#[test]
fn an_entry_with_an_invalid_machine_name_is_refused() {
    let (_d, home) = home();
    write(&home, "machines/pro.toml", SLACK);
    write(&home, "machines/Bad_Name.toml", "");
    let err = check_file_names(&home).unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
}

#[test]
fn an_entry_that_is_not_toml_is_refused() {
    let (_d, home) = home();
    write(&home, "machines/pro.toml", SLACK);
    write(&home, "machines/other.json", "{}");
    let err = check_file_names(&home).unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
}
