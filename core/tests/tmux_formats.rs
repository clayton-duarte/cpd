//! hs-d684 item 2 and pro-d61 item 1, the contract lines-docs writes (docs/contract.md §6.1):
//!
//! 1. the title pill's leading `━` carries no colour of its own: it takes the pane border line's
//!    style (pro-d61), blue on the focused pane and colour 7 otherwise, and stands *before* the
//!    pill's own colour (the blue/grey conditional and the dango choice); and
//! 2. the status line is heavy: every line is `━` (U+2501) and every junction is `┻` (U+253B),
//!    with no light `─` (U+2500) or `┸` (U+2538) left.
//!
//! The subject is cpd's own strings, `BORDER_FORMAT` and `STATUS_FORMAT`, so no tmux runs here.
//! The module is named `tmux_formats` (not `tests`) so nextest prefixes each test with the file:
//! `tmux_formats::<fn>` is the name the Job's acceptance lists.

mod tmux_formats {
    use cpd_core::host::tmux::{BORDER_FORMAT, STATUS_FORMAT};

    #[test]
    fn the_title_pill_lead_is_the_pane_border_colour() {
        // No style directive stands before the lead `━` (pro-d61): it takes the border line's style.
        // The plain title: `━`, then the pill's own colour starts.
        assert!(
            BORDER_FORMAT.contains("#{?#{@cpd_title},━#{?pane_active,"),
            "the plain title lead has no style before it: {}",
            *BORDER_FORMAT
        );
        assert!(
            !BORDER_FORMAT.contains("#[fg=colour7]━"),
            "no colour 7 before a lead: {}",
            *BORDER_FORMAT
        );
    }

    #[test]
    fn the_status_line_is_heavy_with_a_heavy_junction() {
        const HEAVY_LINE: char = '\u{2501}';
        const HEAVY_JUNCTION: char = '\u{253B}';
        const LIGHT_LINE: char = '\u{2500}';
        const LIGHT_JUNCTION: char = '\u{2538}';
        assert!(
            STATUS_FORMAT.contains(HEAVY_LINE),
            "the status line is heavy: {}",
            *STATUS_FORMAT
        );
        assert!(
            STATUS_FORMAT.contains(HEAVY_JUNCTION),
            "the status junction is heavy: {}",
            *STATUS_FORMAT
        );
        assert!(
            !STATUS_FORMAT.contains(LIGHT_LINE),
            "no light line in the status format: {}",
            *STATUS_FORMAT
        );
        assert!(
            !STATUS_FORMAT.contains(LIGHT_JUNCTION),
            "no light junction in the status format: {}",
            *STATUS_FORMAT
        );
    }
}
