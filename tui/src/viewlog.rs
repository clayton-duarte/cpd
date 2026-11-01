//! A view's log, `$CPD_HOME/run/tui/<view>.log` (commands.md §17, hs-d363): a view writes nothing to its terminal
//! outside its frame, so what it has to say beyond the frame (and a dialog, when the User must see it) goes here, appended, one timestamped line each.

use std::io::Write;
use std::path::PathBuf;

use cpd_core::home::CpdHome;
use cpd_core::ports::Ports;
use time::format_description::well_known::Rfc3339;

/// The log of one view.
pub struct ViewLog {
    path: PathBuf,
}

impl ViewLog {
    /// The log of `view` in `home`; nothing is created until the first line.
    pub fn of(home: &CpdHome, view: &str) -> ViewLog {
        // Not in `CpdHome` (contract §1.4): the layout's one source of truth lists the files of `run/tui/` there.
        ViewLog {
            path: home.run_dir().join("tui").join(format!("{view}.log")),
        }
    }

    /// Appends `<RFC 3339 time> <line>`; `Err` with the reason when the file cannot be written.
    pub fn append(&self, line: &str) -> Result<(), String> {
        self.append_with(&Ports::current(), line)
    }

    /// [`ViewLog::append`] stamped by the clock of `ports`.
    pub fn append_with(&self, ports: &Ports, line: &str) -> Result<(), String> {
        let at = ports
            .clock
            .utc()
            .format(&Rfc3339)
            .expect("a UTC time is RFC 3339");
        let failed = |e: std::io::Error| format!("cannot write {}: {e}", self.path.display());
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(failed)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(failed)?;
        writeln!(file, "{at} {line}").map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn lines_are_appended_one_timestamped_line_each_and_the_directory_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let log = ViewLog::of(&home, "tree");
        log.append("first").unwrap();
        log.append("second").unwrap();
        let text = std::fs::read_to_string(dir.path().join("run/tui/tree.log")).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        for (line, said) in lines.iter().zip(["first", "second"]) {
            let (at, rest) = line.split_once(' ').unwrap();
            assert!(OffsetDateTime::parse(at, &Rfc3339).is_ok(), "{at}");
            assert_eq!(rest, said);
        }
    }

    #[test]
    fn a_log_that_cannot_be_written_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("run/tui/tree.log")).unwrap();
        let log = ViewLog::of(
            &CpdHome::resolve(Some(dir.path().into()), None).unwrap(),
            "tree",
        );
        assert!(log.append("x").unwrap_err().contains("tree.log"));
    }
}
