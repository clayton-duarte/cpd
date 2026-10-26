//! The Plan view's PR box titles are the `Link` widget (pro-d60): `#<number> <state>` underlined, then the cue, ordinary
//! cells and no OSC 8; a plain left click on any of it opens the pull request and does nothing else.

use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Modifier;

use super::*;
use crate::link::CUE;
use crate::read::{Pull, PrState};
use cpd_core::host::runner::{FakeRunner, Reply};
use cpd_core::ports::Fakes;

fn pull(number: i64, lead_working: bool) -> Pull {
    Pull {
        number,
        state: PrState::Open,
        url: format!("https://github.com/acme/repo/pull/{number}"),
        conflicting: false,
        clean: false,
        lead_working,
        update: None,
        draft: false,
    }
}

fn pr_with(key: &str, pull: Option<Pull>) -> read::PrNode {
    read::PrNode { pull, ..sample_pr(key) }
}

fn draw(ui: &mut Ui, centre: &Centre, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
    terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
    terminal.backend().buffer().clone()
}

/// The first cell whose symbol holds `#` of the title of pull request `number` (a cell may hold more than the `#`, today's
/// OSC 8 pieces do): the title's first cell. A title is found by the digit after its `#` being `number`'s first.
fn title_x(buf: &Buffer, number: i64) -> Option<(u16, u16)> {
    let needle = format!("#{}", number.to_string().chars().next().unwrap());
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if buf[(x, y)].symbol().contains(&needle) {
                return Some((x, y));
            }
        }
    }
    None
}

fn cells(buf: &Buffer, x: u16, y: u16, n: u16) -> Vec<String> {
    (x..x + n).map(|x| buf[(x, y)].symbol().to_owned()).collect()
}

fn underlined(buf: &Buffer, x: u16, y: u16) -> bool {
    buf[(x, y)].modifier.contains(Modifier::UNDERLINED)
}

fn left(x: u16, y: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers }
}

/// Ports whose `open` answers `reply`, and the runner to read its calls from.
fn ports_with(reply: Reply) -> (Ports, Arc<FakeRunner>) {
    let fakes = Fakes::new();
    let runner = Arc::new(FakeRunner::new((*fakes.clock).clone()).on("open", |_| true, reply));
    let mut ports = fakes.ports.clone();
    ports.runner = runner.clone();
    (ports, runner)
}

fn opened(runner: &FakeRunner) -> Vec<String> {
    runner.calls().iter().filter(|c| c.program == "open").flat_map(|c| c.args.clone()).collect()
}

fn one_box(number: i64, working: bool) -> Centre {
    sample_centre(vec![pr_with("api", Some(pull(number, working)))], Vec::new())
}

#[test]
fn a_pr_box_title_draws_number_and_state_underlined_then_the_cue_and_no_cell_holds_an_escape() {
    let centre = one_box(12, false);
    let mut ui = Ui::default();
    let buf = draw(&mut ui, &centre, 10);
    let (x, y) = title_x(&buf, 12).expect("the title shows #12");
    assert_eq!(cells(&buf, x, y, 11).concat(), format!("#12 open {CUE}"));
    for dx in 0..8 {
        assert!(underlined(&buf, x + dx, y), "`#12 open` is underlined at +{dx}");
    }
    assert!(!underlined(&buf, x + 8, y), "the gap is not underlined");
    assert!(!underlined(&buf, x + 9, y), "the cue is not underlined");
    assert_eq!(buf[(x + 9, y)].fg, palette::clickable_text().fg.unwrap(), "the cue is in the clickable colour");
    for cell in &buf.content {
        assert!(!cell.symbol().contains('\u{1b}'), "no cell holds an escape: {:?}", cell.symbol());
    }
}

#[test]
fn a_left_click_on_a_pr_box_titles_text_or_cue_opens_its_pull_request_url() {
    let centre = one_box(12, false);
    let mut ui = Ui::default();
    let buf = draw(&mut ui, &centre, 10);
    let (x, y) = title_x(&buf, 12).expect("the title shows #12");
    let url = "https://github.com/acme/repo/pull/12";
    for (dx, modifiers) in [(0, KeyModifiers::NONE), (5, KeyModifiers::NONE), (9, KeyModifiers::NONE), (3, KeyModifiers::SHIFT | KeyModifiers::SUPER)] {
        let (ports, runner) = ports_with(Reply::ok(""));
        assert!(ui.click(&ports, left(x + dx, y, modifiers)).unwrap(), "a click at +{dx} is the link's");
        assert_eq!(opened(&runner), [url], "+{dx} {modifiers:?}");
    }
    // another button, or a cell off the title, opens nothing
    let (ports, runner) = ports_with(Reply::ok(""));
    let right = MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), ..left(x, y, KeyModifiers::NONE) };
    assert!(!ui.click(&ports, right).unwrap());
    assert!(!ui.click(&ports, left(0, 0, KeyModifiers::NONE)).unwrap());
    assert!(opened(&runner).is_empty(), "{:?}", opened(&runner));
}

#[test]
fn a_click_on_the_title_does_nothing_else_than_open_the_page() {
    let centre = sample_centre(vec![pr_with("api", Some(pull(12, false)))], vec![sample_job("api", "a")]);
    let mut ui = Ui::default();
    ui.selected.key = Some("a".into());
    let buf = draw(&mut ui, &centre, 12);
    let (x, y) = title_x(&buf, 12).expect("the title shows #12");
    let (selected, scroll) = (ui.selected.key.clone(), ui.scroll);
    let (ports, runner) = ports_with(Reply::ok(""));
    assert!(ui.click(&ports, left(x + 2, y, KeyModifiers::NONE)).unwrap());
    assert_eq!(opened(&runner), ["https://github.com/acme/repo/pull/12"]);
    assert_eq!((ui.selected.key.clone(), ui.scroll, ui.error.clone()), (selected, scroll, None), "no selection, scroll or message changes");
    assert_eq!(runner.calls().len(), 1, "nothing but `open` ran");
}

#[test]
fn the_lead_working_dot_stays_between_the_state_and_the_cue_inside_the_link() {
    let centre = one_box(12, true);
    let mut ui = Ui::default();
    let buf = draw(&mut ui, &centre, 10);
    let (x, y) = title_x(&buf, 12).expect("the title shows #12");
    let dot = palette::SPINNER[0];
    assert_eq!(cells(&buf, x, y, 13).concat(), format!("#12 open {dot} {CUE}"));
    assert!(underlined(&buf, x + 9, y), "the dot is part of the text");
    assert!(!underlined(&buf, x + 10, y) && !underlined(&buf, x + 11, y), "the gap and the cue are not");
    let (ports, runner) = ports_with(Reply::ok(""));
    assert!(ui.click(&ports, left(x + 9, y, KeyModifiers::NONE)).unwrap(), "a click on the dot is the link's");
    assert_eq!(opened(&runner), ["https://github.com/acme/repo/pull/12"]);
}

#[test]
fn after_the_chart_scrolled_a_click_opens_the_url_of_the_title_now_under_the_pointer() {
    let mut web = pr_with("web", Some(pull(2, false)));
    web.needs = vec![cpd_core::ids::PrKey::parse("api").unwrap()];
    let centre = sample_centre(vec![pr_with("api", Some(pull(1, false))), web], Vec::new());
    let mut ui = Ui::default();
    let first = draw(&mut ui, &centre, 8);
    let (ax, ay) = title_x(&first, 1).expect("api's title");
    let (_, wy) = title_x(&first, 2).expect("web's title");
    assert!(wy > ay, "web is below api");
    // scroll the chart to its end (the draw clamps): web's title moves up
    ui.scroll = (0, 100);
    let moved = draw(&mut ui, &centre, 8);
    let (wx, wy2) = title_x(&moved, 2).expect("web's title is in view");
    assert!(wy2 < wy, "the title moved up with the scroll: {wy} to {wy2}");
    let (ports, runner) = ports_with(Reply::ok(""));
    assert!(ui.click(&ports, left(wx, wy2, KeyModifiers::NONE)).unwrap());
    assert_eq!(opened(&runner), ["https://github.com/acme/repo/pull/2"], "the title under the pointer is web's");
    // the cell api's title left is not a link of api any more
    let (ports, runner) = ports_with(Reply::ok(""));
    ui.click(&ports, left(ax, ay, KeyModifiers::NONE)).unwrap();
    assert!(!opened(&runner).iter().any(|u| u.ends_with("/pull/1")), "{:?}", opened(&runner));
}

#[test]
fn a_pr_box_with_no_pull_request_has_no_link() {
    let mut web = pr_with("web", Some(pull(2, false)));
    web.needs = vec![cpd_core::ids::PrKey::parse("api").unwrap()];
    let centre = sample_centre(vec![pr_with("api", None), web], Vec::new());
    let mut ui = Ui::default();
    let buf = draw(&mut ui, &centre, 20);
    let (ports, runner) = ports_with(Reply::ok(""));
    // the box with a pull request has its link: the harness sees it
    let (x, y) = title_x(&buf, 2).expect("web's title");
    assert!(ui.click(&ports, left(x, y, KeyModifiers::NONE)).unwrap());
    // no cell of the box without one is a link, and none holds an escape or the cue
    let (ports, runner2) = ports_with(Reply::ok(""));
    for cy in 0..y.saturating_sub(1) {
        for cx in 0..buf.area.width {
            assert!(!ui.click(&ports, left(cx, cy, KeyModifiers::NONE)).unwrap(), "({cx}, {cy})");
        }
    }
    assert!(opened(&runner2).is_empty());
    assert_eq!(opened(&runner), ["https://github.com/acme/repo/pull/2"]);
    let api_row: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol()).collect();
    assert!(!api_row.contains(CUE) && !api_row.contains('\u{1b}'), "{api_row:?}");
}
