//! `context decide`, `note`, `ask` and `answer` (commands.md §3.4, §3.5): one
//! record appended through [`Contexts::write`].

use serde_json::json;

use super::{By, Contexts, Draft, Select};
use crate::args::Text;
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::SessionId;
use crate::output::Output;

/// `CPD_SESSION`, else `PI_SESSION_ID`, else none (commands.md §3.4).
fn session_from_env() -> Result<Option<SessionId>, CpdError> {
    ["CPD_SESSION", "PI_SESSION_ID"]
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(|v| SessionId::parse(&v.to_string_lossy()))
        .transpose()
}

/// `context decide`, `note`, `ask` and `answer` (§3.4, §3.5).
pub(super) fn write(
    command: &str,
    select: &Select,
    text: &Text,
    draft: Draft,
) -> Result<Output, CpdError> {
    let Env {
        config, mut store, ..
    } = Env::open()?;
    let context = select.resolve(&mut store)?;
    let by = By {
        session: session_from_env()?,
        plan: select.plan.plan.clone(),
    };
    let written = Contexts::new(&config, &mut store).write(&context, command, by, text, draft)?;
    let mut out = json!({"record": written.record.to_json(), "seq": written.seq});
    if written.already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}
