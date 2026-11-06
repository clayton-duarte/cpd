//! Model discovery (contract §3.7 E15, §4.4 `swr_models`; commands.md §14.5; Context decisions pro-d13,
//! pro-d17, pro-d20): the helper `pi/lib/models-list.ts` lists the models pi can offer, and this area
//! runs it, validates its answer once at the edge, `swr::source::models` diffs each provider against its row
//! and writes it. The engine's E15 and `cpd models refresh` share all of it.
//!
//! cpd has no provider concept here (pro-d20): no provider name, no key variable, no HTTP client. They are
//! named by the helper's answer, which inherits the caller's environment; no key value is ever formatted into
//! an error, an Event or an output.

mod cache;
pub mod cmd;
mod helper;

pub use cache::{Catalog, Changes, Outcome, Source, Written};
pub use cmd::{Command, run};
pub use helper::{Answer, Entry, Helper, Listed, ProviderFailure};

use crate::error::ErrorCode;

/// Whether `name` is a file-name-safe provider name (contract §1.4): `^[A-Za-z0-9._-]+$`.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// The error code a helper's `models_*` code names; `None` for any other text.
pub fn code_of(text: &str) -> Option<ErrorCode> {
    ErrorCode::ALL
        .iter()
        .copied()
        .find(|c| c.as_str() == text && text.starts_with("models_"))
}
