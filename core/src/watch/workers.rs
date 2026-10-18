//! The `@cpd_workers` step (contract §6.1; commands.md §12; User, hs-d149): the
//! styled `workers N/M` text of the StatusBar, which tmux's status line cannot
//! compute. Every tick, `N` is the Attempts not `ended` (the slots of the
//! `widgets` view) and `M` is `concurrency`; `tmux set -g @cpd_workers` runs
//! only when the text differs from the one last set (the first tick always
//! sets it). No row, no Event, except an `error` when tmux fails while a server
//! runs: with no server there is no status line to feed, and the step waits.

use super::engine::Engine;
use super::report::{About, Step};
use crate::error::CpdError;

/// The option the status line draws (`#{@cpd_workers}`).
const OPTION: &str = "@cpd_workers";

/// The Attempts holding a slot (§3.4): the same query as the `widgets` view's slots.
const HELD: &str = "SELECT count(*) FROM attempts WHERE status <> 'ended'";

/// The styled text: `workers ` and `/limit` dim, `running` green above 0, else colour 8.
fn styled(running: u32, limit: u32) -> String {
    let colour = if running > 0 { "green" } else { "colour8" };
    format!("#[dim]workers #[nodim,fg={colour}]{running}#[default,dim]/{limit}#[default]")
}

pub(super) fn run(engine: &mut Engine) {
    let held = engine.store.read(|conn| Ok(conn.query_row(HELD, [], |r| r.get::<_, u32>(0))?));
    let text = match held {
        Ok(held) => styled(held, engine.config.concurrency),
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    if engine.workers.as_deref() == Some(text.as_str()) {
        return;
    }
    // No server, no status line: nothing is set or remembered, and it is set once a server appears.
    match engine.tmux.server_running() {
        Ok(true) => {}
        Ok(false) => return engine.clear(Step::Workers, &About::Machine),
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    }
    let set: Result<String, CpdError> = engine.tmux.run(&["set".to_owned(), "-g".to_owned(), OPTION.to_owned(), text.clone()]);
    match set {
        Ok(_) => {
            engine.workers = Some(text);
            engine.clear(Step::Workers, &About::Machine);
        }
        // Not remembered: the next tick tries again.
        Err(e) => engine.fail(Step::Workers, About::Machine, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::styled;

    #[test]
    fn none_running_is_colour_8() {
        assert_eq!(styled(0, 3), "#[dim]workers #[nodim,fg=colour8]0#[default,dim]/3#[default]");
    }

    #[test]
    fn some_running_is_green() {
        assert_eq!(styled(3, 6), "#[dim]workers #[nodim,fg=green]3#[default,dim]/6#[default]");
    }
}
