//! `pr merge-green` (commands.md §9.10a): stub, not yet implemented.

/// Whether a pull request's cached `checks` value allows the merge protocol to start. Stub: allows everything.
pub fn require_green(_checks: &str) -> Result<(), String> {
    Ok(())
}
