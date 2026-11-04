//! The in-process fakes of tmux and git (ADR 0013): routes of the [`FakeRunner`](crate::host::runner::FakeRunner),
//! chosen by the program's file name. Compiled only with the cargo feature `fakes`.

pub mod classifier;
pub mod embedding;
pub mod git;
pub mod judge;
pub mod tmux;
