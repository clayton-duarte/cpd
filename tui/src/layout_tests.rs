//! The layout test: the box spacing rule (hs-d578, hs-d476) pinned per container, by the cells of a drawn buffer.
//!
//! A subject draws one fixture through its production path into a [`Sample`]; [`check`] reads `(symbol, style)` cells,
//! row by row, for every [`Placed`] box. The area files (`layout_dialog_tests`, `layout_popup_tests`,
//! `layout_tree_tests`) hold the rows (one `#[test]` per subject). No snapshot: assertions only.

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use crate::box_widget::spacing;

/// The left cap of a pill (U+E0B6).
pub(crate) const PILL_LEFT: &str = "\u{E0B6}";
/// The right cap of a pill (U+E0B4).
pub(crate) const PILL_RIGHT: &str = "\u{E0B4}";
/// Every glyph of a wall: no row of a box too narrow to draw holds one (B3).
const WALL_GLYPHS: [&str; 7] = ["╭", "╮", "╰", "╯", "│", "─", "├"];

/// How a box is walled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Rounded walls all round.
    Full,
    /// A left rail with a closing row, a blank row above and one below inside the rect.
    Rail,
    /// Rounded walls all round, the title is text (a Workflow's `api · repo 󱦰 main · dispatch`), not a pill: (8) does not apply.
    FullText,
    /// A section title (`SectionTitle`): its rect is the three rows; the rows above and below the title row are blank and the
    /// title row starts one column in (the margin) with `──`, then the pill's left cap when a pill fits.
    Rule,
}

/// Where the pane's window sits on a scrolling canvas (the Plan view).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scroll {
    /// At (0, 0): the canvas keeps its margin columns, so they are asserted.
    Corner,
    /// Elsewhere: a scrolled canvas reaches the pane's edges by design, so the margin columns are not asserted.
    Middle,
}

/// The fixture the Plan pane's sample draws, at a scroll offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanCase {
    /// One Workflow with no Jobs.
    Empty(Scroll),
    /// Three Jobs in two Workflows: a Workflow box next to another.
    Three(Scroll),
}

/// Which assertion a named exemption lifts on its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// The cell after the wall is blank (the rows whose glyph sits against the rail by record: hs-d337, hs-d339, hs-d484).
    Padding,
    /// The row under the top border and the row above the bottom border are not blank (a spacing row, hs-d578).
    Vertical,
    /// The box is below `MIN_WIDTH` and still draws (B3 does not apply): the Context fold is not the widget, it stays
    /// on the `fold::*` line builders (research section 2); only the margin and "nothing outside" are asserted.
    Minimum,
    /// The pane clips the box (the canvas scrolls): only (7), nothing outside the container, is asserted of it.
    Clipped,
    /// The row's text runs to the box's last column: the tree's rows end at the pane's inner edge (pro-d186), so the
    /// rail's blank last column (2) and its right padding (4) are not asserted of it.
    RightEdge,
}

/// A row that keeps a recorded design, by name: the name carries the record id.
#[derive(Clone, Debug)]
pub(crate) struct Exempt {
    #[allow(dead_code)] // the record id names the row for the reader of the sample; no check reads it
    pub name: &'static str,
    /// The row in the sample's buffer.
    pub row: u16,
    pub rule: Rule,
}

/// A box as drawn: its wall rect (taken from the Rect the view saved for hit-testing, or from `geometry`).
#[derive(Clone, Debug)]
pub(crate) struct Placed {
    pub name: &'static str,
    pub kind: Kind,
    pub rect: Rect,
    pub exempt: Vec<Exempt>,
}

/// What a subject drew: the buffer, what its container passed (the pane, or for a dialog the client its popup sits
/// in) and the boxes in it.
pub(crate) struct Sample {
    pub buf: Buffer,
    pub container: Rect,
    pub boxes: Vec<Placed>,
}

/// Where a right click falls in a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Click {
    First,
    Last,
    Middle,
}

impl Click {
    /// The cell clicked in a `width` x `height` pane.
    pub(crate) fn at(self, width: u16, height: u16) -> (u16, u16) {
        match self {
            Click::First => (0, 0),
            Click::Last => (width - 1, height - 1),
            Click::Middle => (width / 2, height / 2),
        }
    }
}

/// The fixture a subject draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Case {
    /// The `Archive` question.
    Confirm,
    /// An input dialog, its text empty, of one row, wrapped to three rows.
    InputEmpty,
    InputOne,
    InputWrapped,
    /// Read-only text.
    Text,
    /// Two entries to tick.
    Checklist,
    /// A right-click menu opened at a click.
    Menu(Click),
}

/// The fixture the tree pane's samples draw (the Context fold).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeCase {
    /// The tree's Contexts: live ones with Plans that have counters, and an archived one.
    ContextFold,
}

/// The fixture the section samples draw: the title's three rows, or the body rows under it (no box, the container's margin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionCase {
    /// The section title, placed as a `Kind::Rule`.
    Title,
    /// The rows under the title: no box, only the margin and "nothing outside".
    Rows,
}

/// A cell with nothing in it, whatever its style.
pub(crate) fn blank(cell: &Cell) -> bool {
    cell.symbol().trim().is_empty()
}

/// A cell with nothing in it and no style: what a margin column holds.
fn plain(cell: &Cell) -> bool {
    let d = Cell::default();
    blank(cell) && cell.fg == d.fg && cell.bg == d.bg && cell.modifier == d.modifier
}

/// A cell that shows something: a glyph, or a block (a cursor, a selection).
fn inked(cell: &Cell) -> bool {
    !blank(cell) || cell.modifier.contains(Modifier::REVERSED) || cell.bg != Cell::default().bg
}

/// The rows of `buf` as text, for a message.
pub(crate) fn dump(buf: &Buffer) -> String {
    (buf.area.y..buf.area.bottom())
        .map(|y| format!("{y:>2}|{}|", (buf.area.x..buf.area.right()).map(|x| buf[(x, y)].symbol()).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The symbol at `(x, y)`.
pub(crate) fn sym(buf: &Buffer, x: u16, y: u16) -> &str {
    buf[(x, y)].symbol()
}

fn exempted(placed: &Placed, row: u16, rule: Rule) -> bool {
    placed.exempt.iter().any(|e| e.row == row && e.rule == rule)
}

/// Every assertion of the layout test on `sample`, drawn at `width`:
/// (1) margin, (2) walls, (3) padding, (4) content, (5) vertical, (6) hs-d476, (7) nothing outside the container,
/// (8) title, (9) B3 (no half boxes). Panics with `subject`, `width` and the cell.
pub(crate) fn check(subject: &str, width: u16, sample: &Sample) {
    check_at(subject, width, sample, Scroll::Corner);
}

/// [`check`] for a pane whose canvas is at `scroll`.
pub(crate) fn check_at(subject: &str, width: u16, sample: &Sample, scroll: Scroll) {
    let Sample { buf, container, boxes } = sample;
    let fail = |x: u16, y: u16, what: &str| {
        panic!("{subject} at width {width}: {what} at cell ({x}, {y}) = {:?}\n{}", sym(buf, x, y), dump(buf))
    };
    // (1) the margin columns of the container are blank and unstyled on every row, boxes or not. At the canvas's corner
    // only: a scrolled canvas reaches the pane's edges by design, and so does a box the pane clips on the right.
    let clipped = |p: &Placed| p.exempt.iter().any(|e| e.rule == Rule::Clipped);
    let clips = boxes.iter().any(clipped);
    for y in container.y..container.bottom() {
        for dx in 0..spacing::MARGIN_X {
            for x in [container.x + dx, container.right() - 1 - dx] {
                let right = x != container.x + dx;
                if scroll == Scroll::Middle || (right && clips) {
                    continue;
                }
                if !plain(&buf[(x, y)]) {
                    fail(x, y, "(1) the margin column is not blank and unstyled");
                }
            }
        }
    }
    // (7) nothing is written outside the container
    for y in buf.area.y..buf.area.bottom() {
        for x in buf.area.x..buf.area.right() {
            if !container.contains(ratatui::layout::Position::new(x, y)) && !plain(&buf[(x, y)]) {
                fail(x, y, "(7) written outside the container");
            }
        }
    }
    for placed in boxes {
        let r = placed.rect;
        if r.width == 0 || r.height == 0 {
            continue;
        }
        let name = placed.name;
        if placed.kind == Kind::Rule {
            check_rule(placed, buf, &fail);
            continue;
        }
        if clipped(placed) {
            continue;
        }
        if scroll == Scroll::Corner && r.x < container.x + spacing::MARGIN_X || r.right() + spacing::MARGIN_X > container.right() {
            fail(r.x, r.y, &format!("(1) box {name} {r:?} keeps less than the margin from the container {container:?}"));
        }
        let min_rows = if placed.kind == Kind::Rail { 4 } else { 2 };
        if r.width < spacing::MIN_WIDTH && placed.exempt.iter().any(|e| e.rule == Rule::Minimum) {
            continue;
        }
        if r.width < spacing::MIN_WIDTH || r.height < min_rows {
            // (9) B3: below the minimum the box draws nothing, no half box
            for y in r.y..r.bottom() {
                for x in r.x..r.right() {
                    if WALL_GLYPHS.contains(&sym(buf, x, y)) {
                        fail(x, y, &format!("(9) box {name} {r:?} is below the minimum and draws a wall glyph"));
                    }
                }
            }
            continue;
        }
        check_box(placed, buf, &fail);
    }
}

fn check_box(placed: &Placed, buf: &Buffer, fail: &dyn Fn(u16, u16, &str)) {
    let (r, name) = (placed.rect, placed.name);
    let (right, rail) = (r.right() - 1, placed.kind == Kind::Rail);
    // the rows: `top` is the top border, `closing` the bottom one (the closing row of a rail), the body rows between
    let (top, closing) = if rail { (r.y + 1, r.bottom() - 2) } else { (r.y, r.bottom() - 1) };
    let walls = |y: u16, left: &[&str], end: Option<&str>, what: &str| {
        if !left.contains(&sym(buf, r.x, y)) {
            fail(r.x, y, &format!("(2) box {name}: the first column is not {what}"));
        }
        if let Some(end) = end.filter(|_| !rail) {
            if sym(buf, right, y) != end {
                fail(right, y, &format!("(2) box {name}: the last column is not {end}"));
            }
        }
        // a rail's title runs to the area's last cell (the Inbox's pill ends there); the body and closing rows leave it blank
        if rail && y != top && !blank(&buf[(right, y)]) && !exempted(placed, y, Rule::RightEdge) {
            fail(right, y, &format!("(2) box {name}: a rail writes its last column"));
        }
    };
    walls(top, &["╭"], Some("╮"), "╭");
    for y in top + 1..closing {
        walls(y, &["│", "├"], Some("│"), "│ or ├");
    }
    walls(closing, &["╰"], Some("╯"), "╰");
    // (6) hs-d476: a rail has a blank row above and one below
    if rail {
        for y in [r.y, r.bottom() - 1] {
            for x in r.x..r.right() {
                if !blank(&buf[(x, y)]) {
                    fail(x, y, &format!("(6) box {name}: the row above or below a rail is not blank"));
                }
            }
        }
    }
    // (3) the padding cells, (4) the content, on the body rows (a rail's closing row counts for (4))
    let last_content = right.saturating_sub(spacing::RIGHT_PAD);
    for y in top + 1..closing + u16::from(rail) {
        let body = y < closing;
        if body && !exempted(placed, y, Rule::Padding) {
            if !blank(&buf[(r.x + 1, y)]) {
                fail(r.x + 1, y, &format!("(3) box {name}: the cell after the wall is not blank"));
            }
            if !rail && !blank(&buf[(right - 1, y)]) {
                fail(right - 1, y, &format!("(3) box {name}: the cell before the right wall is not blank"));
            }
        }
        for x in r.x + 1..=right {
            if x > last_content && !blank(&buf[(x, y)]) && !(x == right && !rail) && !exempted(placed, y, Rule::RightEdge) {
                fail(x, y, &format!("(4) box {name}: content past column {last_content}"));
            }
        }
    }
    // (5) vertical padding is 0: the row under the top border and the row above the bottom one show something
    for y in [top + 1, closing - 1] {
        if y <= top || y >= closing || exempted(placed, y, Rule::Vertical) {
            continue;
        }
        if (r.x + 1..right).all(|x| !inked(&buf[(x, y)])) {
            fail(r.x + 1, y, &format!("(5) box {name}: a blank row under the top border or above the bottom one"));
        }
    }
    // (8) the title: `╭─` then the pill's left cap
    let pill = (r.x..r.right()).find(|&x| sym(buf, x, top) == PILL_LEFT);
    if let Some(x) = pill.filter(|_| placed.kind != Kind::FullText) {
        if x != r.x + 2 || sym(buf, r.x, top) != "╭" || sym(buf, r.x + 1, top) != "─" {
            fail(x, top, &format!("(8) box {name}: the title does not start `╭─` then the pill's left cap"));
        }
    }
}

/// A section title's three rows: blank above and below the title row, which starts one column in with `──` and then, when a
/// pill fits, its left cap.
fn check_rule(placed: &Placed, buf: &Buffer, fail: &dyn Fn(u16, u16, &str)) {
    let (r, name) = (placed.rect, placed.name);
    if r.height != 3 {
        fail(r.x, r.y, &format!("(10) section title {name} {r:?} is not three rows"));
    }
    for y in [r.y, r.y + 2] {
        for x in r.x..r.right() {
            if !blank(&buf[(x, y)]) {
                fail(x, y, &format!("(10) section title {name}: the row above or below the title row is not blank"));
            }
        }
    }
    let y = r.y + 1;
    if r.width >= 3 && (sym(buf, r.x + 1, y) != "─" || sym(buf, r.x + 2, y) != "─") {
        fail(r.x + 1, y, &format!("(10) section title {name}: the title row does not start one column in with `──`"));
    }
    if !blank(&buf[(r.x, y)]) || !blank(&buf[(r.right() - 1, y)]) {
        fail(r.x, y, &format!("(10) section title {name}: the title row's first or last column is not blank"));
    }
    if let Some(x) = (r.x..r.right()).find(|&x| sym(buf, x, y) == PILL_LEFT) {
        if x != r.x + 3 {
            fail(x, y, &format!("(10) section title {name}: the pill's left cap is not right after `──`"));
        }
    }
}
