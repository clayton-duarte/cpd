//! The Lead sessions running in the cockpit: discovered from tmux pane listings, grouped by Context, labelled.
//!
//! A chat pane is a pane that publishes `@session_id` (tmux-agent-state.ts, or the launch scripts for sessions that
//! started before it). Nothing is configured.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::Path,
    sync::Mutex,
};

use serde_json::Value;

/// The `tmux list-panes -a -F` format that `parse_panes` reads.
pub const PANE_FORMAT: &str = "#{pane_id}\t#{window_name}\t#{@session_id}\t#{@session_file}\t#{@session_name}\t#{@agent_state}\t#{pane_current_path}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub pane_id: String,
    pub window: String,
    pub id: String,
    pub file: String,
    /// The published `@session_name` (may be empty: see `label`).
    pub name: String,
    pub state: String,
    /// The session's project directory: its file's header `cwd`, else the pane's `pane_current_path`.
    pub cwd: String,
}

impl Session {
    /// The session in the centre slot (window `main`).
    pub fn is_active(&self) -> bool {
        self.window == "main"
    }

    /// The Context: until the entity exists, the repo directory name of the session's cwd
    /// (the session file's own, else the pane's).
    pub fn context(&self) -> String {
        Path::new(&self.cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "(no cwd)".into())
    }
}

/// The `cwd` of a session file's header line (its first line only: files can be tens of MB).
/// `Err` when the file cannot be read; `Ok(None)` when the header has no `cwd`.
pub fn header_cwd(path: &str) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    BufReader::new(fs::File::open(path)?).read_line(&mut line)?;
    let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    Ok(v["cwd"].as_str().filter(|c| !c.is_empty()).map(String::from))
}

/// Header cwds by `@session_file`: a file is read once, and again only when a session publishes another path.
/// An unreadable file is not remembered, so a file that appears later is picked up.
static HEADER_CWDS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn cached_header_cwd(path: &str) -> Option<String> {
    let mut guard = HEADER_CWDS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(c) = cache.get(path) {
        return c.clone();
    }
    let cwd = header_cwd(path).ok()?;
    cache.insert(path.into(), cwd.clone());
    cwd
}

/// The chat panes of a pane listing in `PANE_FORMAT`; panes with no `@session_id` are not chats. Malformed lines are skipped.
pub fn parse_panes(listing: &str) -> Vec<Session> {
    listing
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() == 7 && !f[2].is_empty()).then(|| Session {
                pane_id: f[0].into(),
                window: f[1].into(),
                id: f[2].into(),
                file: f[3].into(),
                name: f[4].into(),
                state: f[5].into(),
                cwd: if f[3].is_empty() { None } else { cached_header_cwd(f[3]) }.unwrap_or_else(|| f[6].into()),
            })
        })
        .collect()
}

/// Group sessions under their Context: contexts sorted by name, sessions by id (stable while panes swap around).
pub fn group(mut sessions: Vec<Session>) -> Vec<(String, Vec<Session>)> {
    sessions.sort_by(|a, b| (a.context(), &a.id).cmp(&(b.context(), &b.id)));
    let mut groups: Vec<(String, Vec<Session>)> = Vec::new();
    for s in sessions {
        match groups.last_mut() {
            Some((ctx, list)) if *ctx == s.context() => list.push(s),
            _ => groups.push((s.context(), vec![s])),
        }
    }
    groups
}

/// The label: the pi name, else the Workflow id, else the first 8 characters of the session id.
pub fn label(name: &str, workflow: Option<&str>, id: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.into();
    }
    match workflow {
        Some(w) if !w.trim().is_empty() => w.trim().into(),
        _ => id.chars().take(8).collect(),
    }
}

/// The name of the last `session_info` entry of a session file's text; `None` when there is none or it is empty.
pub fn last_session_name(text: &str) -> Option<String> {
    let entry = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["type"] == "session_info").last()?;
    entry["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from)
}

/// The name to show: the published `@session_name`, else (sessions started before the extension) the file's last `session_info`.
pub fn display_name(s: &Session) -> String {
    if !s.name.trim().is_empty() {
        return s.name.clone();
    }
    fs::read_to_string(&s.file).ok().and_then(|t| last_session_name(&t)).unwrap_or_default()
}

/// The Workflow attached to a session: the one line of `<cpd home>/leads/<id>/workflow`; `None` when unattached.
pub fn workflow_of(cpd_home: &Path, session_id: &str) -> Option<String> {
    let text = fs::read_to_string(cpd_home.join("leads").join(session_id).join("workflow")).ok()?;
    Some(text.lines().next()?.trim().to_string()).filter(|w| !w.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
%0\tmain\t\t\t\t\t/home/u/projects/cpd
%1\tmain\tbbbb2222-x\t/s/b.jsonl\tReviewer\tidle\t/home/u/projects/cpd
%2\tpark-cpd-rust\taaaa1111-x\t/s/a.jsonl\t\tworking\t/home/u/projects/cpd
%3\tpark-other\tcccc3333-x\t\t\t\t/home/u/projects/other
garbage line
%4\tmain\t\t\t\t\t/tmp";

    #[test]
    fn only_panes_with_a_session_id_are_chats() {
        let s = parse_panes(LISTING);
        assert_eq!(s.iter().map(|s| s.pane_id.as_str()).collect::<Vec<_>>(), ["%1", "%2", "%3"]);
        assert_eq!(s[0], Session { pane_id: "%1".into(), window: "main".into(), id: "bbbb2222-x".into(), file: "/s/b.jsonl".into(), name: "Reviewer".into(), state: "idle".into(), cwd: "/home/u/projects/cpd".into() });
        assert!(s[0].is_active() && !s[1].is_active());
    }

    #[test]
    fn the_context_is_the_repo_directory_of_the_cwd() {
        let s = parse_panes(LISTING);
        assert_eq!(s.iter().map(Session::context).collect::<Vec<_>>(), ["cpd", "cpd", "other"]);
    }

    #[test]
    fn sessions_group_under_their_context_in_a_stable_order() {
        let mut s = parse_panes(LISTING);
        s.reverse();
        let g = group(s);
        let shape: Vec<(&str, Vec<&str>)> = g.iter().map(|(c, l)| (c.as_str(), l.iter().map(|s| s.id.as_str()).collect())).collect();
        assert_eq!(shape, [("cpd", vec!["aaaa1111-x", "bbbb2222-x"]), ("other", vec!["cccc3333-x"])]);
    }

    #[test]
    fn the_label_is_the_name_then_the_workflow_then_the_short_id() {
        assert_eq!(label("Reviewer", Some("lead-fork"), "0123456789abcdef"), "Reviewer");
        assert_eq!(label("  ", Some("lead-fork"), "0123456789abcdef"), "lead-fork");
        assert_eq!(label("", None, "0123456789abcdef"), "01234567");
        assert_eq!(label("", Some(" "), "0123456789abcdef"), "01234567");
    }

    #[test]
    fn the_last_session_info_name_wins() {
        let text = r#"{"type":"session","id":"x"}
{"type":"session_info","name":"first"}
{"type":"message","message":{"role":"user"}}
not json
{"type":"session_info","name":" second "}
"#;
        assert_eq!(last_session_name(text), Some("second".into()));
    }

    #[test]
    fn no_session_info_or_a_cleared_name_is_no_name() {
        assert_eq!(last_session_name(r#"{"type":"session","id":"x"}"#), None);
        assert_eq!(last_session_name("{\"type\":\"session_info\",\"name\":\"a\"}\n{\"type\":\"session_info\"}"), None);
        assert_eq!(last_session_name("{\"type\":\"session_info\",\"name\":\"\"}"), None);
    }

    #[test]
    fn the_name_falls_back_to_the_session_file() {
        let dir = std::env::temp_dir().join(format!("cpd-tui-test-{}-name", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        fs::write(&file, "{\"type\":\"session_info\",\"name\":\"from file\"}\n").unwrap();
        let mut s = parse_panes(LISTING).remove(1);
        s.file = file.display().to_string();
        assert_eq!(display_name(&s), "from file");
        s.name = "published".into();
        assert_eq!(display_name(&s), "published");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cpd-tui-test-{}-{tag}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn listing_for(file: &Path) -> String {
        format!("%1\tmain\tid1\t{}\t\tidle\t/tmp", file.display())
    }

    #[test]
    fn the_header_cwd_wins_over_the_pane_cwd() {
        let dir = scratch("hdr");
        let f = dir.join("s.jsonl");
        fs::write(&f, "{\"type\":\"session\",\"id\":\"x\",\"cwd\":\"/home/u/projects/cpd\"}\n").unwrap();
        let s = parse_panes(&listing_for(&f)).remove(0);
        assert_eq!((s.cwd.as_str(), s.context().as_str()), ("/home/u/projects/cpd", "cpd"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_or_a_header_without_cwd_falls_back_to_the_pane_cwd() {
        let dir = scratch("fallback");
        let f = dir.join("none.jsonl");
        assert_eq!(parse_panes(&listing_for(&f)).remove(0).cwd, "/tmp");
        fs::write(&f, "{\"type\":\"session\",\"id\":\"x\"}\n").unwrap();
        assert_eq!(parse_panes(&listing_for(&f)).remove(0).cwd, "/tmp");
        let g = dir.join("empty.jsonl");
        fs::write(&g, "").unwrap();
        assert_eq!(parse_panes(&listing_for(&g)).remove(0).cwd, "/tmp");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_first_line_is_read() {
        let dir = scratch("first");
        let f = dir.join("s.jsonl");
        // A later line with a cwd, and a huge unterminated-JSON tail, must not matter.
        fs::write(&f, "{\"type\":\"session\",\"id\":\"x\"}\n{\"type\":\"x\",\"cwd\":\"/elsewhere\"}\n").unwrap();
        assert_eq!(header_cwd(f.to_str().unwrap()).unwrap(), None);
        fs::write(&f, format!("{{\"cwd\":\"/a/b\"}}\n{}", "x".repeat(1 << 20))).unwrap();
        assert_eq!(header_cwd(f.to_str().unwrap()).unwrap(), Some("/a/b".into()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_is_read_again_only_when_the_path_changes() {
        let dir = scratch("cache");
        let f = dir.join("s.jsonl");
        fs::write(&f, "{\"cwd\":\"/a/one\"}\n").unwrap();
        assert_eq!(parse_panes(&listing_for(&f)).remove(0).cwd, "/a/one");
        fs::write(&f, "{\"cwd\":\"/a/two\"}\n").unwrap();
        assert_eq!(parse_panes(&listing_for(&f)).remove(0).cwd, "/a/one");
        let g = dir.join("t.jsonl");
        fs::write(&g, "{\"cwd\":\"/a/three\"}\n").unwrap();
        assert_eq!(parse_panes(&listing_for(&g)).remove(0).cwd, "/a/three");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_workflow_comes_from_the_leads_record() {
        let home = std::env::temp_dir().join(format!("cpd-tui-test-{}-wf", std::process::id()));
        fs::create_dir_all(home.join("leads/s1")).unwrap();
        fs::write(home.join("leads/s1/workflow"), "lead-fork\n").unwrap();
        fs::create_dir_all(home.join("leads/s2")).unwrap();
        assert_eq!(workflow_of(&home, "s1"), Some("lead-fork".into()));
        assert_eq!(workflow_of(&home, "s2"), None);
        assert_eq!(workflow_of(&home, "s3"), None);
        fs::remove_dir_all(&home).unwrap();
    }
}
