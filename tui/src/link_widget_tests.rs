//! The `Link` widget, `Links` and `open_url` (pro-d60): ordinary cells, no OSC 8, a click target per drawn link.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::link::{CUE, Link, Links, open_url};
use crate::palette;
use cpd_core::clock::ManualClock;
use cpd_core::error::ErrorCode;
use cpd_core::host::runner::{FakeRunner, Reply};

const URL: &str = "https://github.com/o/r/pull/7";

fn buf(w: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, 2))
}

fn cell(b: &Buffer, x: u16) -> (String, Style, Modifier) {
    let c = &b[(x, 0)];
    (c.symbol().to_owned(), c.style(), c.modifier)
}

fn runner() -> FakeRunner {
    FakeRunner::new(ManualClock::new())
}

fn underlined(b: &Buffer, x: u16) -> bool {
    b[(x, 0)].modifier.contains(Modifier::UNDERLINED)
}

#[test]
fn a_link_with_no_text_draws_only_the_cue_in_the_clickable_colour() {
    let mut b = buf(6);
    Link::new(URL).render(Rect::new(2, 0, 4, 1), &mut b);
    assert_eq!(Link::new(URL).width(), 1);
    assert_eq!(b[(2, 0)].symbol(), CUE);
    assert_eq!(b[(2, 0)].fg, palette::clickable_text().fg.unwrap());
    for x in [0, 1, 3, 4, 5] {
        assert_eq!(b[(x, 0)].symbol(), " ", "x {x}");
    }
}

#[test]
fn a_link_with_text_draws_the_underlined_text_a_gap_and_the_cue() {
    let mut b = buf(8);
    let link = Link::new(URL).text("abc");
    assert_eq!(link.width(), 5);
    link.render(Rect::new(1, 0, 7, 1), &mut b);
    let row: Vec<String> = (0..8).map(|x| b[(x, 0)].symbol().to_owned()).collect();
    assert_eq!(row, [" ", "a", "b", "c", " ", CUE, " ", " "]);
    for x in 1..=3 {
        assert!(underlined(&b, x), "x {x}");
    }
}

#[test]
fn the_underline_is_added_to_every_span_and_each_span_keeps_its_colours() {
    let mut b = buf(6);
    let line = Line::from(vec![
        Span::styled("a", Style::new().fg(Color::Red)),
        Span::styled("b", Style::new().fg(Color::Green).bg(Color::Blue).add_modifier(Modifier::BOLD)),
    ]);
    Link::new(URL).text(line).render(Rect::new(0, 0, 6, 1), &mut b);
    let (_, a, am) = cell(&b, 0);
    assert_eq!(a.fg, Some(Color::Red));
    assert!(am.contains(Modifier::UNDERLINED));
    let (_, s, m) = cell(&b, 1);
    assert_eq!((s.fg, s.bg), (Some(Color::Green), Some(Color::Blue)));
    assert!(m.contains(Modifier::UNDERLINED | Modifier::BOLD));
}

#[test]
fn the_cue_and_the_gap_are_not_underlined() {
    let mut b = buf(6);
    Link::new(URL).text("ab").render(Rect::new(0, 0, 6, 1), &mut b);
    assert_eq!(b[(2, 0)].symbol(), " ");
    assert!(!underlined(&b, 2));
    assert_eq!(b[(3, 0)].symbol(), CUE);
    assert!(!underlined(&b, 3));
    assert_eq!(b[(3, 0)].fg, palette::clickable_text().fg.unwrap());
}

#[test]
fn no_cell_the_widget_writes_holds_an_escape_character() {
    let mut b = buf(12);
    Link::new(URL).text("hello").render(Rect::new(0, 0, 12, 1), &mut b);
    Link::new(URL).render(Rect::new(0, 1, 12, 1), &mut b);
    assert!(b[(0, 0)].symbol() == "h", "the widget drew");
    for c in &b.content {
        assert!(!c.symbol().contains('\x1b') && !c.symbol().contains('\x07'), "{:?}", c.symbol());
    }
}

#[test]
fn links_at_answers_the_url_on_every_cell_of_the_output_and_none_elsewhere() {
    let mut b = buf(10);
    let mut links = Links::default();
    links.draw(Link::new(URL).text("abc"), Rect::new(2, 0, 8, 1), &mut b);
    for x in 2..7 {
        assert_eq!(links.at(Position::new(x, 0)), Some(URL), "x {x}");
    }
    for x in [0, 1, 7, 8, 9] {
        assert_eq!(links.at(Position::new(x, 0)), None, "x {x}");
    }
    assert_eq!(links.at(Position::new(3, 1)), None);
}

#[test]
fn a_link_with_no_text_is_hit_only_on_its_cue_cell() {
    let mut b = buf(6);
    let mut links = Links::default();
    links.draw(Link::new(URL), Rect::new(3, 0, 3, 1), &mut b);
    assert_eq!(links.at(Position::new(3, 0)), Some(URL));
    for x in [0, 2, 4, 5] {
        assert_eq!(links.at(Position::new(x, 0)), None, "x {x}");
    }
}

#[test]
fn a_narrow_area_cuts_the_text_with_an_ellipsis_and_keeps_the_cue() {
    let mut b = buf(8);
    let mut links = Links::default();
    links.draw(Link::new(URL).text("abcdef"), Rect::new(0, 0, 5, 1), &mut b);
    let row: Vec<String> = (0..6).map(|x| b[(x, 0)].symbol().to_owned()).collect();
    assert_eq!(row, ["a", "b", "…", " ", CUE, " "]);
    assert!(underlined(&b, 0) && underlined(&b, 2));
    assert_eq!(links.at(Position::new(4, 0)), Some(URL));
    assert_eq!(links.at(Position::new(5, 0)), None);

    let mut b = buf(8);
    let mut links = Links::default();
    links.draw(Link::new(URL).text("abcdef"), Rect::new(1, 0, 2, 1), &mut b);
    assert_eq!(b[(1, 0)].symbol(), CUE);
    assert_eq!(b[(2, 0)].symbol(), " ");
    assert_eq!(links.at(Position::new(1, 0)), Some(URL));
    assert_eq!(links.at(Position::new(2, 0)), None);
}

#[test]
fn an_area_of_zero_width_draws_and_records_nothing() {
    let mut b = buf(4);
    let mut links = Links::default();
    links.draw(Link::new(URL).text("ab"), Rect::new(1, 0, 0, 1), &mut b);
    links.draw(Link::new(URL), Rect::new(1, 0, 3, 0), &mut b);
    assert_eq!(b, buf(4));
    assert_eq!(links, Links::default());
}

#[test]
fn opening_a_link_runs_open_with_the_url_as_its_one_argument() {
    let fake = runner().on("open", |_| true, Reply::ok(""));
    open_url(&fake, URL).unwrap();
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].program.as_str(), calls[0].args.as_slice()), ("open", [URL.to_owned()].as_slice()));
}

#[test]
fn a_url_that_is_not_http_or_https_is_refused_and_nothing_runs() {
    let fake = runner().on("open", |_| true, Reply::ok(""));
    let bad = ["file:///etc/passwd", "ftp://x.y/z", "github.com/o/r", "https://a b", "https://a\nb", "https://é.com", "https://a\x1b]8;;"];
    for url in bad {
        let _ = open_url(&fake, url);
    }
    for url in bad {
        assert!(open_url(&fake, url).is_err(), "{url:?}");
    }
    assert!(fake.calls().is_empty(), "{:?}", fake.calls());
    assert!(open_url(&fake, "http://localhost:3000/x").is_ok());
}

#[test]
fn a_failing_open_is_returned_as_an_error() {
    let fake = runner().on("open", |_| true, Reply::exit(1, "", "The file does not exist"));
    let err = open_url(&fake, URL).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    for part in ["open", URL, "The file does not exist"] {
        assert!(err.message().contains(part), "{part}: {}", err.message());
    }
}
