//! The `op` calls (commands.md §24 "op calls"). STUBS in this Job: the next Job (`secret-impl-op`) fills them in.
//! Each returns the JSON fields the command adds to its output (`value`, `items`, `created`, `edited`, `archived`).
//! A value is only ever passed in memory (stdin of `op`), never in argv.

use serde_json::{Map, Value};

use crate::error::{CpdError, ErrorCode};

/// The fields an op call adds to the command's output.
pub type Fields = Map<String, Value>;

fn not_implemented() -> CpdError {
    CpdError::new(ErrorCode::Internal, "secret: op calls are not implemented yet")
}

/// `op item get <item> --vault <vault> --fields label=<field> --reveal`: `{value}`.
pub fn get(_vault: &str, _item: &str, _field: &str) -> Result<Fields, CpdError> {
    Err(not_implemented())
}

/// `op item list --vault <vault> --format json`: `{items: [{id, title}]}`.
pub fn list(_vault: &str) -> Result<Fields, CpdError> {
    Err(not_implemented())
}

/// Creates the item with one concealed field: `{created: true}`.
pub fn create(_vault: &str, _item: &str, _field: &str, _value: &str) -> Result<Fields, CpdError> {
    Err(not_implemented())
}

/// Sets (or adds) the field: `{edited: true}`.
pub fn edit(_vault: &str, _item: &str, _field: &str, _value: &str) -> Result<Fields, CpdError> {
    Err(not_implemented())
}

/// Archives the item: `{archived: true}`.
pub fn delete(_vault: &str, _item: &str) -> Result<Fields, CpdError> {
    Err(not_implemented())
}
