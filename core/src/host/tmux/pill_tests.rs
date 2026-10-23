//! Red tests of the tmux pill adapter (`pill.rs`): pure strings, no tmux process.

use super::palette::cells;
use super::pill::{fg, fg_if, pill, range};
use super::BORDER_FORMAT;

const LEFT: char = '\u{e0b6}';
const RIGHT: char = '\u{e0b4}';

#[test]
fn tmux_fg_prints_the_colour_number() {
    assert_eq!(fg(4), "#[fg=colour4]");
    assert_eq!(fg(15), "#[fg=colour15]");
}

#[test]
fn tmux_fg_if_wraps_two_colours_in_a_condition() {
    assert_eq!(fg_if("pane_active", 4, 7), "#{?pane_active,#[fg=colour4],#[fg=colour7]}");
}

#[test]
fn tmux_pill_is_lead_cap_reverse_text_noreverse_cap() {
    assert_eq!(pill("#[fg=colour3]", "", "x"), "#[fg=colour3]\u{e0b6}#[reverse]x#[noreverse]\u{e0b4}");
    assert_eq!(pill("#[fg=colour7]", "━", "#{@t}"), "#[fg=colour7]━\u{e0b6}#[reverse]#{@t}#[noreverse]\u{e0b4}");
}

#[test]
fn tmux_range_wraps_the_inner_in_a_user_range() {
    assert_eq!(range("logs", "abc"), "#[range=user|logs]abc#[norange]");
}

#[test]
fn tmux_pill_decodes_to_the_spec_caps_reverse_and_colour_for_every_meaning() {
    let spec = crate::pill::spec();
    assert_eq!((spec.caps.left.as_str(), spec.caps.right.as_str()), ("\u{e0b6}", "\u{e0b4}"));
    let m = &spec.meanings;
    let indices = [m.plain, m.in_progress, m.review, m.done, m.failed, m.planning, m.nothing, m.focus];
    for index in indices.into_iter().flat_map(|i| [i, spec.clickable(i)]) {
        let drawn = cells(&pill(&fg(index), "", "x"));
        let chars: String = drawn.iter().map(|c| c.ch).collect();
        assert_eq!(chars, format!("{LEFT}x{RIGHT}"), "index {index}");
        let reverse: Vec<bool> = drawn.iter().map(|c| c.reverse).collect();
        assert_eq!(reverse, [false, true, false], "index {index}: reverse on the text only");
        assert!(drawn.iter().all(|c| c.fg == Some(index) && c.bg.is_none()), "index {index}: foreground on all, no background");
    }
}

#[test]
fn the_border_format_title_pill_follows_pane_active_with_colour_numbers() {
    let f: &str = &BORDER_FORMAT;
    assert!(f.contains("#{?pane_active,#[fg=colour4],#[fg=colour7]}"), "{f}");
}
