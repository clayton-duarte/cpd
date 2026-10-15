//! Reset-layout (commands.md §13.2): the cockpit's sizes, and the `resize-pane` calls that apply them. Not a CLI
//! command: the StatusBar's ⧉ button runs [`reset`] in-process, and `cpd2 tui` runs it once after building a new
//! `main`.
//!
//! tmux takes `resize-pane -x/-y` with a percentage as a share of the window. The chat slot is never resized: it
//! takes what is left.

use crate::error::CpdError;
use crate::host::layout::MAIN_WINDOW;
use crate::host::{PaneId, Tmux, View};

/// The views resized, in order, with the `resize-pane` flag and size of each.
const ORDER: [(View, &str, &str); 5] = [
    (View::Statusbar, "-y", "1"),
    (View::Tree, "-x", "20%"),
    (View::Widgets, "-x", "20%"),
    (View::Plan, "-y", "20%"),
    (View::Worker, "-x", "30%"),
];

/// The ordered `resize-pane` arguments that reset the window, given each pane as `(pane, view)`. The Worker's step
/// is only there when a Worker pane is open.
pub(crate) fn steps(panes: &[(PaneId, View)]) -> Vec<Vec<String>> {
    ORDER
        .iter()
        .filter_map(|(view, flag, size)| {
            let (id, _) = panes.iter().find(|(_, v)| v == view)?;
            Some(["resize-pane", "-t", id.as_str(), flag, size].map(String::from).to_vec())
        })
        .collect()
}

/// Resets the layout of `<cockpit>:main`.
pub(crate) fn reset(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let panes: Vec<(PaneId, View)> = tmux
        .list_panes()?
        .into_iter()
        .filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW)
        .filter_map(|p| Some((p.pane, p.cpd_view?)))
        .collect();
    for step in steps(&panes) {
        tmux.run(&step)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes(views: &[Option<View>]) -> Vec<(PaneId, View)> {
        views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| Some((PaneId::parse(&format!("%{i}"))?, (*v)?)))
            .collect()
    }

    fn joined(steps: Vec<Vec<String>>) -> Vec<String> {
        steps.iter().map(|s| s.join(" ")).collect()
    }

    #[test]
    fn the_reset_applies_the_sizes_in_order() {
        // %0 tree, %1 chat, %2 widgets, %3 plan, %4 statusbar
        let p = panes(&[Some(View::Tree), Some(View::Chat), Some(View::Widgets), Some(View::Plan), Some(View::Statusbar)]);
        assert_eq!(
            joined(steps(&p)),
            ["resize-pane -t %4 -y 1", "resize-pane -t %0 -x 20%", "resize-pane -t %2 -x 20%", "resize-pane -t %3 -y 20%"]
        );
    }

    #[test]
    fn an_open_worker_pane_is_resized_last_to_30_percent() {
        let p = panes(&[
            Some(View::Tree),
            Some(View::Chat),
            Some(View::Worker),
            Some(View::Widgets),
            Some(View::Plan),
            Some(View::Statusbar),
        ]);
        let s = joined(steps(&p));
        assert_eq!(s.len(), 5);
        assert_eq!(s[4], "resize-pane -t %2 -x 30%");
    }
}
