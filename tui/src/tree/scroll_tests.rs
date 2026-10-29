//! The tree's scroll, frames and clicks (docs/research/tree-scroll.md §5), pinned so that `draw` can stop rendering
//! `tui-tree-widget`'s `Tree` (hs-d597, hs-d596). The first four tests are green on the code that renders the widget and
//! must stay unchanged by its replacement; the last is red until the view no longer names the widget.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::actions::Action;
use super::fixture::{archived, context, plan, session, tree};
use super::view::{Effect, View};
use crate::read::{ContextNode, Place, Tree};
use cpd_core::ids::SessionId;
use cpd_core::plan::Phase;
use cpd_core::session::State;

pub(super) fn draw_now(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

pub(super) fn text_rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()).collect()
}

pub(super) fn press(view: &mut View, code: KeyCode) -> Effect {
    view.key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn pointer(view: &mut View, kind: MouseEventKind, column: u16, row: u16) -> Effect {
    view.mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
}

pub(super) fn click(view: &mut View, column: u16, row: u16) -> Effect {
    pointer(view, MouseEventKind::Down(MouseButton::Left), column, row)
}

pub(super) fn wheel(view: &mut View, down: bool, column: u16, row: u16) {
    let kind = if down { MouseEventKind::ScrollDown } else { MouseEventKind::ScrollUp };
    assert_eq!(pointer(view, kind, column, row), Effect::None);
}

fn sid(s: &str) -> SessionId {
    SessionId::parse(s).unwrap()
}

/// A Context `id` with `n` Plans `<id>00`, `<id>01`, …, each with an idle Lead `l-<id>NN`.
fn plans_of(id: &str, n: usize) -> ContextNode {
    let plans = (0..n)
        .map(|i| plan(&format!("{id}{i:02}"), id, Phase::Dispatch, [0; 6], session(&format!("l-{id}{i:02}"), "lead", State::Idle, None, Place::Parked), vec![]))
        .collect();
    context(id, plans)
}

/// One Context `alpha` with `n` Plans `alpha00`, …: `n + 1` rows.
pub(super) fn long_tree(n: usize) -> Tree {
    tree(vec![plans_of("alpha", n)])
}

/// The Lead's id a drawn row names (`alpha07` → `l-alpha07`), if it is a Plan row.
fn lead_of(row: &str) -> Option<SessionId> {
    let at = row.find("alpha")?;
    let name: String = row[at..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (name.len() > "alpha".len()).then(|| sid(&format!("l-{name}")))
}

fn is_cue(row: &str) -> bool {
    row.trim_start().starts_with('▲') || row.trim_start().starts_with('▼')
}

#[test]
fn rows_that_grow_above_a_cursor_that_did_not_move_draw_the_same_frame_twice() {
    let mut view = View::new(tree(vec![plans_of("bb", 25)]), true);
    for _ in 0..24 {
        press(&mut view, KeyCode::Down); // the cursor on bb23, the row before the last of 27
    }
    let _ = draw_now(&mut view, 43, 24);
    view.refresh(tree(vec![plans_of("aa", 40), plans_of("bb", 25)])); // 42 more rows above the cursor
    let grown = text_rows(&draw_now(&mut view, 43, 24));
    let again = text_rows(&draw_now(&mut view, 43, 24));
    assert_eq!(grown, again, "the frame the rows grew in is the next one");
    let (first, last) = (grown.iter().position(|r| is_cue(r)).expect("a cue"), grown.iter().rposition(|r| is_cue(r)).unwrap());
    let at = grown.iter().position(|r| r.contains("bb23")).expect("the cursor's row is drawn");
    assert!(first < at && (last == first || at < last), "between the cues: {grown:?}");
}

#[test]
fn a_click_on_a_scrolled_tree_opens_the_session_drawn_under_the_pointer() {
    let mut view = View::new(long_tree(60), true);
    draw_now(&mut view, 43, 24);
    wheel(&mut view, true, 10, 5);
    wheel(&mut view, true, 10, 5);
    let rows = text_rows(&draw_now(&mut view, 43, 24));
    let (above, below) = (rows.iter().position(|r| r.trim_start().starts_with('▲')).expect("▲ cue"), rows.iter().position(|r| r.trim_start().starts_with('▼')).expect("▼ cue"));
    assert!(rows[above + 1].contains("alpha05"), "two notches of three rows: {rows:?}");
    for y in above + 1..below {
        let lead = lead_of(&rows[y]).unwrap_or_else(|| panic!("row {y} is a Plan row: {rows:?}"));
        assert_eq!(click(&mut view, 10, y as u16), Effect::Run(Action::Open(lead)), "row {y}: {:?}", rows[y]);
    }
    assert!(rows[above + 1..below].len() > 15, "a window of rows: {rows:?}");
}

/// Every cell of the buffer as runs `n*[symbol|fg|bg|modifier]`, each as the `Debug` text of its value (no colour is named).
fn encode(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            let mut runs: Vec<(usize, String)> = Vec::new();
            for x in 0..buf.area.width {
                let c = &buf[(x, y)];
                let cell = format!("[{:?}|{:?}|{:?}|{:?}]", c.symbol(), c.fg, c.bg, c.modifier);
                match runs.last_mut() {
                    Some((n, last)) if *last == cell => *n += 1,
                    _ => runs.push((1, cell)),
                }
            }
            runs.iter().map(|(n, c)| format!("{n}*{c}")).collect::<Vec<_>>().join(" ")
        })
        .collect()
}

/// `archived()` (live Contexts, an archived Context `zeta`, archived rows folded in `alpha`) with `alpha` grown by Plans
/// until it scrolls, drawn at 43 x `height` after `notches` wheel notches, the `expand` rows clicked first when `unfold`.
fn frame(height: u16, notches: usize, unfold: bool) -> Vec<String> {
    let mut t = archived();
    for i in 0..14 {
        let p = plan(&format!("extra{i:02}"), "alpha", Phase::Dispatch, [0, 1, 0, 0, 0, i % 3], session(&format!("l-extra{i:02}"), "lead", State::Idle, None, Place::Parked), vec![]);
        t.contexts[0].plans.insert(1, p);
    }
    let mut view = View::new(t, true);
    if unfold {
        // the tall frame shows every closing row; a click on each `expand` row, the lowest first
        let rows = text_rows(&draw_now(&mut view, 43, 80));
        for y in (0..rows.len()).rev().filter(|&y| rows[y].contains("expand")) {
            click(&mut view, 10, y as u16);
        }
    }
    draw_now(&mut view, 43, height);
    for _ in 0..notches {
        wheel(&mut view, true, 10, 5);
    }
    encode(&draw_now(&mut view, 43, height))
}

const SCENARIOS: [(u16, usize, bool); 6] = [(12, 0, false), (12, 2, false), (20, 1, true), (20, 4, true), (31, 0, true), (31, 9, false)];

#[test]
fn a_scrolled_tree_with_folds_and_spacers_draws_the_same_cells_as_the_widget_did() {
    for (i, (height, notches, unfold)) in SCENARIOS.into_iter().enumerate() {
        let got = frame(height, notches, unfold);
        let want = FROZEN[i];
        assert_eq!(got.len(), want.len(), "scenario {i} ({height} rows, {notches} notches, unfolded {unfold})");
        for (y, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(g, w, "scenario {i} ({height} rows, {notches} notches, unfolded {unfold}), row {y}");
        }
    }
}

#[test]
fn the_click_geometry_follows_the_scroll_and_the_cues_and_the_tail_are_no_row() {
    // A short tree: row 0, the margins, the spacer between the Contexts and the tail below the last row are no row.
    let mut view = View::new(tree(vec![plans_of("alpha", 2), plans_of("beta", 2)]), true);
    let rows = text_rows(&draw_now(&mut view, 43, 24));
    let spacer = rows.iter().skip(1).position(|r| r.is_empty()).map(|y| y + 1).expect("a spacer row between the Contexts");
    let last = rows.iter().rposition(|r| !r.is_empty()).unwrap();
    assert_eq!(click(&mut view, 10, 0), Effect::None, "row 0 is blank");
    assert_eq!(click(&mut view, 10, spacer as u16), Effect::None, "the spacer is no row");
    assert_eq!(click(&mut view, 10, last as u16 + 1), Effect::None, "just below the last row");
    assert_eq!(click(&mut view, 10, 23), Effect::None, "the tail of a short tree");
    assert_eq!(click(&mut view, 0, 2), Effect::None, "the left margin");
    assert_eq!(click(&mut view, 42, 2), Effect::None, "the right margin");
    assert_eq!(click(&mut view, 10, 2), Effect::Run(Action::Open(sid("l-alpha00"))), "the first Plan's row");

    // A long tree: the cues are no row, and the wheel moves what the same coordinates mean.
    let mut view = View::new(long_tree(60), true);
    let rows = text_rows(&draw_now(&mut view, 43, 24));
    let below = rows.iter().position(|r| r.trim_start().starts_with('▼')).expect("▼ cue");
    assert_eq!(click(&mut view, 10, below as u16), Effect::None, "the ▼ cue is no row");
    assert_eq!(click(&mut view, 10, below as u16 - 1), Effect::Run(Action::Open(lead_of(&rows[below - 1]).unwrap())), "the last row above it");
    wheel(&mut view, true, 10, 5);
    let moved = text_rows(&draw_now(&mut view, 43, 24));
    assert!(moved[0].trim_start().starts_with('▲'), "the ▲ cue now: {moved:?}");
    assert_eq!(click(&mut view, 10, 0), Effect::None, "the ▲ cue is no row");
    assert_eq!(click(&mut view, 10, 1), Effect::Run(Action::Open(lead_of(&moved[1]).unwrap())), "the row under the cue");
    assert_ne!(lead_of(&moved[1]), lead_of(&rows[1]), "the same coordinates mean another row after the wheel");
    assert_eq!(click(&mut view, 10, below as u16), Effect::None, "the ▼ cue is still no row");
}

/// The product source of `rows.rs` and `view.rs`: the text before the first line that starts `#[cfg(test)]`.
fn product(source: &str) -> &str {
    let end = source.lines().scan(0, |at, l| {
        let start = *at;
        *at += l.len() + 1;
        Some((start, l))
    });
    end.into_iter().find(|(_, l)| l.starts_with("#[cfg(test)]")).map_or(source, |(at, _)| &source[..at])
}

#[test]
fn the_tree_view_never_renders_the_widget_or_a_scratch_buffer() {
    let named = ["TreeWidget", "StatefulWidget", "rendered_at", "scroll_down", "scroll_up", "Buffer::empty(Rect::new(0, 0, 1, 1))"];
    let mut found = Vec::new();
    for (file, source) in [("rows.rs", include_str!("rows.rs")), ("view.rs", include_str!("view.rs"))] {
        let text = product(source);
        found.extend(named.iter().filter(|n| text.contains(**n)).map(|n| format!("{file}: {n}")));
    }
    assert!(found.is_empty(), "the view draws its rows itself: {found:?}");
}

/// The frames of [`SCENARIOS`] as the widget drew them, re-taken once when the Inbox section left the tree (the tree then has the whole column) (per row, runs of
/// cells `n*[symbol|fg|bg|modifier]`, each the `Debug` text of its value). Frozen: never computed by the code under test.
const FROZEN: [&[&str]; 6] = [
    &[
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightMagenta|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["b"|White|Reset|BOLD] 1*["e"|White|Reset|BOLD] 1*["t"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 35*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightRed|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["l"|White|Reset|NONE] 33*[" "|Reset|Reset|NONE] 1*["3"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|White|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["a"|White|Reset|BOLD] 1*["l"|White|Reset|BOLD] 1*["p"|White|Reset|BOLD] 1*["h"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightCyan|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|Blue|Reset|NONE] 1*["h"|Blue|Reset|NONE] 1*["i"|Blue|Reset|NONE] 1*["p"|Blue|Reset|NONE] 28*[" "|Reset|Reset|NONE] 1*["2"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightYellow|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["5"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["9"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"33*[" "|Reset|Reset|NONE] 1*["▼"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["1"|DarkGray|Reset|NONE] 1*["9"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    ],
    &[
    r##"34*[" "|Reset|Reset|NONE] 1*["▲"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["6"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["9"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["8"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["7"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["6"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["5"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["4"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"33*[" "|Reset|Reset|NONE] 1*["▼"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["1"|DarkGray|Reset|NONE] 1*["3"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    ],
    &[
    r##"34*[" "|Reset|Reset|NONE] 1*["▲"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["3"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|White|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["a"|White|Reset|BOLD] 1*["l"|White|Reset|BOLD] 1*["p"|White|Reset|BOLD] 1*["h"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightCyan|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|Blue|Reset|NONE] 1*["h"|Blue|Reset|NONE] 1*["i"|Blue|Reset|NONE] 1*["p"|Blue|Reset|NONE] 28*[" "|Reset|Reset|NONE] 1*["2"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightYellow|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["5"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["9"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["8"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["7"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["6"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["5"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["4"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["0"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["k"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["c"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 30*[" "|Reset|Reset|NONE] 1*["4"|LightMagenta|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["p"|White|Reset|NONE] 1*["·"|White|Reset|NONE] 1*["f"|White|Reset|NONE] 1*["-"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["d"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE]"##,
    r##"34*[" "|Reset|Reset|NONE] 1*["▼"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["8"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    ],
    &[
    r##"33*[" "|Reset|Reset|NONE] 1*["▲"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 2*["1"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["m"|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["7"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["6"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["5"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["4"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["0"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["k"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["c"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 30*[" "|Reset|Reset|NONE] 1*["4"|LightMagenta|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["p"|White|Reset|NONE] 1*["·"|White|Reset|NONE] 1*["f"|White|Reset|NONE] 1*["-"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["d"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 1*["d"|DarkGray|Reset|NONE] 30*[" "|DarkGray|Reset|NONE] 1*["1"|DarkGray|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["3"|DarkGray|Reset|NONE] 1*["6"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["s"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["p"|DarkGray|Reset|NONE] 1*["·"|DarkGray|Reset|NONE] 1*["f"|DarkGray|Reset|NONE] 1*["-"|DarkGray|Reset|NONE] 1*["b"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 28*[" "|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2c}"|Gray|Reset|NONE] 1*[" "|Gray|Reset|NONE] 1*["c"|Gray|Reset|NONE] 1*["o"|Gray|Reset|NONE] 2*["l"|Gray|Reset|NONE] 1*["a"|Gray|Reset|NONE] 1*["p"|Gray|Reset|NONE] 1*["s"|Gray|Reset|NONE] 1*["e"|Gray|Reset|NONE] 31*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["g"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 2*["m"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2a}"|DarkGray|Reset|NONE] 1*[" "|White|Reset|NONE] 1*["n"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["w"|White|Reset|NONE] 36*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["\u{f0b26}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["a"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["c"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["v"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 33*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    ],
    &[
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightMagenta|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["b"|White|Reset|BOLD] 1*["e"|White|Reset|BOLD] 1*["t"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 35*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightRed|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["l"|White|Reset|NONE] 33*[" "|Reset|Reset|NONE] 1*["3"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|White|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["a"|White|Reset|BOLD] 1*["l"|White|Reset|BOLD] 1*["p"|White|Reset|BOLD] 1*["h"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightCyan|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|Blue|Reset|NONE] 1*["h"|Blue|Reset|NONE] 1*["i"|Blue|Reset|NONE] 1*["p"|Blue|Reset|NONE] 28*[" "|Reset|Reset|NONE] 1*["2"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightYellow|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["5"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["9"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["8"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["7"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["6"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["5"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["4"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["0"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["k"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["c"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 30*[" "|Reset|Reset|NONE] 1*["4"|LightMagenta|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["p"|White|Reset|NONE] 1*["·"|White|Reset|NONE] 1*["f"|White|Reset|NONE] 1*["-"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["d"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 1*["d"|DarkGray|Reset|NONE] 30*[" "|DarkGray|Reset|NONE] 1*["1"|DarkGray|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["3"|DarkGray|Reset|NONE] 1*["6"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["s"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["p"|DarkGray|Reset|NONE] 1*["·"|DarkGray|Reset|NONE] 1*["f"|DarkGray|Reset|NONE] 1*["-"|DarkGray|Reset|NONE] 1*["b"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 28*[" "|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2c}"|Gray|Reset|NONE] 1*[" "|Gray|Reset|NONE] 1*["c"|Gray|Reset|NONE] 1*["o"|Gray|Reset|NONE] 2*["l"|Gray|Reset|NONE] 1*["a"|Gray|Reset|NONE] 1*["p"|Gray|Reset|NONE] 1*["s"|Gray|Reset|NONE] 1*["e"|Gray|Reset|NONE] 31*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["g"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 2*["m"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2a}"|DarkGray|Reset|NONE] 1*[" "|White|Reset|NONE] 1*["n"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["w"|White|Reset|NONE] 36*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["\u{f0b26}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["a"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["c"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["v"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 33*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    ],
    &[
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightMagenta|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["b"|White|Reset|BOLD] 1*["e"|White|Reset|BOLD] 1*["t"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 35*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightRed|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["l"|White|Reset|NONE] 33*[" "|Reset|Reset|NONE] 1*["3"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|White|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["a"|White|Reset|BOLD] 1*["l"|White|Reset|BOLD] 1*["p"|White|Reset|BOLD] 1*["h"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|LightCyan|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|Blue|Reset|NONE] 1*["h"|Blue|Reset|NONE] 1*["i"|Blue|Reset|NONE] 1*["p"|Blue|Reset|NONE] 28*[" "|Reset|Reset|NONE] 1*["2"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightYellow|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["5"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["9"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["8"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["7"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["6"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["5"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["4"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["3"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["2"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["2"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 1*["0"|White|Reset|NONE] 1*["1"|White|Reset|NONE] 27*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["1"|LightGreen|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["x"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["r"|White|Reset|NONE] 1*["a"|White|Reset|NONE] 2*["0"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE] 1*["1"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["k"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["t"|White|Reset|NONE] 1*["c"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 30*[" "|Reset|Reset|NONE] 1*["4"|LightMagenta|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|White|Reset|NONE] 1*[" "|Reset|Reset|NONE] 1*["s"|White|Reset|NONE] 1*["h"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["p"|White|Reset|NONE] 1*["·"|White|Reset|NONE] 1*["f"|White|Reset|NONE] 1*["-"|White|Reset|NONE] 1*["i"|White|Reset|NONE] 1*["d"|White|Reset|NONE] 29*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["o"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 1*["d"|DarkGray|Reset|NONE] 30*[" "|DarkGray|Reset|NONE] 1*["1"|DarkGray|Reset|NONE] 1*["⋅"|DarkGray|Reset|NONE] 1*["3"|DarkGray|Reset|NONE] 1*["6"|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["├"|DarkGray|Reset|NONE] 1*["─"|DarkGray|Reset|NONE] 1*["\u{e3e3}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["s"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["p"|DarkGray|Reset|NONE] 1*["·"|DarkGray|Reset|NONE] 1*["f"|DarkGray|Reset|NONE] 1*["-"|DarkGray|Reset|NONE] 1*["b"|DarkGray|Reset|NONE] 1*["l"|DarkGray|Reset|NONE] 28*[" "|DarkGray|Reset|NONE] 1*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2c}"|Gray|Reset|NONE] 1*[" "|Gray|Reset|NONE] 1*["c"|Gray|Reset|NONE] 1*["o"|Gray|Reset|NONE] 2*["l"|Gray|Reset|NONE] 1*["a"|Gray|Reset|NONE] 1*["p"|Gray|Reset|NONE] 1*["s"|Gray|Reset|NONE] 1*["e"|Gray|Reset|NONE] 31*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╭"|DarkGray|Reset|NONE] 1*["\u{e3d5}"|DarkGray|Reset|BOLD] 1*[" "|Reset|Reset|BOLD] 1*["g"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 2*["m"|White|Reset|BOLD] 1*["a"|White|Reset|BOLD] 34*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["╰"|DarkGray|Reset|NONE] 1*["\u{f0b2a}"|DarkGray|Reset|NONE] 1*[" "|White|Reset|NONE] 1*["n"|White|Reset|NONE] 1*["e"|White|Reset|NONE] 1*["w"|White|Reset|NONE] 36*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    r##"1*[" "|Reset|Reset|NONE] 1*["\u{f0b26}"|DarkGray|Reset|NONE] 1*[" "|DarkGray|Reset|NONE] 1*["a"|DarkGray|Reset|NONE] 1*["r"|DarkGray|Reset|NONE] 1*["c"|DarkGray|Reset|NONE] 1*["h"|DarkGray|Reset|NONE] 1*["i"|DarkGray|Reset|NONE] 1*["v"|DarkGray|Reset|NONE] 1*["e"|DarkGray|Reset|NONE] 33*[" "|Reset|Reset|NONE]"##,
    r##"43*[" "|Reset|Reset|NONE]"##,
    ],
];
