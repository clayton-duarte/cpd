//! `cpd tag` (docs/commands.md §26.1): tags the commit of `origin/main` that last changed the workspace version of
//! `Cargo.toml` with the annotated tag `v<version>` and pushes it. Git runs through `ports.runner`.
//! STUB: the logic is the next Job's.

use std::path::Path;

use crate::ports::Ports;

/// Runs the steps of §26.1 in the git repository `root`. `Ok` is the one `tag-version: ...` line of a success
/// (`tagged v<version> on <sha>` or `v<version> exists; nothing to do`), `Err` the one `tag-version: ...` line of
/// a failure; neither ends in a newline. The streams and the exit code are the command's.
pub fn tag(_ports: &Ports, _root: &Path) -> Result<String, String> {
    todo!("cpd tag")
}

#[cfg(test)]
mod tests;
