//! The search-query sanitiser: rewrites a typed query into the FTS5 `MATCH` string cpd sends.
//! The grammar (rules R1-R10) is `docs/research/context-holographic.md`, `## Search contract`.

/// Stub: the identity (red state).
pub(super) fn sanitise(query: &str) -> String {
    query.to_owned()
}
