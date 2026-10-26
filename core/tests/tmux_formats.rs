//! hs-d684 items 1 and 2, the contract lines-docs writes (docs/contract.md §6.1):
//!
//! 1. the title pill's leading `━` is the pane-border line colour, colour 7, and stands
//!    *before* the pill's own colour (the blue/grey conditional and the dango choice), so it is
//!    independent of the pill; and
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
        // The lead `━` is `#[fg=colour7]`, then the pill's own colour starts: the plain title's
        // blue-on-focus / grey conditional.
        assert!(
            BORDER_FORMAT.contains("#[fg=colour7]━#{?pane_active,"),
            "the plain title lead is colour 7 before the pill's colour: {}",
            *BORDER_FORMAT
        );
        // The dango view pills too: the lead is colour 7, then the `tree`/`plan`/`widgets` colour.
        assert!(
            BORDER_FORMAT.contains("#[fg=colour7]━#{?#{==:#{@cpd_view},tree},"),
            "the view title lead is colour 7 before the dango colour: {}",
            *BORDER_FORMAT
        );
    }

    #[test]
    fn the_status_line_is_heavy_with_a_heavy_junction() {
        const HEAVY_LINE: char = '\u{2501}';
        const HEAVY_JUNCTION: char = '\u{253B}';
        const LIGHT_LINE: char = '\u{2500}';
        const LIGHT_JUNCTION: char = '\u{2538}';
        assert!(STATUS_FORMAT.contains(HEAVY_LINE), "the status line is heavy: {}", *STATUS_FORMAT);
        assert!(STATUS_FORMAT.contains(HEAVY_JUNCTION), "the status junction is heavy: {}", *STATUS_FORMAT);
        assert!(!STATUS_FORMAT.contains(LIGHT_LINE), "no light line in the status format: {}", *STATUS_FORMAT);
        assert!(!STATUS_FORMAT.contains(LIGHT_JUNCTION), "no light junction in the status format: {}", *STATUS_FORMAT);
    }
}
