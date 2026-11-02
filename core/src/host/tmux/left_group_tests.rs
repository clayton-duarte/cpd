//! The left group of the status line (five buttons, 25 cells) and the arithmetic that counts it.

use super::*;

// ---------- the status line, as `STATUS_FORMAT`'s text (the pane tests of `cli/tests/tui/tui.rs` drew it on a real tmux client) ----------
//
// tmux renders the format and is not tested. These pin what cpd writes into it: the option names, the arithmetic of `long_fits!`
// and of the fill, the preview and version widths, and the order, colours and joins of the pills.

/// A join: one grey `━`, outside the click ranges.
const JOIN: &str = "#[fg=colour8]\u{2501}#[default]";
/// The width a pill adds while its option is set, and 0 while it is unset.
const RUNNER_WIDTH: &str = "#{?@cpd_runner,#{e|+:#{n:@cpd_runner},3},0}";
const PREVIEW_WIDTH: &str = "#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}";
const VERSION_WIDTH: &str = "#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}";
const INCIDENT_WIDTH: &str = "#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}";
/// The left group: the leading `━`, five buttons of 4 cells and the joins between them.
const LEFT_WIDTH: usize = 25;
/// The long workers form in the pane tests: `workers 9·4·6/6` (15 cells) and two caps, `#{w:@cpd_workers}`.
const LONG_WIDTH: usize = 15 + 2;

/// `long_fits!()` as text: the client is at least the workers pill's width plus 27 (the left group's 25, the closing `━` and a
/// fill of at least 1), the preview pill, the version pill and the incident pill.
fn long_fits() -> String {
    format!(
        "#{{e|>=:#{{client_width}},#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{w:@cpd_workers}},2}},27}},{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}},{VERSION_WIDTH}}},{INCIDENT_WIDTH}}}}}"
    )
}

/// A button of the left group: its click range, its glyph and one space between the two caps (4 cells), a block of the bright pair of `base`.
fn button(range: &str, glyph: &str, base: u8) -> String {
    format!(
        "#[range=user|{range}]#[fg=colour{}]\u{e0b6}#[reverse]{glyph} #[noreverse]\u{e0b4}#[default]#[norange]",
        spec().clickable(base)
    )
}

/// A pill of colour `colour`: its text between the caps, with no padding, in reverse video.
fn pill_of(colour: u8, text: &str) -> String {
    format!("#[fg=colour{colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]")
}

/// The workers pill: the long form while the client is wide enough, else the short form.
fn workers_pill() -> String {
    pill_of(
        8,
        &format!(
            "#{{?{},#{{@cpd_workers}},#{{@cpd_workers_short}}}}",
            long_fits()
        ),
    )
}

/// The fill's cell count: the client's width less the left group, the preview, version and incident pills and the right
/// group's workers pill in the form drawn plus the closing `━`.
fn fill_count() -> String {
    format!(
        "#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},25}},{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}},{VERSION_WIDTH}}},{INCIDENT_WIDTH}}},#{{e|+:#{{?{},#{{e|+:#{{w:@cpd_workers}},2}},#{{e|+:#{{w:@cpd_workers_short}},2}}}},1}}}}",
        long_fits()
    )
}

/// The preview pill's conditional: the join and `preview: <value>`, empty while `@cpd_preview` is unset.
fn preview_branch() -> String {
    format!(
        "#{{?#{{@cpd_preview}},#[fg=colour8]\u{2501}{},}}",
        pill_of(3, "preview: #{@cpd_preview}")
    )
}

/// What follows the settings button in the format.
fn after_the_buttons() -> String {
    let logs = button("settings", SETTINGS_GLYPH, BUTTON_BASE);
    let f: &str = &STATUS_FORMAT;
    let after = &f[f.find(&logs).expect("the settings button") + logs.len()..];
    // the runner pill (pro-d199) sits between the settings button and the preview marker
    after[after.find("#{?#{@cpd_preview}").expect("the preview pill")..].to_owned()
}

// The right group is 18 cells wide in the long form (a text of 15 cells, 2 caps, the closing `━`):
// with the left group's 25 and a fill of at least 1 it needs a client of 44 columns; one less gets the short form.
#[test]
fn the_fill_and_the_long_form_count_a_left_group_of_25_and_27() {
    let f: &str = &STATUS_FORMAT;
    assert_eq!(
        long_fits!(),
        long_fits(),
        "`client_width >= w:@cpd_workers + 2 + 27`, plus the pills that are set"
    );
    assert_eq!(
        LEFT_WIDTH + 1 + 1,
        27,
        "the left group's 25, the closing `━` and a fill of 1"
    );
    assert_eq!(
        LONG_WIDTH + 27,
        44,
        "the long form needs a client of 44 columns"
    );
    assert!(
        f.contains(&workers_pill()),
        "the workers pill reads `@cpd_workers` when the long form fits: {f}"
    );
    // the line is full: the fill is the width less the left group and the right group in the form drawn
    assert_eq!(fill_count!(), fill_count());
    assert!(
        f.contains(&format!(
            "#{{?window_zoomed_flag,#{{R:\u{2501},{}}},",
            fill_count()
        )),
        "{f}"
    );
}

#[test]
fn the_status_line_is_one_full_line_of_joined_pills_in_the_short_workers_form_below_44_columns() {
    let f: &str = &STATUS_FORMAT;
    // the short form is the else branch of the long form's test, and the fill subtracts its width then
    assert!(
        f.contains(&workers_pill())
            && workers_pill().contains(",#{@cpd_workers},#{@cpd_workers_short}}"),
        "{f}"
    );
    assert!(
        fill_count().contains(",#{e|+:#{w:@cpd_workers},2},#{e|+:#{w:@cpd_workers_short},2}}"),
        "{}",
        fill_count()
    );
    // 44 fits and 43 does not: the test is `>=`
    assert!(
        long_fits().starts_with("#{e|>=:#{client_width},"),
        "{}",
        long_fits()
    );
    // the format writes no `workers` word of its own: it is in `@cpd_workers`, which the engine sets
    assert!(!f.replace("@cpd_workers", "").contains("workers"), "{f}");
}

#[test]
fn the_left_group_is_five_one_glyph_buttons_of_25_cells() {
    let f: &str = &STATUS_FORMAT;
    let buttons = format!(
        "{JOIN}{}{JOIN}{}{JOIN}{}{JOIN}{}{JOIN}{}",
        button("reset", RESET_GLYPH, BUTTON_BASE),
        button("legend", LEGEND_GLYPH, BUTTON_BASE),
        button("logs", LOGS_GLYPH, BUTTON_BASE),
        button("costs", COSTS_GLYPH, BUTTON_BASE),
        button("settings", SETTINGS_GLYPH, BUTTON_BASE)
    );
    // the leading `━`, then the five buttons joined by a single `━` each: 5 joins and 5 buttons of 4 cells, 25 cells
    assert!(f.starts_with(&buttons), "{f}");
    assert_eq!(buttons.matches('\u{2501}').count() + 5 * 4, LEFT_WIDTH);
    assert!(
        f.starts_with(&format!("{JOIN}#[range=user|reset]#[fg=colour15]\u{e0b6}")),
        "the leading line cell, grey, then a button's cap: {f}"
    );
    // the right group ends with the version conditional, then the closing `━`, grey
    assert!(
        f.ends_with(&format!(
            "#{{?#{{@cpd_version}},#[fg=colour8]\u{2501}{},}}}}{JOIN}",
            pill_of(8, "#{@cpd_version}")
        )),
        "{f}"
    );
    // the right pills are joined by one `━` (never a space), and the left group by the buttons' own
    assert!(!f.contains("inbox"), "{f}");
    assert!(
        !f.contains("\u{e0b4} \u{e0b6}") && !f.contains("\u{e0b4}\u{e0b6}"),
        "no space and no touching caps between two pills: {f}"
    );
}

#[test]
fn the_status_line_fill_is_grey_and_spans_the_free_width_of_any_client() {
    let f: &str = &STATUS_FORMAT;
    // the fill follows the settings button (and the preview pill, while set) in grey, and the right group follows it
    let after = after_the_buttons();
    assert!(after.starts_with(&preview_branch()), "{after}");
    assert!(
        after[preview_branch().len()..]
            .starts_with("#[fg=colour8]#{?window_zoomed_flag,#{R:\u{2501},"),
        "{after}"
    );
    assert!(after.contains("#[default]#{?@cpd_incident,"), "{after}");
    // a zoomed window's fill is one run of the whole free width, in either form (40 and 100 columns: the short form and the long one)
    assert!(
        f.contains(&format!(
            "#{{?window_zoomed_flag,#{{R:\u{2501},{}}},",
            fill_count()
        )),
        "{f}"
    );
    // otherwise it starts after the left group's 25 cells and the preview pill
    assert!(
        f.contains(&format!(
            "#{{e|+:#{{e|+:25,{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}}"
        )),
        "{f}"
    );
}

const PREVIEW: &str = "cpd.tui-visuals.box-order 6c43886";

// The preview pill is `preview: <value>` between caps, no padding, joined to the buttons by a `━`: 45 cells with
// 33 of value. With the left group's 25, the long form's 27 and a fill of 1 it needs 89 columns.
#[test]
fn the_long_form_needs_44_columns_and_the_preview_89() {
    let f: &str = &STATUS_FORMAT;
    // `#{n:@cpd_preview}` + 12: the join, two caps and `preview: `
    assert_eq!(LONG_WIDTH + 27, 44, "the long form alone needs 44 columns");
    assert_eq!(1 + 2 + "preview: ".chars().count(), 12);
    assert_eq!(PREVIEW.chars().count(), 33);
    assert_eq!(
        LONG_WIDTH + 27 + PREVIEW.chars().count() + 12,
        89,
        "the long form fits from 89 columns with this preview"
    );
    // right after the buttons: a join, then the pill in reverse video on colour 3, unpadded and with no background
    assert!(
        after_the_buttons().starts_with(&preview_branch()),
        "{}",
        after_the_buttons()
    );
    assert!(
        preview_branch().contains("\u{e0b6}#[reverse]preview: #{@cpd_preview}#[noreverse]\u{e0b4}"),
        "{}",
        preview_branch()
    );
    assert!(!f.contains("bg="), "{f}");
    // the fill and the long form's test are shorter by the pill's width while the option is set
    assert!(
        fill_count().contains(PREVIEW_WIDTH) && long_fits().contains(PREVIEW_WIDTH),
        "{f}"
    );
    assert!(
        f.contains(&fill_count()) && f.contains(&long_fits()),
        "the format uses them: {f}"
    );
}

#[test]
fn the_status_line_shows_no_preview_once_the_option_is_unset() {
    let f: &str = &STATUS_FORMAT;
    // the pill is a conditional on the option with an empty else branch, and the width one with 0
    assert!(
        f.contains(&preview_branch()) && preview_branch().ends_with(",}"),
        "{f}"
    );
    assert!(PREVIEW_WIDTH.ends_with(",0}"), "{PREVIEW_WIDTH}");
    // and the option is used nowhere else: with those two forms taken out, no `@cpd_preview` is left
    let rest = f.replace(PREVIEW_WIDTH, "").replace(&preview_branch(), "");
    assert!(!rest.contains("@cpd_preview"), "{rest}");
}
