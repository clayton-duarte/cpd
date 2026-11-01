//! What `core/tests/migrations.rs` needs from the store and cannot get from its
//! public API (contract §4.5 "The checks"): this binary's named migrations and
//! the database as it was before one of them. Not part of the interface.

use rusqlite::Connection;

use super::Catalog;
use super::schema::NAMED;
use super::verdict::reference_database;
use crate::error::CpdError;
use crate::home::CpdHome;

/// `(name, compat, sql)` of every named migration of this binary, by name;
/// `compat` is `"additive"` or `"breaking"`, as `build.rs` parsed it.
#[doc(hidden)]
pub fn named_migrations() -> Vec<(&'static str, &'static str, &'static str)> {
    NAMED
        .iter()
        .map(|n| (n.name, n.compat.as_str(), n.sql))
        .collect()
}

/// A fresh in-memory database after the legacy set, the runner's table, the
/// first `named_count` named migrations (by name) and the reconcile: the state
/// `NAMED[named_count]` is applied to. `home` is for a migration's Rust part.
#[doc(hidden)]
pub fn prefix_database(named_count: usize, home: &CpdHome) -> Result<Connection, CpdError> {
    reference_database(
        &Catalog {
            named: &NAMED[..named_count],
            schema_hash: "",
        },
        home,
    )
}
