//! The status line's junctions (contract §6.1, hs-d575 b, hs-d577): a `┸` at each column where a vertical pane border
//! meets the line. Each test lays panes out on a private server, has tmux expand `STATUS_FORMAT` for a client of a given
//! width and reads the row back as cells (or as the raw expansion); nothing here names how the fill is spelled.

use super::palette::{cells, show, Cell, Server};
use super::*;

const J: char = '\u{2538}';
const HEIGHT: &str = "20";

/// The standing option values of the research's worked examples (the right group is 22 cells long, 14 short).
fn standing(server: &Server, preview: Option<&str>) {
    for (option, value) in [
        ("@cpd_workers", "workers 1/4"),
        ("@cpd_workers_width", "13"),
        ("@cpd_workers_short", "1/4"),
        ("@cpd_workers_short_width", "5"),
        ("@cpd_version", "0.9.1"),
    ] {
        server.run(&["set", "-g", option, value]);
    }
    if let Some(p) = preview {
        server.run(&["set", "-g", "@cpd_preview", p]);
    }
}

/// A window `width` cells wide of one pane.
fn one_pane(width: usize, preview: Option<&str>) -> Server {
    let server = Server::new(1);
    server.run(&["resize-window", "-x", &width.to_string(), "-y", HEIGHT]);
    standing(&server, preview);
    server
}

/// Two panes side by side in a window `width` wide: `%0` of width `left` (its border at column `left`), `%1` at the right edge.
fn two_panes(width: usize, left: usize, preview: Option<&str>) -> Server {
    let server = one_pane(width, preview);
    server.run(&["split-window", "-d", "-h", "-t", "%0", "-l", &(width - left - 1).to_string(), "sleep 600"]);
    server
}

/// The cockpit's layout in its creation order: `%0` tree (width `tree`), `%1` plan, `%2` widgets (width `widgets`), `%3` chat below the plan.
fn cockpit(width: usize, tree: usize, widgets: usize) -> Server {
    let server = one_pane(width, None);
    server.run(&["split-window", "-d", "-h", "-t", "%0", "-l", &(width - tree - 1).to_string(), "sleep 600"]);
    server.run(&["split-window", "-d", "-h", "-t", "%1", "-l", &widgets.to_string(), "sleep 600"]);
    server.run(&["split-window", "-d", "-v", "-t", "%1", "sleep 600"]);
    server
}

fn junctions(row: &[Cell]) -> Vec<usize> {
    row.iter().enumerate().filter(|(_, c)| c.ch == J).map(|(i, _)| i).collect()
}

fn key(row: &[Cell]) -> Vec<(char, Option<u8>, Option<u8>, bool)> {
    row.iter().map(|c| (c.ch, c.fg, c.bg, c.reverse)).collect()
}

fn raw(server: &Server, width: usize) -> String {
    let out = server.run(&["display-message", "-p", "-t", "%0", "-F", &STATUS_FORMAT.replace("#{client_width}", &width.to_string())]);
    out.strip_suffix('\n').unwrap_or(&out).to_string()
}

#[track_caller]
fn assert_junctions(row: &[Cell], want: &[usize], what: &str) {
    assert_eq!(junctions(row), want, "{what}: {}", show(row));
}

#[test]
fn junction_is_drawn_at_each_bottom_pane_border_of_the_cockpit_layout() {
    let wide = cockpit(140, 34, 33);
    assert_junctions(&wide.status(140), &[34, 106], "140 columns");
    let narrow = cockpit(100, 20, 20);
    assert_junctions(&narrow.status(100), &[20], "100 columns (the second border is under the workers pill)");
}

#[test]
fn junction_is_not_drawn_under_the_left_group_or_the_preview_pill() {
    for (preview, drawn_from) in [(None, 12), (Some("abc"), 27)] {
        for w in [3, 5, 6, 9, 10, 11, 12, 13, 14, 15, 26, 27, 28] {
            let row = two_panes(100, w, preview).status(100);
            let want: Vec<usize> = if w >= drawn_from { vec![w] } else { vec![] };
            assert_junctions(&row, &want, &format!("width {w}, preview {preview:?}"));
        }
    }
}

#[test]
fn junction_is_not_drawn_under_the_right_group_in_either_form() {
    for (w, c) in [(23, 76), (22, 77), (21, 78), (20, 79)] {
        let row = two_panes(100, c, None).status(100);
        let want: Vec<usize> = if w >= 22 { vec![c] } else { vec![] };
        assert_junctions(&row, &want, &format!("long form, widgets width {w}"));
    }
    for (tree, want) in [(19, vec![19]), (20, vec![])] {
        let row = two_panes(34, tree, None).status(34);
        assert_junctions(&row, &want, &format!("short form, tree width {tree}"));
    }
}

#[test]
fn junction_row_keeps_the_clients_width() {
    for width in [60, 100, 140] {
        let row = two_panes(width, 20, None).status(width);
        assert!(!junctions(&row).is_empty(), "a junction at {width}: {}", show(&row));
        assert_eq!(row.len(), width, "the row is {width} cells: {}", show(&row));
        assert_eq!(one_pane(width, None).status(width).len(), width);
    }
}

#[test]
fn junction_follows_a_resize_of_a_pane() {
    let server = two_panes(100, 50, None);
    for x in [25, 40] {
        server.run(&["resize-pane", "-t", "%0", "-x", &x.to_string()]);
        assert_junctions(&server.status(100), &[x], &format!("after resizing the tree to {x}"));
    }
}

#[test]
fn junction_is_not_drawn_in_a_zoomed_window() {
    let server = cockpit(140, 34, 33);
    assert_junctions(&server.status(140), &[34, 106], "unzoomed");
    server.run(&["resize-pane", "-Z", "-t", "%0"]);
    let zoomed = server.status(140);
    assert_junctions(&zoomed, &[], "zoomed");
    assert_eq!(key(&zoomed), key(&one_pane(140, None).status(140)), "a zoomed window has the plain fill");
    server.run(&["resize-pane", "-Z", "-t", "%0"]);
    assert_junctions(&server.status(140), &[34, 106], "unzoomed again");
}

#[test]
fn junction_is_colour_8_and_no_other_cell_of_the_row_changes() {
    let row = cockpit(140, 34, 33).status(140);
    assert!(!junctions(&row).is_empty(), "junctions are drawn: {}", show(&row));
    let mut plain = row.clone();
    for c in plain.iter_mut().filter(|c| c.ch == J) {
        assert_eq!(c.fg, Some(8), "a junction is colour 8");
        c.ch = '\u{2500}';
    }
    assert_eq!(key(&plain), key(&one_pane(140, None).status(140)), "no other cell changes");
}

#[test]
fn junction_is_outside_every_click_range() {
    let server = cockpit(140, 34, 33);
    let text = raw(&server, 140);
    assert!(text.contains(J), "junctions are drawn: {text}");
    let mut rest = text.as_str();
    while let Some(at) = rest.find("#[range=user|") {
        let end = rest[at..].find("#[norange]").expect("an open range") + at;
        assert!(!rest[at..end].contains(J), "a junction inside a range: {text}");
        rest = &rest[end..];
    }
    let plain = raw(&one_pane(140, None), 140);
    for name in ["reset", "legend", "logs"] {
        let span = |t: &str| {
            let from = t.find(&format!("#[range=user|{name}]")).unwrap();
            t[from..from + t[from..].find("#[norange]").unwrap()].to_string()
        };
        assert_eq!(span(&text), span(&plain), "the {name} button is unchanged");
    }
}

#[test]
fn junction_follows_the_panes_left_to_right_after_a_layout_change() {
    let server = Server::new(4);
    server.run(&["resize-window", "-x", "140", "-y", HEIGHT]);
    standing(&server, None);
    server.run(&["select-layout", "-t", "%0", "even-horizontal"]);
    let listed = server.run(&["list-panes", "-t", "%0", "-F", "#{pane_index} #{pane_right}"]);
    let mut rights: Vec<(usize, usize)> = listed
        .lines()
        .map(|l| {
            let (i, r) = l.split_once(' ').unwrap();
            (i.parse().unwrap(), r.parse().unwrap())
        })
        .collect();
    rights.sort();
    let want: Vec<usize> = rights.iter().take(3).map(|(_, r)| r + 1).collect();
    assert_junctions(&server.status(140), &want, "four panes side by side");
}
