//! The Worker indicator at the right of the StatusBar: `workers N/M`.
//!
//! Interim source, like `leads/<id>/workflow`: the files cpd keeps in its home, read directly until cpd2 provides the
//! data. `M` is `concurrency` in `<cpd home>/config.json`; `N` counts the slots `<cpd home>/slots/slot-<i>.json`
//! (`{wf, attempt, pid, at}`, one per running Attempt) whose `pid` is alive (cpd reuses a slot, so a dead pid is stale).

use std::{fs, path::Path};

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde_json::Value;

use crate::palette::{self, Meaning};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workers {
    /// Running now: live slots.
    pub running: usize,
    /// The machine-wide limit; `None` when `config.json` has no readable `concurrency`.
    pub limit: Option<u64>,
}

/// Whether process `pid` exists (Linux: `/proc/<pid>`).
fn alive(pid: u64) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Read the count and the limit from a cpd home.
pub fn read(home: &Path) -> Workers {
    let limit = fs::read_to_string(home.join("config.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["concurrency"].as_u64());
    let running = fs::read_dir(home.join("slots"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("slot-") && e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str::<Value>(&fs::read_to_string(e.path()).ok()?).ok()?["pid"].as_u64())
        .filter(|pid| alive(*pid))
        .count();
    Workers { running, limit }
}

impl Workers {
    /// `workers N/M`, or `N/M` when `width` columns cannot hold the long form; `None` when not even that fits.
    /// `N` is in the "in progress" colour above 0, else "nothing"; `workers` and `/M` are dim.
    pub fn line(&self, width: u16) -> Option<Line<'static>> {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let n = Span::styled(self.running.to_string(), palette::fg(if self.running > 0 { Meaning::InProgress } else { Meaning::Nothing }));
        let limit = Span::styled(format!("/{}", self.limit.map_or("?".to_string(), |m| m.to_string())), dim);
        let short = Line::from(vec![n.clone(), limit.clone()]);
        let long = Line::from(vec![Span::styled("workers ", dim), n, limit]);
        [long, short].into_iter().find(|l| l.width() <= width as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cpd-tui-test-{}-workers-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("slots")).unwrap();
        dir
    }

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// A pid that is not alive: above any pid_max.
    const DEAD: u64 = 4_000_000_000;

    #[test]
    fn live_slots_count_and_stale_ones_do_not() {
        let h = home("count");
        fs::write(h.join("config.json"), r#"{"concurrency": 10, "other": 1}"#).unwrap();
        let me = std::process::id();
        fs::write(h.join("slots/slot-0.json"), format!(r#"{{"wf":"w","attempt":"a","pid":{me},"at":"t"}}"#)).unwrap();
        fs::write(h.join("slots/slot-1.json"), format!(r#"{{"wf":"w","attempt":"b","pid":{DEAD},"at":"t"}}"#)).unwrap();
        fs::write(h.join("slots/slot-2.json"), format!(r#"{{"wf":"w","attempt":"c","pid":{me},"at":"t"}}"#)).unwrap();
        fs::write(h.join("slots/slot-3.json"), "not json").unwrap();
        fs::write(h.join("slots/notes.txt"), format!(r#"{{"pid":{me}}}"#)).unwrap();
        assert_eq!(read(&h), Workers { running: 2, limit: Some(10) });
        fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn no_home_files_is_zero_running_and_no_limit() {
        let h = std::env::temp_dir().join(format!("cpd-tui-test-{}-workers-none", std::process::id()));
        assert_eq!(read(&h), Workers { running: 0, limit: None });
    }

    #[test]
    fn the_long_form_has_a_coloured_count_and_dim_words() {
        let line = Workers { running: 3, limit: Some(10) }.line(40).unwrap();
        assert_eq!(text(&line), "workers 3/10");
        assert_eq!(line.spans[1].style.fg, Some(palette::color(Meaning::InProgress)), "N is in progress above 0");
        assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM) && line.spans[2].style.add_modifier.contains(Modifier::DIM));
        let idle = Workers { running: 0, limit: Some(10) }.line(40).unwrap();
        assert_eq!(idle.spans[1].style.fg, Some(palette::color(Meaning::Nothing)), "N is nothing at 0");
    }

    #[test]
    fn a_narrow_row_shortens_the_indicator_and_a_tiny_one_drops_it() {
        let w = Workers { running: 3, limit: Some(10) };
        assert_eq!(text(&w.line(12).unwrap()), "workers 3/10");
        assert_eq!(text(&w.line(11).unwrap()), "3/10");
        assert_eq!(text(&w.line(4).unwrap()), "3/10");
        assert!(w.line(3).is_none());
        assert_eq!(text(&Workers { running: 0, limit: None }.line(40).unwrap()), "workers 0/?");
    }
}
