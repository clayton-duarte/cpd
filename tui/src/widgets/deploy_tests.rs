//! Red tests (pro-d100): the Inbox row's glyph by kind for `deploy`, `pr-ready` and `review-requested`, the legend's three
//! kind lines, and the click on a `deploy` item's label. Rows are read from a `TestBackend` buffer; `open` is a mock.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use ratatui::text::Line;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::inbox::{self, Shape, item};
use super::legend;
use crate::link::{Links, open_url};
use crate::palette::{self, Meaning};
use cpd_core::clock::ManualClock;
use cpd_core::host::runner::FakeRunner;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const OPENED: &str = "2026-01-01T00:00:00.000Z";
const URL: &str = "https://github.com/acme/api/actions/runs/42";
const WIDTH: u16 = 60;

fn drawn(items: &[Item]) -> (Buffer, Links) {
    let shape = Shape::new(items.len(), false);
    let pane = Rect::new(0, 0, WIDTH, 12);
    let now = OffsetDateTime::parse("2026-01-01T00:03:00.000Z", &Rfc3339).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 12)).unwrap();
    let mut links = Links::default();
    terminal.draw(|f| links = inbox::draw(f, pane, items, shape, None, now, &|_| None)).unwrap();
    (terminal.backend().buffer().clone(), links)
}

/// The colour of the cell that shows `glyph` in the drawn row of the only item.
fn glyph_colour(item: Item, glyph: &str) -> Option<Option<Color>> {
    let (buf, _) = drawn(&[item]);
    (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == glyph)
        .map(|(x, y)| buf[(x, y)].fg).map(Some)
}

fn clickable(m: Meaning) -> Option<Color> {
    Some(palette::color_clickable(m))
}

#[test]
fn deploy_item_draws_f15a9_in_the_yellow_tier_colour() {
    let it = item(1, Kind::Deploy, Urgency::Yellow, "acme/api deploy prod: ship", OPENED);
    assert_eq!(glyph_colour(it, "\u{F15A9}"), Some(clickable(Meaning::Review)));
}

#[test]
fn pr_ready_item_draws_f12f4_in_the_yellow_tier_colour() {
    let it = item(1, Kind::PrReady, Urgency::Yellow, "acme/api#1 ready", OPENED);
    assert_eq!(glyph_colour(it, "\u{F12F4}"), Some(clickable(Meaning::Review)));
}

#[test]
fn review_requested_item_draws_f12f2_in_the_blue_tier_colour() {
    let it = item(1, Kind::ReviewRequested, Urgency::Blue, "acme/api#2 review", OPENED);
    assert_eq!(glyph_colour(it, "\u{F12F2}"), Some(clickable(Meaning::External)));
}

#[test]
fn legend_inbox_section_lists_the_three_kind_lines_after_the_six_tiers() {
    let all = legend::sections();
    let inbox = all.iter().find(|s| s.title == "Inbox").expect("an Inbox section");
    let text = |line: &Line| -> String { line.spans.iter().map(|s| s.content.as_ref()).collect() };
    let cells: Vec<String> = inbox.rows.iter().flatten().map(text).collect();
    let tail: Vec<&str> = cells.iter().skip(cells.len().saturating_sub(6)).map(String::as_str).collect();
    assert_eq!(
        tail,
        [
            "\u{F15A9}", "a deploy waits for your approval",
            "\u{F12F4}", "a pull request of yours is ready to merge",
            "\u{F12F2}", "a review asked of you",
        ]
    );
    let colours: Vec<_> = inbox.rows.iter().rev().take(3).map(|r| r[0].spans[0].style.fg).collect();
    assert_eq!(colours, [clickable(Meaning::External), clickable(Meaning::Review), clickable(Meaning::Review)]);
    assert_eq!(inbox.rows.len(), 11, "eight old lines and the three kind lines");
}

#[test]
fn clicking_a_deploy_item_label_opens_its_url() {
    let mut it = item(1, Kind::Deploy, Urgency::Yellow, "acme/api deploy prod: ship", OPENED);
    it.url = Some(URL.to_owned());
    let (buf, links) = drawn(&[it]);
    let at = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| Position::new(x, y)))
        .find(|p| links.at(*p).is_some());
    let url = at.and_then(|p| links.at(p)).expect("the label is a Link");
    let runner = FakeRunner::new(ManualClock::new());
    open_url(&runner, url).unwrap();
    let calls = runner.calls();
    assert_eq!(calls.iter().map(|c| c.args.clone()).collect::<Vec<_>>(), [vec![URL.to_owned()]], "`open <url>`");
}
