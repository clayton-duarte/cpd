//! The cockpit's sizes (the User's) and the reset that applies them. One place: the StatusBar's ⧉ button and
//! `cpd-tui reset-layout` (run by `cockpit.sh`) both call `steps` through `main::reset_layout`.
//!
//! tmux takes `resize-pane -x/-y` with a percentage as a share of the window. The chat takes what is left.

pub const STATUSBAR_ROWS: &str = "1";
pub const CONTEXT_WIDTH: &str = "20%";
pub const DISPATCH_WIDTH: &str = "20%";
pub const PLAN_HEIGHT: &str = "20%";
pub const WORKER_WIDTH: &str = "30%";

/// The ordered `resize-pane` arguments that reset the window, given each pane as `(pane_id, @view)`. The Worker
/// view's step is only there when a Worker view is open. Panes that are not a view (the chat) are never resized.
pub fn steps(panes: &[(String, String)]) -> Vec<Vec<String>> {
    let order = [
        ("statusbar", "-y", STATUSBAR_ROWS),
        ("context", "-x", CONTEXT_WIDTH),
        ("dispatch", "-x", DISPATCH_WIDTH),
        ("plan", "-y", PLAN_HEIGHT),
        ("worker", "-x", WORKER_WIDTH),
    ];
    order
        .iter()
        .filter_map(|(view, flag, size)| {
            let (id, _) = panes.iter().find(|(_, v)| v == view)?;
            Some(["resize-pane", "-t", id, flag, size].map(String::from).to_vec())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes(views: &[&str]) -> Vec<(String, String)> {
        views.iter().enumerate().map(|(i, v)| (format!("%{i}"), v.to_string())).collect()
    }

    fn joined(steps: Vec<Vec<String>>) -> Vec<String> {
        steps.iter().map(|s| s.join(" ")).collect()
    }

    #[test]
    fn the_reset_applies_the_users_sizes_in_order() {
        // %0 context, %1 chat, %2 dispatch, %3 plan, %4 statusbar
        let p = panes(&["context", "", "dispatch", "plan", "statusbar"]);
        assert_eq!(
            joined(steps(&p)),
            ["resize-pane -t %4 -y 1", "resize-pane -t %0 -x 20%", "resize-pane -t %2 -x 20%", "resize-pane -t %3 -y 20%"]
        );
    }

    #[test]
    fn an_open_worker_view_is_resized_last_to_30_percent() {
        let p = panes(&["context", "", "worker", "dispatch", "plan", "statusbar"]);
        let s = joined(steps(&p));
        assert_eq!(s.len(), 5);
        assert_eq!(s[4], "resize-pane -t %2 -x 30%");
    }
}
