//! Red tests of the tmux pill adapter (`pill.rs`): pure strings, no tmux process.

use super::pill::{fg, fg_if, lit, pill, range};

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
fn tmux_lit_is_coloured_text_on_the_block_then_the_pills_style() {
    assert_eq!(lit(8, 1, "5"), "#[noreverse]#[fg=colour1]#[bg=colour8]5#[reverse]#[fg=colour8]#[bg=default]");
}
