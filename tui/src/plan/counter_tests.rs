//! Red tests of the Workflow active-time counter on the bottom border of a PR box (Context record pro-d175): the duration
//! text of `format_elapsed`, its place, style and width on the box's bottom border, and the exit `┬` of a need line.

use ratatui::buffer::Buffer;

use super::lines_tests::{reaches, violations};
use super::flow::Pill;
use super::{boxes, sample_pr};
use crate::read::{NeedsUser, WorkflowNode};
use crate::test_text::rows;
use cpd_core::ids::WorkflowKey;
use ratatui::layout::Rect;

fn chars(s: &str) -> Vec<String> {
    s.chars().map(String::from).collect()
}

fn canvas(prs: &[WorkflowNode]) -> Buffer {
    boxes::layout(prs, &[], &NeedsUser::default(), 0).chart.buf
}

/// The first row of `buf` that starts with `╰`, as cells.
fn bottom(buf: &Buffer) -> (usize, Vec<String>) {
    let r = rows(buf);
    let y = r.iter().position(|l| l.starts_with('╰')).expect("a bottom border");
    (y, chars(r[y].trim_end()))
}

fn with(key: &str, secs: Option<u64>) -> WorkflowNode {
    WorkflowNode { elapsed_seconds: secs, ..sample_pr(key) }
}

#[test]
fn under_a_minute_is_seconds_only() {
    let got: Vec<String> = [0, 1, 42, 59].map(boxes::format_elapsed).into();
    assert_eq!(got, ["0s", "1s", "42s", "59s"]);
}

#[test]
fn under_an_hour_is_minutes_then_seconds() {
    let got: Vec<String> = [60, 123, 720, 3599].map(boxes::format_elapsed).into();
    assert_eq!(got, ["1m·0s", "2m·3s", "12m·0s", "59m·59s"]);
}

#[test]
fn an_hour_or_more_is_hours_minutes_then_seconds() {
    let got: Vec<String> = [3600, 3723, 90061, u64::MAX].map(boxes::format_elapsed).into();
    assert_eq!(got, ["1h·0m·0s", "1h·2m·3s", "25h·1m·1s", "5124095576030431h·0m·15s"]);
}

#[test]
fn a_box_with_elapsed_time_reads_the_counter_on_its_bottom_border() {
    let buf = canvas(&[with("api", Some(3723))]);
    let (_, row) = bottom(&buf);
    let line = row.concat();
    assert!(line.starts_with("╰─ 1h·2m·3s ─"), "{line}");
    let width = buf.area.width as usize;
    assert_eq!(row.len(), width, "{line}");
    assert!(row[12..width - 1].iter().all(|c| c == "─"), "{line}");
    assert_eq!(row[width - 1], "╯", "{line}");
}

#[test]
fn the_counter_is_drawn_in_the_muted_text_colour() {
    let buf = canvas(&[with("api", Some(3723))]);
    let (y, row) = bottom(&buf);
    assert!(row.concat().starts_with("╰─ 1h·2m·3s ─"), "no counter drawn");
    let want = crate::palette::muted_text();
    for x in 2..12u16 {
        let c = &buf[(x, y as u16)];
        assert_eq!((c.fg, c.bg, c.modifier), (want.fg.unwrap(), want.bg.unwrap_or(c.bg), c.modifier), "column {x}");
        assert_eq!(c.style().add_modifier, want.add_modifier, "column {x}");
    }
    let last = buf.area.width - 1;
    let corner = |x: u16| {
        let c = &buf[(x, y as u16)];
        (c.fg, c.bg, c.modifier)
    };
    let top = &buf[(0, 0)];
    assert_eq!(corner(0), (top.fg, top.bg, top.modifier));
    assert_eq!(corner(last), (top.fg, top.bg, top.modifier));
}

#[test]
fn the_counter_shows_only_once_the_workflow_has_an_attempt() {
    let buf = canvas(&[with("api", None), with("web", Some(0))]);
    let r = rows(&buf);
    let bottoms: Vec<&String> = r.iter().filter(|l| l.contains('╰')).collect();
    let line = bottoms[0];
    let all: Vec<String> = chars(line);
    let first_end = all.iter().position(|c| c == "╯").unwrap();
    assert!(all[1..first_end].iter().all(|c| c == "─"), "api has no counter: {line}");
    let web = line.chars().skip(first_end + 1).collect::<String>();
    let web = web.trim_start();
    assert!(web.starts_with("╰─ 0s ─"), "{line}");
}

#[test]
fn a_box_is_widened_to_hold_its_counter_plus_four() {
    let mut p = sample_pr("a");
    p.repo_name = "r".into();
    p.base = "m".into();
    p.elapsed_seconds = Some(u64::MAX);
    let title = format!(" a · r \u{F19B0} m · {} ", p.status.as_str()).chars().count();
    let buf = canvas(&[p]);
    let (_, row) = bottom(&buf);
    let line = row.concat();
    assert_eq!(row.len(), (title + 4).max(30), "{line}");
    assert!(line.starts_with("╰─ 5124095576030431h·0m·15s ─"), "{line}");
    assert!(line.ends_with("─╯"), "{line}");
}

/// The `┬` columns of the first box's bottom border, its width and the canvas.
fn exit(secs: Option<u64>) -> (Buffer, usize, usize, Vec<usize>) {
    let web = WorkflowNode { needs: vec![WorkflowKey::parse("api").unwrap()], ..sample_pr("web") };
    let buf = canvas(&[with("api", secs), web]);
    let r = rows(&buf);
    let y = r.iter().position(|l| l.starts_with('╰')).unwrap();
    let row = chars(&r[y]);
    let width = row.iter().position(|c| c == "╯").unwrap() + 1;
    let tees = (0..width).filter(|&x| row[x] == "┬").collect();
    (buf, y, width, tees)
}

#[test]
fn a_need_line_leaves_a_box_past_its_counter() {
    let (buf, y, width, tees) = exit(Some(u64::MAX));
    assert_eq!(tees, [2 + 26], "the first `─` after the counter\n{}", rows(&buf).join("\n"));
    let r = rows(&buf);
    let bottom_row = chars(&r[y]);
    assert_eq!(bottom_row[..29].concat(), "╰─ 5124095576030431h·0m·15s ┬", "{}", r.join("\n"));
    let below = chars(&r[y + 1]);
    assert_eq!(below[28], "╰", "the line turns right\n{}", r.join("\n"));
    let web_top = y + 1 + r[y + 1..].iter().position(|l| l.starts_with('╭')).expect("web's top border");
    let landed: Vec<usize> = chars(&r[web_top]).iter().enumerate().filter(|(_, c)| *c == "┴").map(|(x, _)| x).collect();
    assert_eq!(landed.len(), 1, "{}", r.join("\n"));
    assert!(landed[0] > 28, "{}", r.join("\n"));
    let web_bottom = web_top + r[web_top..].iter().position(|l| l.starts_with('╰')).expect("web's bottom border");
    let boxes_: Vec<Rect> = vec![
        Rect::new(0, 0, width as u16, y as u16 + 1),
        Rect::new(0, web_top as u16, buf.area.width, (web_bottom - web_top) as u16 + 1),
    ];
    let none: Vec<Pill> = Vec::new();
    assert!(violations(&buf, &[], &boxes_, buf.area).is_empty());
    let _ = (reaches, none);

    let (_, _, w5, t5) = exit(Some(5));
    let (_, _, wn, tn) = exit(None);
    assert_eq!(t5, [(w5 - 1) / 2], "the middle column");
    assert_eq!(t5, tn);
    assert_eq!(w5, wn);
}
