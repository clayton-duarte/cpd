//! The tree's folds survive a restart of the view, in `$CPD_HOME/run/tui/tree-folds.json` (commands.md §13.2,
//! hs-d260): which Contexts have their archived rows hidden (`collapsed`) and which archived Contexts the User
//! expanded (`expanded`), as the closing toggle of each Context folds them (hs-d344). The view's own state, not Ledger
//! state, under the same exception as the Plan view's scroll offsets.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tui_tree_widget::TreeState;

use super::rows::{Id, toggle_id};
use crate::read::Tree;
use cpd_core::home::CpdHome;
use cpd_core::ports::Ports;

/// The folds: the file's content, `{"collapsed": ["<context id>", …], "expanded": [["<id>", …], …]}`,
/// plus the archive row's fold under `"archived"` (absent or false = folded; hs-d698, hs-d707).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folds {
    /// The live Contexts whose archived rows the User hid (live Contexts start expanded).
    collapsed: BTreeSet<Id>,
    /// The archived Contexts the User expanded, each as its one-id path (archived Contexts start folded, hs-d339).
    expanded: BTreeSet<Vec<Id>>,
    /// The tree root's archive row is expanded (the archived Contexts show); folded by default.
    #[serde(default, skip_serializing_if = "is_false")]
    archived: bool,
}

/// Whether the archive fold is the default (folded): skipped when serialising, so an old file loads and the folded
/// case stays byte-identical to today.
fn is_false(flag: &bool) -> bool {
    !*flag
}

impl Folds {
    /// The folds of `state` over `tree`: what differs from each Context's start (open when live, folded when
    /// archived). Only Contexts that exist are kept, so ids that match none are dropped.
    pub fn of(state: &TreeState<Id>, tree: &Tree) -> Folds {
        let opened = state.opened();
        let open = |c: &&crate::read::ContextNode| opened.contains(&vec![toggle_id(c.id.as_str())]);
        let collapsed = tree
            .contexts
            .iter()
            .filter(|c| !c.is_archived() && !open(c))
            .map(|c| c.id.to_string())
            .collect();
        let expanded = tree
            .contexts
            .iter()
            .filter(|c| c.is_archived() && open(c))
            .map(|c| vec![c.id.to_string()])
            .collect();
        let archived = tree
            .contexts
            .iter()
            .any(crate::read::ContextNode::is_archived)
            && opened.contains(&vec![super::rows::ARCHIVE.to_owned()]);
        Folds {
            collapsed,
            expanded,
            archived,
        }
    }

    /// Applies the folds to a state that has opened every live Context: the live rows of a Context always show, so its
    /// toggle is closed when collapsed (its archived rows hide) and opened when expanded, and both count as known.
    /// Ids that match no row are harmless.
    pub fn merge(&self, state: &mut TreeState<Id>, known: &mut HashSet<Vec<Id>>) {
        if self.archived {
            state.open(vec![super::rows::ARCHIVE.to_owned()]);
        }
        let mut apply = |id: &Id, expanded: bool| {
            let context = vec![id.clone()];
            state.open(context.clone());
            let toggle = vec![toggle_id(id)];
            if expanded {
                state.open(toggle)
            } else {
                state.close(&toggle)
            };
            known.insert(context);
        };
        for id in &self.collapsed {
            apply(id, false);
        }
        for path in &self.expanded {
            if let [id] = path.as_slice() {
                apply(id, true);
            }
        }
    }
}

/// The file `$CPD_HOME/run/tui/tree-folds.json`, and what this view knows of it.
pub struct FoldsFile {
    path: PathBuf,
    /// What the file holds, as far as this view knows; a write is due when the folds differ from it.
    saved: Folds,
    /// When the file was last written, or tried.
    written: Option<Instant>,
}

/// A write is due at most this often (a fold that changes more often waits).
const SAVE_EVERY: Duration = Duration::from_secs(1);

impl FoldsFile {
    pub fn at(home: &CpdHome) -> FoldsFile {
        // Not in `CpdHome` (contract §1.4): the layout's one source of truth lists this file there.
        let path = home.run_dir().join("tui").join("tree-folds.json");
        FoldsFile {
            path,
            saved: Folds::default(),
            written: None,
        }
    }

    /// The folds in the file: the defaults when it is missing; `Err` with the reason when it cannot be read or is not
    /// the object above.
    pub fn load(&self) -> Result<Folds, String> {
        let text = match fs::read(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Folds::default()),
            Err(e) => return Err(format!("{}: {e}", self.path.display())),
        };
        let folds: Folds =
            serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", self.path.display()))?;
        match folds.expanded.iter().any(Vec::is_empty) {
            true => Err(format!(
                "{}: an expanded path is empty",
                self.path.display()
            )),
            false => Ok(folds),
        }
    }

    /// Takes `folds` as what the file holds: nothing is written until they change.
    pub fn assume(&mut self, folds: Folds) {
        self.saved = folds;
    }

    /// Writes `folds` when they differ from the file and a second has passed since the last write. A failure is
    /// returned and leaves the folds unsaved, so the next call retries.
    #[cfg(test)]
    pub fn save(&mut self, folds: &Folds) -> Result<(), String> {
        self.save_with(&Ports::current(), folds)
    }

    /// [`FoldsFile::save`] with the second counted on the clock of `ports`.
    pub fn save_with(&mut self, ports: &Ports, folds: &Folds) -> Result<(), String> {
        let now = ports.clock.now();
        if *folds == self.saved
            || self
                .written
                .is_some_and(|at| now.saturating_duration_since(at) < SAVE_EVERY)
        {
            return Ok(());
        }
        self.written = Some(now);
        self.write(folds)
            .map_err(|e| format!("fold state not saved: {}: {e}", self.path.display()))?;
        self.saved.clone_from(folds);
        Ok(())
    }

    /// Replaces the file through [`cpd_core::atomic::write_atomic`].
    fn write(&self, folds: &Folds) -> io::Result<()> {
        let text = serde_json::to_vec(folds).map_err(io::Error::other)?;
        cpd_core::atomic::write_atomic(&self.path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture::archived;
    use super::*;

    fn file_in(dir: &tempfile::TempDir) -> FoldsFile {
        FoldsFile::at(&CpdHome::resolve(Some(dir.path().into()), None).unwrap())
    }

    fn path_of(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("run/tui/tree-folds.json")
    }

    fn folds(collapsed: &[&str], expanded: &[&[&str]]) -> Folds {
        Folds {
            collapsed: collapsed.iter().map(|s| (*s).to_owned()).collect(),
            expanded: expanded
                .iter()
                .map(|p| p.iter().map(|s| (*s).to_owned()).collect())
                .collect(),
            archived: false,
        }
    }

    #[test]
    fn the_folds_round_trip_through_the_file_which_holds_collapsed_and_expanded() {
        let dir = tempfile::tempdir().unwrap();
        let want = folds(&["beta", "alpha"], &[&["beta", "…"], &["…"]]);
        file_in(&dir).save(&want).unwrap();
        assert_eq!(
            fs::read_to_string(path_of(&dir)).unwrap(),
            r#"{"collapsed":["alpha","beta"],"expanded":[["beta","…"],["…"]]}"#
        );
        assert_eq!(file_in(&dir).load().unwrap(), want);
    }

    #[test]
    fn a_missing_file_is_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(file_in(&dir).load().unwrap(), Folds::default());
    }

    #[test]
    fn an_invalid_file_is_refused_with_its_reason_and_replaced_by_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_of(&dir);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bad in [
            "",
            "{not json",
            "[]",
            "{}",
            r#"{"collapsed":["a"]}"#,
            r#"{"collapsed":"a","expanded":[]}"#,
            r#"{"collapsed":[1],"expanded":[]}"#,
            r#"{"collapsed":[],"expanded":["…"]}"#,
            r#"{"collapsed":[],"expanded":[[]]}"#,
            r#"{"collapsed":[],"expanded":[],"cursor":[]}"#,
        ] {
            fs::write(&path, bad).unwrap();
            let reason = file_in(&dir).load().unwrap_err();
            assert!(reason.contains("tree-folds.json"), "{bad:?}: {reason}");
        }
        let mut file = file_in(&dir);
        file.load().unwrap_err();
        file.save(&folds(&["alpha"], &[])).unwrap();
        assert_eq!(file_in(&dir).load().unwrap(), folds(&["alpha"], &[]));
    }

    #[test]
    fn an_unreadable_file_is_refused_with_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(path_of(&dir)).unwrap(); // reading a directory fails
        let reason = file_in(&dir).load().unwrap_err();
        assert!(reason.contains("tree-folds.json"), "{reason}");
    }

    #[test]
    fn a_write_leaves_no_temporary_file_and_creates_the_parents() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = file_in(&dir);
        file.save(&folds(&["alpha"], &[])).unwrap();
        file.written = None;
        file.save(&folds(&["beta"], &[])).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path().join("run/tui"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["tree-folds.json"]);
    }

    #[test]
    fn a_failed_write_is_reported_leaves_no_temporary_file_and_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let tui = dir.path().join("run/tui");
        fs::create_dir_all(tui.join("tree-folds.json")).unwrap(); // a directory cannot be replaced by a file
        let mut file = file_in(&dir);
        let want = folds(&["alpha"], &[]);
        let error = file.save(&want).unwrap_err();
        assert!(error.starts_with("fold state not saved: "), "{error}");
        assert_eq!(
            fs::read_dir(&tui).unwrap().count(),
            1,
            "only the directory is there"
        );
        fs::remove_dir(tui.join("tree-folds.json")).unwrap();
        file.written = None;
        file.save(&want).unwrap();
        assert_eq!(file_in(&dir).load().unwrap(), want);
    }

    #[test]
    fn a_write_waits_a_second_after_the_last_and_unchanged_folds_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = file_in(&dir);
        file.save(&Folds::default()).unwrap();
        assert!(!path_of(&dir).exists(), "nothing changed: nothing written");
        file.save(&folds(&["alpha"], &[])).unwrap();
        file.save(&folds(&["alpha", "beta"], &[])).unwrap();
        assert_eq!(
            file_in(&dir).load().unwrap(),
            folds(&["alpha"], &[]),
            "the second change waits"
        );
        file.written = Some(Instant::now() - SAVE_EVERY);
        file.save(&folds(&["alpha", "beta"], &[])).unwrap();
        assert_eq!(
            file_in(&dir).load().unwrap(),
            folds(&["alpha", "beta"], &[]),
            "a second later it writes"
        );
    }

    #[test]
    fn the_folds_of_a_state_keep_only_rows_that_exist_and_merge_back() {
        let tree = archived();
        let (mut state, mut known) = (TreeState::default(), HashSet::new());
        super::super::rows::open_new(&mut state, &mut known, &tree);
        let stored = folds(
            &["beta", "vanished"],
            &[&["zeta"], &["alpha"], &["gone"], &["alpha", "ship"]],
        );
        stored.merge(&mut state, &mut known);
        let open = |state: &TreeState<Id>, path: &[&str]| {
            state
                .opened()
                .contains(&path.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
        };
        let (alpha, beta, zeta) = (toggle_id("alpha"), toggle_id("beta"), toggle_id("zeta"));
        assert!(!open(&state, &[&beta]), "beta's archived rows are hidden");
        assert!(
            open(&state, &["beta"]),
            "but its Context stays open: its live rows show"
        );
        assert!(open(&state, &[&alpha]), "alpha is expanded");
        assert!(
            open(&state, &["zeta"]) && open(&state, &[&zeta]),
            "the archived zeta is expanded"
        );
        assert!(
            known.contains(&vec!["vanished".to_owned()]),
            "a collapsed Context counts as known"
        );
        assert!(open(&state, &["vanished"]), "and opens when it appears");
        assert_eq!(
            Folds::of(&state, &tree),
            folds(&["beta"], &[&["zeta"]]),
            "unknown ids, a live Context open as it starts and paths below a Context are dropped"
        );
    }
}
