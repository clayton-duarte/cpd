//! The pill and line colours of contract §6.1.1 (hs-d530, hs-d531), written from the docs: the exact formats and server
//! options; and the static facts of `host.rs` of the `session` tests, which read these constants and start no tmux.

use super::{BORDER_FORMAT, SERVER_OPTIONS, STATUS_FORMAT, STATUS_CLICK};
use crate::host::layout::View;

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

/// Expands `BORDER_FORMAT` with the fake's evaluator, for a pane with these variables.
fn border(session: &str, title: &str, view: &str, active: bool, command: &str) -> String {
    // `None` for a name that is no variable: the evaluator then expands a `#{..}` condition itself.
    crate::fakes::tmux::format::expand(&BORDER_FORMAT, &|name: &str| {
        Some(match name {
            "@cpd_session" => session.to_owned(),
            "@cpd_title" => title.to_owned(),
            "@cpd_view" => view.to_owned(),
            "pane_active" => (if active { "1" } else { "0" }).to_owned(),
            "pane_current_command" => command.to_owned(),
            _ => return None,
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Style {
    Base,
    Colour(u8),
}

/// The styled cells of an expanded format, as tmux 3.8 draws it (`format_draw`, `style_parse`): `default` is the base
/// style, `fg=colourN` sets the foreground; any other directive panics.
fn cells(expansion: &str) -> Vec<(char, Style)> {
    let mut out = Vec::new();
    let mut style = Style::Base;
    let mut rest = expansion;
    while let Some(c) = rest.chars().next() {
        if let Some(run) = rest.strip_prefix("#[") {
            let end = run.find(']').expect("a closed #[");
            for directive in run[..end].split(',') {
                style = match directive {
                    "default" => Style::Base,
                    d => match d.strip_prefix("fg=colour") {
                        Some(n) => Style::Colour(n.parse().expect("a colour number")),
                        None => panic!("unread directive {d}"),
                    },
                };
            }
            rest = &run[end + 1..];
        } else {
            out.push((c, style));
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

#[test]
fn border_mark_tree_view_draws_the_three_glyphs_in_13_15_10_with_border_styled_lines_between() {
    use Style::{Base, Colour};
    let want = vec![
        ('\u{f0bf2}', Colour(13)),
        ('━', Base),
        ('\u{f0c19}', Colour(15)),
        ('━', Base),
        ('\u{f0bf5}', Colour(10)),
    ];
    for active in [true, false] {
        let expansion = border("", "", "tree", active, "cpd");
        let mut got: Vec<_> = cells(&expansion).into_iter().filter(|c| *c != (' ', Base)).collect();
        while got.last() == Some(&('━', Base)) {
            got.pop();
        }
        assert_eq!(got, want, "active {active}: {expansion:?}");
        assert!(!expansion.contains('\u{e0b6}') && !expansion.contains('\u{e0b4}'), "{expansion:?}");
        assert!(!expansion.contains("reverse") && !expansion.contains("Context"), "{expansion:?}");
        assert!(cells(&expansion).iter().all(|(c, s)| *c != '━' || *s == Base), "{expansion:?}");
    }
}

#[test]
fn border_mark_gives_every_glyph_an_empty_cell_after_it_so_the_terminal_draws_it_big() {
    use Style::Base;
    let glyphs = ['\u{f0bf2}', '\u{f0c19}', '\u{f0bf5}'];
    for active in [true, false] {
        let got = cells(&border("", "", "tree", active, "cpd"));
        for glyph in glyphs {
            let i = got.iter().position(|(c, _)| *c == glyph).expect("a glyph cell");
            assert_eq!(got.get(i + 1), Some(&(' ', Base)), "active {active}: glyph {glyph:?} in {got:?}");
        }
        let d = got.iter().position(|(c, _)| *c == glyphs[2]).unwrap();
        assert!(got[d + 2..].iter().all(|c| *c == ('━', Base)), "active {active}: {got:?}");
    }
}

#[test]
fn border_mark_plan_and_widgets_views_draw_no_title() {
    for view in ["plan", "widgets"] {
        for active in [true, false] {
            assert_eq!(border("", "", view, active, "cpd"), "", "{view} active {active}");
        }
    }
}

#[test]
fn border_format_title_pills_follow_the_rule() {
    for (active, colour) in [(true, 4), (false, 7)] {
        let pill = |text: &str| format!("━#[fg=colour{colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}");
        assert_eq!(border("lead-a", "Retry Payouts", "", active, "cpd"), pill("Retry Payouts"));
        assert_eq!(border("", "", "", active, "zsh"), pill("zsh"));
        assert_eq!(border("lead-a", "", "", active, "cpd"), "");
        assert_eq!(border("", "", "chat", active, "cpd"), "");
        assert_eq!(border("", "", "history", active, "cpd"), "");
    }
    let colour_zero = format!("colour{}", 0);
    assert!(!BORDER_FORMAT.contains("#[fg=colour7]━"));
    assert!(!BORDER_FORMAT.contains(&colour_zero), "no colour 0 in the format");
}

#[test]
fn status_line_pills_follow_the_rule() {
    const JOIN: &str = "#[fg=colour8]\u{2501}#[default]";
    let button = |range: &str, colour: &str, glyph: &str| {
        format!("#[range=user|{range}]#[fg={colour}]\u{e0b6}#[reverse]{glyph}#[noreverse]\u{e0b4}#[default]#[norange]")
    };
    let pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]")
    };
    let italic_pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]#[italics]{text}#[noitalics]#[noreverse]\u{e0b4}#[default]")
    };
    let long = "#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{w:@cpd_workers},2},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}},#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}}}";
    let want = format!(
        "{JOIN}{}{JOIN}{}{JOIN}{}\
         #{{?#{{@cpd_preview}},#[fg=colour8]\u{2501}{},}}\
         #[fg=colour8]#{{R:\u{2501},#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},12}},#{{?#{{@cpd_preview}},#{{e|+:#{{n:@cpd_preview}},12}},0}}}},#{{?@cpd_version,#{{e|+:#{{n:@cpd_version}},3}},0}}}},#{{?@cpd_incident,#{{e|+:#{{n:@cpd_incident}},18}},0}}}},\
         #{{e|+:#{{?{long},#{{e|+:#{{w:@cpd_workers}},2}},#{{e|+:#{{w:@cpd_workers_short}},2}}}},1}}}}}}\
         #[default]\
         #{{?@cpd_incident,{}{JOIN},}}\
         {}\
         #{{?#{{@cpd_host_alert}},#[fg=colour8]\u{2501}#[range=user|host]{}#[norange],#{{?#{{@cpd_version}},#[fg=colour8]\u{2501}{},}}}}\
         {JOIN}",
        button("reset", "colour13", super::RESET_GLYPH),
        button("legend", "colour15", super::LEGEND_GLYPH),
        button("logs", "colour10", super::LOGS_GLYPH),
        pill("colour3", "preview: #{@cpd_preview}"),
        format!("#[range=user|incident]{}#[norange]", italic_pill("colour9", "Incident Mode: #{@cpd_incident}")),
        pill("colour8", &format!("#{{?{long},#{{@cpd_workers}},#{{@cpd_workers_short}}}}")),
        pill("colour9", "#{@cpd_version}"),
        pill("colour8", "#{@cpd_version}"),
    );
    // The prefix (up to the fill's colour) and the suffix (the right group) are pinned; the fill is pinned by behaviour.
    let fill = want.find("#[fg=colour8]#{R:").expect("the fill") + "#[fg=colour8]".len();
    let right = want.find("#[default]#{?@cpd_incident").expect("the right group");
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
    scan(&root.join("../cli/src"), &forbidden, &mut offenders);
    assert!(offenders.is_empty(), "colour 0 or black named as text:\n{}", offenders.join("\n"));
}

/// Contract §6.1 (hs-d489): the borders of unfocused panes are grey, `pane-border-style fg=colour7` set right before
/// `pane-active-border-style`.
#[test]
fn the_pane_border_style_is_a_server_option_set_right_before_the_active_border_style() {
    let at = SERVER_OPTIONS.iter().position(|o| *o == ["set", "-g", "pane-active-border-style", "fg=blue"]).unwrap();
    assert_eq!(SERVER_OPTIONS[at.checked_sub(1).unwrap()], ["set", "-g", "pane-border-style", "fg=colour7"]);
}

/// Contract §6.1 (hs-d353): tmux 3.8 defaults the popup, popup border and menu styles to a grey background, so
/// cpd sets each to `default`, with `-gq`, right after `pane-active-border-style`.
#[test]
fn the_popup_and_menu_styles_are_server_options_set_to_default_after_the_active_border() {
    let at = SERVER_OPTIONS.iter().position(|o| *o == ["set", "-g", "pane-active-border-style", "fg=blue"]).unwrap();
    let want: [&[&str]; 3] = [
        &["set", "-gq", "popup-style", "default"],
        &["set", "-gq", "popup-border-style", "fg=colour7"],
        &["set", "-gq", "menu-style", "fg=colour15"],
    ];
    assert_eq!(&SERVER_OPTIONS[at + 1..at + 4], &want);
}

/// How a right click reaches the tree (commands.md §13.2): cpd adds no `MouseDown3Pane` binding, so on a server it
/// configured the key is tmux's own default (which selects the pane and passes the click on to an app that captures
/// the mouse), and cpd's own bindings are the left-click pair, `MouseDown1Pane` and the status line's `cpd tui action`.
/// (Replaces the host.rs test that read tmux 3.4's own default back with `list-keys`: that is tmux's, this is ours.)
#[test]
fn a_right_click_is_left_to_tmuxs_default_binding_and_the_status_click_runs_cpd_tui_action() {
    let binds: Vec<&Vec<String>> = SERVER_OPTIONS.iter().filter(|o| o[0] == "bind").collect();
    assert!(binds.iter().all(|o| o[3] != "MouseDown3Pane"), "cpd adds no MouseDown3Pane binding: {binds:?}");
    let status: Vec<&&Vec<String>> = binds.iter().filter(|o| o[3] == "MouseDown1Status").collect();
    assert_eq!(status.len(), 1, "{binds:?}");
    assert_eq!(status[0][..], ["bind", "-T", "root", "MouseDown1Status", "run-shell", "-b", STATUS_CLICK]);
    assert!(STATUS_CLICK.starts_with("cpd tui action "), "{STATUS_CLICK}");
}

/// The status line's format, exactly (hs-d479): one grey `━` line across the row, starting and ending with a `━`; the
/// three button pills each in its own range, joined by a single `━` outside the ranges; the optional unpadded
/// preview pill after a join; the fill; then the workers' pill and the closing `━`
/// (long form from `#{w:@cpd_workers}` plus its two caps and 14: the left group's 12, the closing `━` and a fill of 1).
#[test]
fn the_status_format_is_the_buttons_the_grey_fill_line_and_the_right_pills() {
    let prefix = format!(
            "#[fg=colour8]\u{2501}#[default]\
             #[range=user|reset]#[fg=colour13]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2501}#[default]\
             #[range=user|legend]#[fg=colour15]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2501}#[default]\
             #[range=user|logs]#[fg=colour10]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]",
            super::RESET_GLYPH,
            super::LEGEND_GLYPH,
            super::LOGS_GLYPH
        );
    let prefix = prefix + "#{?#{@cpd_preview},#[fg=colour8]\u{2501}#[fg=colour3]\u{e0b6}#[reverse]preview: #{@cpd_preview}#[noreverse]\u{e0b4}#[default],}\
         #[fg=colour8]";
    let suffix = "#[default]\
         #{?@cpd_incident,#[range=user|incident]#[fg=colour9]\u{e0b6}#[reverse]#[italics]Incident Mode: #{@cpd_incident}#[noitalics]#[noreverse]\u{e0b4}#[default]#[norange]#[fg=colour8]\u{2501}#[default],}\
         #[fg=colour8]\u{e0b6}#[reverse]\
         #{?#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{w:@cpd_workers},2},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}},#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}}},#{@cpd_workers},#{@cpd_workers_short}}\
         #[noreverse]\u{e0b4}#[default]\
         #{?#{@cpd_host_alert},#[fg=colour8]\u{2501}#[range=user|host]#[fg=colour9]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default]#[norange],#{?#{@cpd_version},#[fg=colour8]\u{2501}#[fg=colour8]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default],}}\
         #[fg=colour8]\u{2501}#[default]";
    let format = STATUS_FORMAT.as_str();
    assert!(format.starts_with(&prefix), "{format}");
    assert!(format.ends_with(suffix), "{format}");
}

/// The buttons' glyphs are the hs-d235 Material Design code points (width 1 is not asserted: no `unicode-width` dependency here).
#[test]
fn the_button_glyphs_are_the_hs_d235_code_points() {
    use super::{LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH};
    assert_eq!([RESET_GLYPH, LEGEND_GLYPH, LOGS_GLYPH], ["\u{F0A1D}", "\u{F0A01}", "\u{F09ED}"]);
}

#[test]
fn the_statusbar_is_no_view() {
    assert_eq!(View::parse("statusbar"), None);
    assert!(View::ALL.iter().all(|v| v.as_str() != "statusbar"));
    assert_eq!(View::parse("nope"), None);
}
