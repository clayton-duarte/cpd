//! Red tests for the pill and line colours of contract §6.1.1 (hs-d530, hs-d531), written from the docs: the exact formats and server options.

use cpd_core::host::tmux::{BORDER_FORMAT, SERVER_OPTIONS, STATUS_FORMAT};

/// The value of the server option `name` (set with `set -g` or `set -gq`).
fn option(name: &str) -> &'static str {
    let found: Vec<&Vec<String>> = SERVER_OPTIONS.iter().filter(|o| o[0] == "set" && o[2] == name).collect();
    assert_eq!(found.len(), 1, "{name} is set once");
    &found[0][3]
}

#[test]
fn pane_and_popup_borders_are_colour7_and_the_active_border_blue() {
    assert_eq!(option("pane-border-style"), "fg=colour7");
    assert_eq!(option("popup-border-style"), "fg=colour7");
    assert_eq!(option("pane-active-border-style"), "fg=blue");
}

#[test]
fn border_format_title_pills_follow_the_rule() {
    const CAP: &str = "#{?pane_active,#[fg=colour4],#[fg=colour7]}━\u{e0b6}#[reverse]";
    let pill = |title: &str| format!("{CAP}{title}#[noreverse]\u{e0b4}");
    let want = format!(
        "#{{?#{{@cpd_session}},#{{?#{{@cpd_title}},{lead},}},\
         #{{?#{{==:#{{@cpd_view}},chat}},,#{{?#{{==:#{{@cpd_view}},history}},,{views}}}}}}}",
        lead = pill("#{@cpd_title}"),
        views = pill(
            "#{?#{==:#{@cpd_view},worker},Worker · #{@cpd_job} (Read-Only),#{?#{==:#{@cpd_view},tree},Context,\
             #{?#{==:#{@cpd_view},plan},Plan,#{?#{==:#{@cpd_view},widgets},Deploy,#{pane_current_command}}}}}"
        ),
    );
    assert_eq!(*BORDER_FORMAT, want);
    let colour_zero = format!("colour{}", 0);
    assert_eq!(BORDER_FORMAT.matches("#[reverse]").count(), 2, "one #[reverse] per pill");
    assert_eq!(BORDER_FORMAT.matches("#[noreverse]").count(), 2, "one #[noreverse] per pill");
    assert!(!BORDER_FORMAT.contains(&colour_zero), "no colour 0 in the format");
}

#[test]
fn status_line_pills_follow_the_rule() {
    const JOIN: &str = "#[fg=colour8]\u{2500}#[default]";
    let button = |range: &str, glyph: &str| {
        format!("#[range=user|{range}]#[fg=colour15]\u{e0b6}#[reverse]{glyph}#[noreverse]\u{e0b4}#[default]#[norange]")
    };
    let pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]")
    };
    let italic_pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]#[italics]{text}#[noitalics]#[noreverse]\u{e0b4}#[default]")
    };
    let long = "#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{@cpd_workers_width},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}},#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}}}";
    let want = format!(
        "{JOIN}{}{JOIN}{}{JOIN}{}\
         #{{?#{{@cpd_preview}},#[fg=colour8]\u{2500}{},}}\
         #[fg=colour8]#{{R:\u{2500},#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},12}},#{{?#{{@cpd_preview}},#{{e|+:#{{n:@cpd_preview}},12}},0}}}},#{{?@cpd_version,#{{e|+:#{{n:@cpd_version}},3}},0}}}},\
         #{{e|+:#{{?{long},#{{@cpd_workers_width}},#{{@cpd_workers_short_width}}}},1}}}}}}\
         #[default]#[align=right]\
         #{{?@cpd_incident,{}{JOIN},}}\
         {}\
         #{{?#{{@cpd_version}},#[fg=colour8]\u{2500}{},}}\
         {JOIN}",
        button("reset", cpd_core::host::tmux::RESET_GLYPH),
        button("legend", cpd_core::host::tmux::LEGEND_GLYPH),
        button("logs", cpd_core::host::tmux::LOGS_GLYPH),
        pill("colour3", "preview: #{@cpd_preview}"),
        italic_pill("colour9", "Incident Mode: #{@cpd_incident}"),
        pill("colour8", &format!("#{{?{long},#{{@cpd_workers}},#{{@cpd_workers_short}}}}")),
        pill("colour8", "#{@cpd_version}"),
    );
    // The prefix (up to the fill's colour) and the suffix (the right group) are pinned; the fill is pinned by behaviour.
    let fill = want.find("#[fg=colour8]#{R:").expect("the fill") + "#[fg=colour8]".len();
    let right = want.find("#[default]#[align=right]").expect("the right group");
    assert!(STATUS_FORMAT.starts_with(&want[..fill]), "the prefix: {}", *STATUS_FORMAT);
    assert!(STATUS_FORMAT.ends_with(&want[right..]), "the suffix: {}", *STATUS_FORMAT);
    for old in ["colour236", "push-default", "pop-default"] {
        assert!(!STATUS_FORMAT.contains(old), "{old} is gone from the status format");
    }
}

#[test]
fn menu_selected_and_mode_styles_are_the_blue_block_inverted() {
    assert_eq!(option("menu-selected-style"), "fg=blue,reverse");
    assert_eq!(option("mode-style"), "fg=blue,reverse");
}

/// Walks every `*.rs` under `dir`, appending `path:line: text` for each line holding a forbidden spelling.
fn scan(dir: &std::path::Path, forbidden: &[String], offenders: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            scan(&path, forbidden, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") {
            for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                if forbidden.iter().any(|f| line.contains(f.as_str())) {
                    offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn no_tmux_format_or_option_names_colour_0_or_black_as_text() {
    // Built from parts so that this file does not name them itself.
    let forbidden: Vec<String> = vec![
        format!("fg=colour{}", 0),
        format!("fg=color{}", 0),
        format!("fg={}", "black"),
        format!("fg={}", 0),
        format!("[{}m", 3 * 10),
        format!("38;5;{}", 0),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    scan(&root.join("src"), &forbidden, &mut offenders);
    scan(&root.join("../core/src"), &forbidden, &mut offenders);
    assert!(offenders.is_empty(), "colour 0 or black named as text:\n{}", offenders.join("\n"));
}
