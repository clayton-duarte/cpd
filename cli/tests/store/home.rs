//! A plain temp `$CPD_HOME` for the store tests that call the library in-process: a `tempfile` directory, no
//! reaper, no tmux, no child process, no `sqlite3` CLI. The tests that still run `cpd` use `TestEnv`.

use std::path::PathBuf;

use cpd_core::home::CpdHome;
use cpd_core::store::Store;

/// What the store tests' helpers need of a home: where `$CPD_HOME` is. Implemented by [`Home`] and by `TestEnv`
/// (the tests that still need its commands), so one set of helpers serves both.
pub trait HasCpdHome {
    fn cpd_home(&self) -> PathBuf;
}

impl HasCpdHome for crate::common::TestEnv {
    fn cpd_home(&self) -> PathBuf {
        crate::common::TestEnv::cpd_home(self)
    }
}

/// A temp directory holding `cpd-home/`; removed on drop.
pub struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    /// A home with no store: the first open creates it.
    pub fn empty() -> Home {
        let dir = tempfile::Builder::new()
            .prefix("cpd-store-test-")
            .tempdir()
            .expect("a temp dir");
        std::fs::create_dir_all(dir.path().join("cpd-home")).expect("create cpd-home");
        Home { dir }
    }

    /// A home whose store is migrated and closed (what `TestEnv::new` copies in).
    pub fn new() -> Home {
        let home = Home::empty();
        let resolved = CpdHome::resolve(Some(home.cpd_home().into()), None).unwrap();
        drop(Store::open(&resolved, 5000).expect("migrate the store"));
        home
    }

    /// The temporary directory everything lives in.
    pub fn root(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl HasCpdHome for Home {
    fn cpd_home(&self) -> PathBuf {
        self.dir.path().join("cpd-home")
    }
}

/// pro-d361: the marker a Lead's `/quit` leaves is one file per Session under `run/quit/`.
#[test]
fn the_quit_marker_path_is_under_run_quit_session_id() {
    let home = Home::empty();
    let resolved = CpdHome::resolve(Some(home.cpd_home().into()), None).unwrap();
    let session = cpd_core::ids::SessionId::parse("lead2").unwrap();
    assert_eq!(
        resolved.quit_marker(&session),
        home.cpd_home().join("run").join("quit").join("lead2")
    );
}
