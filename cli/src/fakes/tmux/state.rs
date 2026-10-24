//! Read-only view of the fake's SQLite state. Stub.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowRow {
    pub id: String,
    pub session: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub id: String,
    pub window: String,
    pub session: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub dead: bool,
    pub dead_status: Option<i32>,
    pub options: Vec<(String, String)>,
}

pub struct State;

impl State {
    /// `None` while no server exists.
    pub fn open(_tmux_tmpdir: &Path, _socket: &str) -> Option<State> {
        None
    }
    pub fn panes(&self) -> Vec<Pane> {
        todo!()
    }
    pub fn pane_env(&self, _pane_id: &str) -> Vec<(String, String)> {
        todo!()
    }
    pub fn options(&self) -> Vec<(String, String)> {
        todo!()
    }
    pub fn sessions(&self) -> Vec<SessionRow> {
        todo!()
    }
    pub fn windows(&self) -> Vec<WindowRow> {
        todo!()
    }
}
