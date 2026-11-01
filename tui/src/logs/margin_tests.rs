//! The logs popup's rows keep one blank column on each side (commands.md §13.2): the text starts in column 1 and ends
//! at column W-2 at the latest.
//!
//! These pin the exact end column and the `…` cut, which the layout rows do not.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use super::view::{Log, draw};
use cpd_core::events::Event;

const W: u16 = 40;
const H: u16 = 8;

fn event(id: i64, text: &str) -> Event {
    Event {
        id,
        at: format!("2026-01-03T00:00:{id:02}.000Z"),
        kind: "plan-phase".into(),
        context: None,
        plan: None,
        session: None,
        job: None,
        attempt: None,
        text: text.into(),
        data: serde_json::json!({}),
    }
}

fn cells(buf: &Buffer, y: u16) -> Vec<String> {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_owned())
        .collect()
}

#[test]
fn the_logs_popup_rows_keep_one_blank_column_on_each_side() {
    let mut log = Log::new(
        vec![event(1, &"long ".repeat(30)), event(2, "short")],
        super::view::test_zone(),
    );
    log.fit(H);
    let mut terminal = Terminal::new(TestBackend::new(W, H)).unwrap();
    terminal.draw(|f| draw(f, f.area(), &log)).unwrap();
    let buf = terminal.backend().buffer().clone();
    // under the title's three rows: the long line, then the short one
    let (long, short) = (cells(&buf, 3), cells(&buf, 4));
    assert_eq!(long[0].trim(), "", "column 0 is blank: {long:?}");
    assert_eq!(short[0].trim(), "", "column 0 is blank: {short:?}");
    assert_eq!(
        long[1], "1",
        "the text starts in column 1 (the clock): {long:?}"
    );
    assert_eq!(
        short[1], "1",
        "the text starts in column 1 (the clock): {short:?}"
    );
    let last = usize::from(W) - 2;
    assert_eq!(
        long.iter().rposition(|c| c.trim() != ""),
        Some(last),
        "the cut line ends at column W-2: {long:?}"
    );
    assert_eq!(long[last], "…", "with the `…` inside: {long:?}");
    assert_eq!(long[last + 1].trim(), "", "column W-1 is blank: {long:?}");
}
