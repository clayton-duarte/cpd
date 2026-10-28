//! Tests of the `[mcp]` table of the machine file (contract §8.3 "MCP servers of the machine scope", "Machine domains").

use super::*;
use crate::error::ErrorCode;
use crate::policy::{parse_scope, Domain};

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
    Path::new("/h/policy/host.toml")
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
    let m = load_machine(&home).unwrap();
    assert_eq!(m.source, MachineSource::Default);
    assert!(m.servers.is_empty());
}

#[test]
fn an_empty_file_means_no_servers_from_default() {
    let (_d, home) = home();
    write(&home, "host.toml", "");
    let m = load_machine(&home).unwrap();
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
fn the_slack_file_loads_from_host_toml_with_its_realpath() {
    let (_d, home) = home();
    write(&home, "host.toml", SLACK);
    let m = load_machine(&home).unwrap();
    let real = std::fs::canonicalize(home.root().join("policy/host.toml")).unwrap();
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
fn an_owner_file_with_mcp_is_refused_and_points_to_host_toml() {
    let err = parse_scope(Path::new("/h/policy/acme/owner.toml"), "[mcp]\n").unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert!(err.message().contains("policy/host.toml"), "{}", err.message());
    // `mcp` is no domain of the policy.
    assert!(Domain::from_name("mcp").is_none());
}

#[test]
fn a_repo_file_with_mcp_servers_is_refused_and_points_to_host_toml() {
    let text = "[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\n";
    let err = parse_scope(Path::new("/h/policy/acme/web.toml"), text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::PolicyInvalid);
    assert!(err.message().contains("policy/host.toml"), "{}", err.message());
}

// h. only host.toml is read

#[test]
fn a_broken_owner_or_repo_file_is_not_parsed_by_load_machine() {
    let (_d, home) = home();
    write(&home, "host.toml", SLACK);
    write(&home, "acme/owner.toml", "this is {{ not toml");
    write(&home, "acme/web.toml", "[mcp.servers.x]\nurl = 1\n");
    assert_eq!(load_machine(&home).unwrap().servers.len(), 1);
}

// ---------- headers (pro-d14) ----------

fn headers(body: &str) -> String {
    entry(&format!("[mcp.servers.slack.headers]\n{body}"))
}

fn json(text: &str) -> serde_json::Value {
    serde_json::to_value(&parse(text).unwrap()["slack"]).unwrap()
}

/// A TOML basic string.
fn q(s: &str) -> String {
    format!("{s:?}")
}

fn with_value(value: &str) -> String {
    headers(&format!("X-Test = {}\n", q(value)))
}

fn with_name(name: &str) -> String {
    headers(&format!("{} = \"v\"\n", q(name)))
}

#[test]
fn headers_the_fullstory_entry_is_accepted_and_kept() {
    let text = "[mcp.servers.fullstory]\nurl = \"https://mcp.fullstory.com/mcp\"\n\n[mcp.servers.fullstory.headers]\nMCP-Protocol-Version = \"2025-06-18\"\n";
    let servers = parse(text).unwrap();
    let h = servers["fullstory"].headers.as_ref().expect("headers kept");
    assert_eq!(h.iter().collect::<Vec<_>>(), [(&"MCP-Protocol-Version".to_owned(), &"2025-06-18".to_owned())]);
    let json = serde_json::to_value(&servers["fullstory"]).unwrap();
    assert_eq!(json["headers"], serde_json::json!({"MCP-Protocol-Version": "2025-06-18"}));
}

#[test]
fn headers_several_pairs_and_a_name_with_digits_and_dashes_are_accepted() {
    let text = headers("MCP-Protocol-Version = \"2025-06-18\"\nX-Api-2-Version = \"7\"\nAccept = \"text/plain\"\n");
    let servers = parse(&text).unwrap();
    let h = servers["slack"].headers.as_ref().expect("headers kept");
    assert_eq!(h.len(), 3);
    assert_eq!(h["X-Api-2-Version"], "7");
    assert_eq!(json(&text)["headers"]["Accept"], "text/plain");
}

#[test]
fn headers_a_left_out_table_stays_out_of_the_json() {
    let servers = parse(&entry("")).unwrap();
    assert!(servers["slack"].headers.is_none());
    assert!(serde_json::to_value(&servers["slack"]).unwrap().get("headers").is_none());
    // Red until `headers` is read: the same entry with a table keeps it.
    assert!(json(&headers("A = \"b\"\n")).get("headers").is_some());
}

#[test]
fn headers_refuse_an_empty_table() {
    // Control: a one-entry table is accepted, so the refusals below are about emptiness, not an unread key.
    accepted(&headers("A = \"b\""));
    refused(&headers(""), Some("headers"));
    refused(&entry("headers = {}"), Some("headers"));
}

#[test]
fn headers_refuse_a_value_that_is_not_a_string() {
    accepted(&with_value("ok"));
    for v in ["1", "true", "[\"a\"]", "{ a = \"b\" }"] {
        refused(&headers(&format!("X-Test = {v}\n")), Some("headers"));
    }
}

#[test]
fn headers_refuse_an_empty_value() {
    accepted(&with_value("x"));
    refused(&with_value(""), Some("headers"));
}

#[test]
fn headers_refuse_a_value_over_500_characters() {
    accepted(&with_value(&"a".repeat(500)));
    refused(&with_value(&"a".repeat(501)), Some("headers"));
}

#[test]
fn headers_refuse_a_nul_cr_or_lf_in_a_value() {
    accepted(&with_value("a b"));
    for c in ["\\u0000", "\\r", "\\n"] {
        refused(&headers(&format!("X-Test = \"a{c}b\"\n")), Some("headers"));
    }
}

#[test]
fn headers_refuse_a_value_with_an_env_reference() {
    accepted(&with_value("plain"));
    for v in ["${TOKEN}", "Bearer ${TOKEN}", "a${B}c"] {
        refused(&with_value(v), Some("headers"));
    }
}

#[test]
fn headers_refuse_a_value_starting_with_a_bang() {
    refused(&with_value("!echo hi"), Some("headers"));
    accepted(&with_value("a!b"));
    accepted(&with_value("cost$5"));
}

#[test]
fn headers_refuse_the_credential_names() {
    accepted(&with_name("X-Authorization"));
    for n in ["Authorization", "authorization", "AUTHORIZATION", "Proxy-Authorization", "Cookie"] {
        refused(&with_name(n), Some("headers"));
    }
}

#[test]
fn headers_refuse_a_name_that_is_not_a_token() {
    accepted(&with_name(&"a".repeat(64)));
    for n in ["", "a b", "a_b", "a:b", &"a".repeat(65)] {
        refused(&with_name(n), Some("headers"));
    }
}

/// The key names of the `mcp` row of the "Machine domains" table of docs/contract.md: the backticked spans that open
/// an item (after `each with: ` or `; `), and the `oauth` keys that follow `optional keys `.
fn contract_mcp_keys() -> std::collections::BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/contract.md");
    let doc = std::fs::read_to_string(path).unwrap();
    let start = doc.find("**Machine domains.**").expect("Machine domains paragraph");
    let mut row = None;
    let mut in_table = false;
    for line in doc[start..].lines() {
        if line.starts_with('|') {
            in_table = true;
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            if cells[0] == "`mcp`" {
                row = Some(cells[1].to_owned());
            }
        } else if in_table {
            break;
        }
    }
    let row = row.expect("the mcp row");
    let parts: Vec<&str> = row.split('`').collect();
    let mut keys = std::collections::BTreeSet::new();
    let mut i = 1;
    while i < parts.len() {
        let before = parts[i - 1];
        if before.ends_with("each with: ") || before.ends_with("; ") {
            keys.insert(parts[i].to_owned());
        } else if before.ends_with("optional keys ") {
            keys.insert(parts[i].to_owned());
            while i + 2 < parts.len() && parts[i + 1] == ", " {
                i += 2;
                keys.insert(parts[i].to_owned());
            }
        }
        i += 2;
    }
    keys
}

#[test]
fn headers_the_contract_table_lists_exactly_the_keys_the_parser_accepts() {
    // key name -> (parent table, a valid TOML line); `oauth` is a table of its own.
    let sample: BTreeMap<&str, (&str, &str)> = BTreeMap::from([
        ("url", ("", "url = \"https://mcp.fullstory.com/mcp\"")),
        ("description", ("", "description = \"d\"")),
        ("exposure", ("", "exposure = \"direct\"")),
        ("toolExposure", ("", "toolExposure = { a_tool = \"hidden\" }")),
        ("headers", ("", "headers = { MCP-Protocol-Version = \"2025-06-18\" }")),
        ("oauth", ("", "")),
        ("clientId", ("oauth", "clientId = \"c\"")),
        ("scope", ("oauth", "scope = \"s\"")),
        ("callbackUrl", ("oauth", "callbackUrl = \"http://localhost:3118/callback\"")),
        ("clientName", ("oauth", "clientName = \"Claude Code\"")),
    ]);
    let documented = contract_mcp_keys();
    let sampled: std::collections::BTreeSet<String> = sample.keys().map(|k| (*k).to_owned()).collect();
    assert_eq!(documented, sampled, "the contract row and the sample keys differ");

    let lines = |parent: &str| sample.values().filter(|(p, l)| *p == parent && !l.is_empty()).map(|(_, l)| *l).collect::<Vec<_>>().join("\n");
    let text = format!("[mcp.servers.x]\n{}\n\n[mcp.servers.x.oauth]\n{}\n", lines(""), lines("oauth"));
    let servers = parse(&text).unwrap_or_else(|e| panic!("an entry with every documented key is refused: {e:?}\n{text}"));
    let json = serde_json::to_value(&servers["x"]).unwrap();
    for key in ["url", "description", "exposure", "toolExposure", "oauth", "headers"] {
        assert!(json.get(key).is_some(), "{key} not kept: {json}");
    }
}

// ---------- oauth.clientName (pro-d17, D20) ----------

const FIGMA: &str = "[mcp.servers.figma]\nurl = \"https://mcp.figma.com/mcp\"\nexposure = \"deferred\"\n\n[mcp.servers.figma.oauth]\nclientName = \"Claude Code\"\n";

fn oauth(body: &str) -> String {
    entry(&format!("[mcp.servers.slack.oauth]\n{body}"))
}

fn name_is(value: &str) -> String {
    oauth(&format!("clientName = {}\n", q(value)))
}

/// The control of every test: a valid `clientName` is accepted (red until the key is read).
fn control() {
    accepted(&name_is("Claude Code"));
}

#[test]
fn client_name_the_figma_entry_is_accepted_and_kept() {
    let servers = parse(FIGMA).unwrap_or_else(|e| panic!("refused: {e:?}"));
    let json = serde_json::to_value(&servers["figma"]).unwrap();
    assert_eq!(json["oauth"], serde_json::json!({"clientName": "Claude Code"}));
    assert_eq!(json["url"], "https://mcp.figma.com/mcp");
    assert_eq!(json["exposure"], "deferred");
}

#[test]
fn client_name_is_kept_beside_the_other_oauth_keys() {
    let text = oauth("clientId = \"c\"\ncallbackUrl = \"http://localhost:3118/callback\"\nscope = \"s\"\nclientName = \"Claude Code\"\n");
    assert_eq!(
        json(&text)["oauth"],
        serde_json::json!({"clientId": "c", "callbackUrl": "http://localhost:3118/callback", "scope": "s", "clientName": "Claude Code"})
    );
}

#[test]
fn client_name_a_left_out_key_stays_out_of_the_json() {
    assert!(json(&oauth("clientId = \"c\"\n"))["oauth"].get("clientName").is_none());
    // Control, red until the key is read.
    assert_eq!(json(&oauth("clientId = \"c\"\nclientName = \"n\"\n"))["oauth"]["clientName"], "n");
}

#[test]
fn client_name_of_one_and_of_500_characters_is_accepted() {
    control();
    for n in [1, 500] {
        let name = "a".repeat(n);
        assert_eq!(json(&name_is(&name))["oauth"]["clientName"], name.as_str());
    }
}

#[test]
fn client_name_is_passed_unchanged() {
    control();
    let name = "  Cläude  Code ";
    assert_eq!(json(&name_is(name))["oauth"]["clientName"], name);
}

#[test]
fn client_name_refuse_an_empty_string() {
    control();
    refused(&name_is(""), Some("clientName"));
}

#[test]
fn client_name_refuse_a_string_over_500_characters() {
    control();
    refused(&name_is(&"a".repeat(501)), Some("clientName"));
}

#[test]
fn client_name_refuse_a_nul() {
    control();
    refused(&oauth("clientName = \"a\\u0000b\"\n"), Some("clientName"));
}

#[test]
fn client_name_refuse_a_value_that_is_not_a_string() {
    control();
    for v in ["1", "true", "[\"a\"]", "{ a = \"b\" }"] {
        refused(&oauth(&format!("clientName = {v}\n")), Some("clientName"));
    }
}

#[test]
fn client_name_the_client_secret_and_client_registration_stay_refused() {
    accepted(&oauth("clientName = \"Claude Code\"\n"));
    refused(&oauth("clientName = \"Claude Code\"\nclientSecret = \"x\"\n"), Some("clientSecret"));
    refused(&oauth("clientName = \"Claude Code\"\nclientRegistration = \"cimd\"\n"), Some("clientRegistration"));
}
