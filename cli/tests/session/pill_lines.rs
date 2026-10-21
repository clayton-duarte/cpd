//! Red tests for the pill and line colours of contract §6.1.1 (hs-d530, hs-d531), written from the docs: the exact formats and server options.

use cpd_core::host::tmux::{BORDER_FORMAT, SERVER_OPTIONS, STATUS_FORMAT};

/// The value of the server option `name` (set with `set -g` or `set -gq`).
fn option(name: &str) -> &'static str {
    let found: Vec<&&[&str]> = SERVER_OPTIONS.iter().filter(|o| o[0] == "set" && o[2] == name).collect();
    assert_eq!(found.len(), 1, "{name} is set once");
    found[0][3]
}

#[test]
fn pane_and_popup_borders_are_colour7_and_the_active_border_blue() {
    assert_eq!(option("pane-border-style"), "fg=colour7");
    assert_eq!(option("popup-border-style"), "fg=colour7");
    assert_eq!(option("pane-active-border-style"), "fg=blue");
}

#[test]
fn border_format_title_pills_follow_the_rule() {
    const CAP: &str = "#{?pane_active,#[fg=blue],#[fg=colour8]}";
    const TEXT: &str = "#{?pane_active,#[fg=colour0]#[bg=blue],#[fg=colour7]#[bg=colour8]}";
    let pill = |title: &str| format!("{CAP}━\u{e0b6}{TEXT}{title}{CAP}#[bg=default]\u{e0b4}");
    let want = format!(
        "#{{?#{{@cpd_session}},#{{?#{{@cpd_title}},{lead},}},\
         #{{?#{{==:#{{@cpd_view}},chat}},,#{{?#{{==:#{{@cpd_view}},history}},,{views}}}}}}}",
        lead = pill("#{@cpd_title}"),
        views = pill(
            "#{?#{==:#{@cpd_view},worker},Worker · #{@cpd_job} (Read-Only),#{?#{==:#{@cpd_view},tree},Context,\
             #{?#{==:#{@cpd_view},plan},Plan,#{?#{==:#{@cpd_view},widgets},Deploy,#{pane_current_command}}}}}"
        ),
    );
    assert_eq!(BORDER_FORMAT, want);
    assert!(!BORDER_FORMAT.contains("reverse"), "no reverse is left in the format");
}

#[test]
fn status_line_pills_follow_the_rule() {
    const JOIN: &str = "#[fg=colour8]\u{2500}#[default]";
    let button = |range: &str, glyph: &str| {
        format!("#[range=user|{range}]#[fg=colour15]\u{e0b6}#[fg=colour0]#[bg=colour15]{glyph}#[fg=colour15]#[bg=default]\u{e0b4}#[default]#[norange]")
    };
    let pill = |text: &str| format!("#[fg=colour8]\u{e0b6}#[fg=colour7]#[bg=colour8]{text}#[fg=colour8]#[bg=default]\u{e0b4}#[default]");
    let long = "#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{@cpd_workers_width},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?#{@cpd_version},#{e|+:#{n:@cpd_version},3},0}}}";
    let want = format!(
        "{JOIN}{}{JOIN}{}{JOIN}{}\
         #{{?#{{@cpd_preview}},#[fg=colour8]\u{2500}{},}}\
         #[fg=colour8]#{{R:\u{2500},#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},12}},#{{?#{{@cpd_preview}},#{{e|+:#{{n:@cpd_preview}},12}},0}}}},#{{?#{{@cpd_version}},#{{e|+:#{{n:@cpd_version}},3}},0}}}},\
         #{{e|+:#{{?{long},#{{@cpd_workers_width}},#{{@cpd_workers_short_width}}}},1}}}}}}\
         #[default]#[align=right]\
         {}{JOIN}{}\
         #{{?#{{@cpd_version}},#[fg=colour8]\u{2500}{},}}\
         {JOIN}",
        button("reset", cpd_core::host::tmux::RESET_GLYPH),
        button("legend", cpd_core::host::tmux::LEGEND_GLYPH),
        button("logs", cpd_core::host::tmux::LOGS_GLYPH),
        pill("preview: #{@cpd_preview}"),
        pill("#{@cpd_inbox}"),
        pill(&format!("#{{?{long},#{{@cpd_workers}},#{{@cpd_workers_short}}}}")),
        pill("#{@cpd_version}"),
    );
    assert_eq!(STATUS_FORMAT, want);
    for old in ["reverse", "colour236", "yellow", "push-default", "pop-default"] {
        assert!(!STATUS_FORMAT.contains(old), "{old} is gone from the status format");
    }
}
