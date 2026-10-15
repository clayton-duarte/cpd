//! The pure parts of the Context view's session actions: forking a session into a new Plan (`n`) and closing one (`x`).

use std::path::{Path, PathBuf};

use crate::sessions::Session;

/// The directory of `fork-lead.sh` and the other launch scripts, from the binary's own location the way `cockpit.sh`
/// finds the binary: the binary is `tui/target/<profile>/cpd-tui`, the scripts are `tui/scripts`.
pub fn scripts_dir(exe: &Path) -> Option<PathBuf> {
    Some(exe.parent()?.parent()?.parent()?.join("scripts"))
}

/// The tmux window of a fork: `park-<name>`, with every character tmux could read as target syntax (`:` `.` …) or that
/// is not a letter, digit, `-` or `_` replaced by `-`.
pub fn window_name(name: &str) -> String {
    let safe: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    format!("park-{safe}")
}

/// `s` as one word of a `sh` command line.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The shell command of the fork window: `fork-lead.sh <source file> <id> <name>`, with no note file, so pi starts the
/// fork without a first message. The socket is passed on explicitly, because the script publishes to that tmux server.
pub fn fork_command(scripts: &Path, socket: &str, source_file: &str, id: &str, name: &str) -> String {
    let script = scripts.join("fork-lead.sh").display().to_string();
    format!("env CPD_TUI_SOCKET={} {} {} {} {}", quote(socket), quote(&script), quote(source_file), quote(id), quote(name))
}

/// Why session `id` cannot be closed right now, judged on a fresh pane listing; `None` when it can. The centre slot
/// (window `main`) cannot be closed, because that would leave the centre empty. It is asked both when `x` is pressed
/// and when `y` confirms, because the session may have been swapped into the centre in between.
pub fn close_refusal(sessions: &[Session], id: &str) -> Option<&'static str> {
    match sessions.iter().find(|s| s.id == id) {
        None => Some("session is already gone"),
        Some(s) if s.is_active() => Some("switch to another session first"),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::parse_panes;

    #[test]
    fn the_scripts_sit_beside_the_target_directory() {
        assert_eq!(scripts_dir(Path::new("/r/tui/target/release/cpd-tui")), Some(PathBuf::from("/r/tui/scripts")));
        assert_eq!(scripts_dir(Path::new("/r/tui/target/debug/cpd-tui")), Some(PathBuf::from("/r/tui/scripts")));
        assert_eq!(scripts_dir(Path::new("cpd-tui")), None);
    }

    #[test]
    fn the_window_name_is_safe_for_tmux() {
        assert_eq!(window_name("cpd-ui"), "park-cpd-ui");
        assert_eq!(window_name(" my plan "), "park-my-plan");
        assert_eq!(window_name("a:b.c"), "park-a-b-c");
        assert_eq!(window_name("x$@%'\"y"), "park-x-----y");
        assert_eq!(window_name("snake_case"), "park-snake_case");
    }

    #[test]
    fn the_fork_command_passes_the_source_the_id_and_the_name_but_no_note() {
        let c = fork_command(Path::new("/r/tui/scripts"), "cpd", "/s/a b.jsonl", "u-1", "it's");
        assert_eq!(c, r"env CPD_TUI_SOCKET='cpd' '/r/tui/scripts/fork-lead.sh' '/s/a b.jsonl' 'u-1' 'it'\''s'");
    }

    const LISTING: &str = "%1\tmain\tbbbb\t/s/b.jsonl\tB\tidle\t/p/cpd\t\t\t\n%2\tpark-a\taaaa\t/s/a.jsonl\tA\tworking\t/p/cpd\t\t\t";

    #[test]
    fn the_centre_session_cannot_be_closed() {
        assert_eq!(close_refusal(&parse_panes(LISTING), "bbbb"), Some("switch to another session first"));
    }

    #[test]
    fn a_parked_session_can_be_closed_even_when_working() {
        assert_eq!(close_refusal(&parse_panes(LISTING), "aaaa"), None);
    }

    #[test]
    fn a_session_swapped_into_the_centre_since_the_prompt_opened_is_refused() {
        let swapped = "%1\tpark-b\tbbbb\t/s/b.jsonl\tB\tidle\t/p/cpd\t\t\t\n%2\tmain\taaaa\t/s/a.jsonl\tA\tworking\t/p/cpd\t\t\t";
        assert_eq!(close_refusal(&parse_panes(swapped), "aaaa"), Some("switch to another session first"));
        assert_eq!(close_refusal(&parse_panes(swapped), "bbbb"), None);
    }

    #[test]
    fn a_vanished_session_is_refused() {
        assert_eq!(close_refusal(&parse_panes(LISTING), "zzzz"), Some("session is already gone"));
    }
}
