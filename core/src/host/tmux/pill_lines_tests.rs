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
    let pill_button = |range: &str, colour: &str, text: &str| {
        format!("#[range=user|{range}]#[fg={colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]#[norange]")
    };
    // a glyph button's pill text is the glyph and one space; the runner's is text, with none
    let button = |range: &str, colour: &str, glyph: &str| pill_button(range, colour, &format!("{glyph} "));
    let pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]")
    };
    let italic_pill = |colour: &str, text: &str| {
        format!("#[fg={colour}]\u{e0b6}#[reverse]#[italics]{text}#[noitalics]#[noreverse]\u{e0b4}#[default]")
    };
    let long = "#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{w:@cpd_workers},2},22},#{?@cpd_runner,#{e|+:#{n:@cpd_runner},3},0}},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}},#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}}}";
    let incident_range = format!("#[range=user|incident]{}#[norange]", italic_pill("colour9", "Incident Mode: #{@cpd_incident}"));
    let want = format!(
        "{JOIN}{}{JOIN}{}{JOIN}{}{JOIN}{}\
         #{{?@cpd_runner,{JOIN}{},}}\
         #{{?#{{@cpd_preview}},#[fg=colour8]\u{2501}{},}}\
         #[fg=colour8]#{{R:\u{2501},#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},20}},#{{?@cpd_runner,#{{e|+:#{{n:@cpd_runner}},3}},0}}}},#{{?#{{@cpd_preview}},#{{e|+:#{{n:@cpd_preview}},12}},0}}}},#{{?@cpd_version,#{{e|+:#{{n:@cpd_version}},3}},0}}}},#{{?@cpd_incident,#{{e|+:#{{n:@cpd_incident}},18}},0}}}},\
         #{{e|+:#{{?{long},#{{e|+:#{{w:@cpd_workers}},2}},#{{e|+:#{{w:@cpd_workers_short}},2}}}},1}}}}}}\
         #[default]\
         #{{?@cpd_incident,{}{JOIN},}}\
         {}\
         #{{?#{{@cpd_host_alert}},#[fg=colour8]\u{2501}#[range=user|host]{}#[norange],#{{?#{{@cpd_version}},#[fg=colour8]\u{2501}{},}}}}\
         {JOIN}",
        button("reset", "colour15", super::RESET_GLYPH),
        button("legend", "colour15", super::LEGEND_GLYPH),
        button("logs", "colour15", super::LOGS_GLYPH),
        button("costs", "colour15", "\u{EEC1}"),
        pill_button("runner", "colour15", "#{@cpd_runner}"),
        pill("colour3", "preview: #{@cpd_preview}"),
        incident_range,
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
/// (long form from `#{w:@cpd_workers}` plus its two caps and 22: the left group's 20, the closing `━` and a fill of 1).
#[test]
fn the_status_format_is_the_buttons_the_grey_fill_line_and_the_right_pills() {
    let prefix = format!(
            "#[fg=colour8]\u{2501}#[default]\
             #[range=user|reset]#[fg=colour15]\u{e0b6}#[reverse]{} #[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2501}#[default]\
             #[range=user|legend]#[fg=colour15]\u{e0b6}#[reverse]{} #[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2501}#[default]\
             #[range=user|logs]#[fg=colour15]\u{e0b6}#[reverse]{} #[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2501}#[default]\
             #[range=user|costs]#[fg=colour15]\u{e0b6}#[reverse]{} #[noreverse]\u{e0b4}#[default]#[norange]",
            super::RESET_GLYPH,
            super::LEGEND_GLYPH,
            super::LOGS_GLYPH,
            "\u{EEC1}"
        );
    // The fifth item (pro-d199): the runner pill, after a join, only while `@cpd_runner` is set.
    let runner = "#[fg=colour8]\u{2501}#[default]#[range=user|runner]#[fg=colour15]\u{e0b6}#[reverse]#{@cpd_runner}#[noreverse]\u{e0b4}#[default]#[norange]";
    let after_costs = |cond: &str| format!("{prefix}#{{?{cond},{runner},}}");
    let rest = "#{?#{@cpd_preview},#[fg=colour8]\u{2501}#[fg=colour3]\u{e0b6}#[reverse]preview: #{@cpd_preview}#[noreverse]\u{e0b4}#[default],}\
         #[fg=colour8]";
    let suffix = "#[default]\
         #{?@cpd_incident,#[range=user|incident]#[fg=colour9]\u{e0b6}#[reverse]#[italics]Incident Mode: #{@cpd_incident}#[noitalics]#[noreverse]\u{e0b4}#[default]#[norange]#[fg=colour8]\u{2501}#[default],}\
         #[fg=colour8]\u{e0b6}#[reverse]\
         #{?#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{w:@cpd_workers},2},22},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}},#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}}},#{@cpd_workers},#{@cpd_workers_short}}\
         #[noreverse]\u{e0b4}#[default]\
         #{?#{@cpd_host_alert},#[fg=colour8]\u{2501}#[range=user|host]#[fg=colour9]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default]#[norange],#{?#{@cpd_version},#[fg=colour8]\u{2501}#[fg=colour8]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default],}}\
         #[fg=colour8]\u{2501}#[default]";
    let format = STATUS_FORMAT.as_str();
    // `#{?@cpd_runner,..}` as the version pill's width does, or `#{?#{@cpd_runner},..}` as the preview pill does.
    let starts = ["@cpd_runner", "#{@cpd_runner}"].map(|cond| format!("{}{rest}", after_costs(cond)));
    assert!(starts.iter().any(|p| format.starts_with(p.as_str())), "{format}");
    // The widths of the long form count the runner pill, so the suffix is pinned from the incident pill to its end
    // and the workers pill's text only by its shape.
    let incident = suffix.find("#{?@cpd_incident").unwrap();
    let workers_at = suffix.find("#[fg=colour8]\u{e0b6}#[reverse]#{?").unwrap();
    let tail = &suffix[suffix.find("#[noreverse]\u{e0b4}#[default]#{?#{@cpd_host_alert}").unwrap()..];
    assert!(incident < workers_at);
    assert!(format.contains(&suffix[incident..workers_at]), "{format}");
    assert!(format.ends_with(tail), "{format}");
}

/// pro-d199: the runner pill is the fifth item of the left group, after the costs button, in its own range, its text the
/// option's value alone between the caps.
#[test]
fn the_runner_pill_follows_the_costs_button_in_its_own_range() {
    use super::RUNNER_OPTION;
    assert_eq!(RUNNER_OPTION, "@cpd_runner");
    let format = STATUS_FORMAT.as_str();
    assert_eq!(format.matches("range=user|runner").count(), 1, "one runner range: {format}");
    let ranges: Vec<&str> = format
        .match_indices("#[range=user|")
        .map(|(i, _)| &format[i + "#[range=user|".len()..])
        .map(|r| &r[..r.find(']').expect("a closed range")])
        .collect();
    assert_eq!(ranges[..5], ["reset", "legend", "logs", "costs", "runner"], "{ranges:?}");
    let pill = "#[range=user|runner]#[fg=colour15]\u{e0b6}#[reverse]#{@cpd_runner}#[noreverse]\u{e0b4}#[default]#[norange]";
    let at = format.find(pill).unwrap_or_else(|| panic!("no runner pill in {format}"));
    let costs_end = format.find("range=user|costs").unwrap();
    assert!(at > costs_end && at < format.find("@cpd_preview").unwrap(), "after costs, before the preview marker: {format}");
    assert!(format[..at].ends_with("#[fg=colour8]\u{2501}#[default]") || format[..at].ends_with(&"#[fg=colour8]\u{2501}#[default]".to_string()), "{format}");
    // Drawn only while the option is set.
    let (before, after) = (&format[..at], &format[at + pill.len()..]);
    assert!(before.contains("#{?@cpd_runner,") || before.contains("#{?#{@cpd_runner},"), "{format}");
    assert!(after.starts_with(",}"), "{format}");
}

/// Display columns as tmux's format_width counts them: a closed `#[..]` is 0 cells, `##` is 1, every other char is 1.
fn display_width(s: &str) -> usize {
    let (mut n, mut rest) = (0, s);
    while let Some(c) = rest.chars().next() {
        if let Some(run) = rest.strip_prefix("#[") {
            rest = &run[run.find(']').expect("a closed #[") + 1..];
        } else if let Some(run) = rest.strip_prefix("##") {
            n += 1;
            rest = run;
        } else {
            n += 1;
            rest = &rest[c.len_utf8()..];
        }
    }
    n
}

/// A tiny evaluator of the subset of tmux's format language the status format uses, on a zoomed window (the plain
/// fill): `#[..]` styles cost no cell; `#{?c,a,b}`, `#{e|op:a,b}`, `#{n:x}`, `#{w:x}`, `#{R:s,n}`, `#{var}`.
struct Row<'a> {
    vars: &'a dyn Fn(&str) -> Option<String>,
}

impl Row<'_> {
    fn split(s: &str) -> Vec<&str> {
        let (mut out, mut depth, mut from) = (Vec::new(), 0, 0);
        for (i, c) in s.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    out.push(&s[from..i]);
                    from = i + 1;
                }
                _ => {}
            }
        }
        out.push(&s[from..]);
        out
    }

    fn var(&self, name: &str) -> String {
        (self.vars)(name).unwrap_or_default()
    }

    fn number(&self, s: &str) -> i64 {
        let v = self.expand(s);
        v.trim().parse().unwrap_or_else(|_| panic!("not a number: {v:?} from {s:?}"))
    }

    fn truthy(&self, cond: &str) -> bool {
        let v = if cond.starts_with('@') || !cond.contains(['#', ':']) { self.var(cond) } else { self.expand(cond) };
        !v.is_empty() && v != "0"
    }

    fn directive(&self, inner: &str) -> String {
        if let Some(rest) = inner.strip_prefix('?') {
            let parts = Self::split(rest);
            let branch = if self.truthy(parts[0]) { parts[1] } else { parts.get(2).copied().unwrap_or("") };
            return self.expand(branch);
        }
        if let Some(rest) = inner.strip_prefix("e|") {
            let (op, args) = rest.split_once(':').unwrap();
            let args = Self::split(args);
            let (a, b) = (self.number(args[0]), self.number(args[1]));
            return match op {
                "+" => a + b,
                "-" => a - b,
                ">=" => i64::from(a >= b),
                ">" => i64::from(a > b),
                "<" => i64::from(a < b),
                other => panic!("unread operator {other}"),
            }
            .to_string();
        }
        if let Some(name) = inner.strip_prefix("n:").or_else(|| inner.strip_prefix("w:")) {
            let value = self.var(name.trim_start_matches('#'));
            return if inner.starts_with("w:") { display_width(&value) } else { value.chars().count() }.to_string();
        }
        if let Some(rest) = inner.strip_prefix("R:") {
            let args = Self::split(rest);
            return self.expand(args[0]).repeat(self.number(args[1]).max(0) as usize);
        }
        self.var(inner)
    }

    fn expand(&self, s: &str) -> String {
        let (mut out, mut rest) = (String::new(), s);
        while let Some(c) = rest.chars().next() {
            if let Some(run) = rest.strip_prefix("#[") {
                rest = &run[run.find(']').expect("a closed #[") + 1..];
            } else if let Some(run) = rest.strip_prefix("#{") {
                let (mut depth, mut end) = (1, 0);
                for (i, c) in run.char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                out.push_str(&self.directive(&run[..end]));
                rest = &run[end + 1..];
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    }
}

/// The row is exactly `#{client_width}` cells with the runner pill showing `pi`, `durable` or not at all, with and without
/// the preview pill, at every width the long and the short workers form cover; the pill's text is on the row exactly when
/// the option is set, and the row begins with the left group of 4-cell glyph buttons.
#[test]
fn the_status_row_keeps_its_width_with_pi_with_durable_and_with_no_runner_option() {
    const PREVIEW: &str = "cpd.tui-visuals.box-order 6c43886";
    let glyphs = [super::RESET_GLYPH, super::LEGEND_GLYPH, super::LOGS_GLYPH, "\u{EEC1}"];
    for runner in [Some("pi"), Some("durable"), None] {
        let runner_width = runner.map_or(0, |r| r.chars().count() + 3);
        for preview in [None, Some(PREVIEW)] {
            let widths: Vec<usize> = match preview {
                None => [60, 80, 120, 215].to_vec(),
                Some(_) => [79, 95, 96, 120].iter().map(|w| w + runner_width).collect(),
            };
            for width in widths {
                let vars = |name: &str| {
                    Some(match name {
                        "client_width" => width.to_string(),
                        "window_zoomed_flag" => "1".to_owned(),
                        "@cpd_workers" => "2 running 1 waiting".to_owned(),
                        "@cpd_workers_short" => "2 1".to_owned(),
                        "@cpd_version" => "1.2.3".to_owned(),
                        "@cpd_runner" => runner?.to_owned(),
                        "@cpd_preview" => preview?.to_owned(),
                        _ => return None,
                    })
                };
                let row = Row { vars: &vars }.expand(&STATUS_FORMAT);
                let what = format!("runner {runner:?}, preview {preview:?}, width {width}: {row:?}");
                assert_eq!(display_width(&row), width, "{what}");
                if preview.is_some() {
                    assert_eq!(row.contains("2 running 1 waiting"), width >= 96 + runner_width, "{what}");
                }
                let mut left = String::from("\u{2501}");
                left.push_str(&glyphs.map(|g| format!("\u{e0b6}{g} \u{e0b4}")).join("\u{2501}"));
                if let Some(r) = runner {
                    left.push_str(&format!("\u{2501}\u{e0b6}{r}\u{e0b4}"));
                }
                assert!(row.starts_with(&left), "the row begins with the left group {left:?}: {what}");
                let pill = runner.map(|r| format!("\u{e0b6}{r}\u{e0b4}"));
                for r in ["pi", "durable"] {
                    let marker = format!("\u{e0b6}{r}\u{e0b4}");
                    assert_eq!(row.contains(&marker), pill.as_deref() == Some(marker.as_str()), "{what}");
                }
            }
        }
    }
}

/// The buttons' glyphs are the hs-d235 Material Design code points (width 1 is not asserted: no `unicode-width` dependency here).
#[test]
fn the_button_glyphs_are_the_hs_d235_code_points() {
    use super::{LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH};
    assert_eq!([RESET_GLYPH, LEGEND_GLYPH, LOGS_GLYPH], ["\u{F0A1D}", "\u{F0A01}", "\u{F09ED}"]);
    assert!(STATUS_FORMAT.contains("\u{EEC1}"), "the costs glyph (U+EEC1) is in the status format: {}", *STATUS_FORMAT);
}

/// pro-d189, pro-d195: the costs button is the fourth range, one pill of the glyph alone, the same whether costs show or not.
#[test]
fn the_costs_button_is_the_fourth_and_looks_the_same_whatever_the_state() {
    let format = STATUS_FORMAT.as_str();
    assert_eq!(format.matches("range=user|costs").count(), 1, "one costs range: {format}");
    let ranges: Vec<&str> = format
        .match_indices("#[range=user|")
        .map(|(i, _)| &format[i + "#[range=user|".len()..])
        .map(|r| &r[..r.find(']').expect("a closed range")])
        .collect();
    assert_eq!(ranges[..4], ["reset", "legend", "logs", "costs"], "the costs range is the fourth: {ranges:?}");
    assert!(!format.contains("@cpd_costs"), "no conditional on an option of the costs: {format}");
    let from = format.find("#[range=user|costs]").unwrap();
    let pill = &format[from..from + format[from..].find("#[norange]").expect("a closed range") + "#[norange]".len()];
    assert_eq!(
        pill,
        "#[range=user|costs]#[fg=colour15]\u{e0b6}#[reverse]\u{EEC1} #[noreverse]\u{e0b4}#[default]#[norange]",
        "the pill text is the glyph and one space"
    );
}

#[test]
fn the_statusbar_is_no_view() {
    assert_eq!(View::parse("statusbar"), None);
    assert!(View::ALL.iter().all(|v| v.as_str() != "statusbar"));
    assert_eq!(View::parse("nope"), None);
}

/// The plain long and short workers texts of Counts{running 6, limit 6, waiting 4, queued 9}.
#[test]
fn the_status_row_keeps_its_width_with_the_plain_workers_texts() {
    let short = "9\u{f0798}4\u{f0798}6/6".to_owned();
    let long = "workers 9\u{f0798}4\u{f0798}6/6".to_owned();
    assert_eq!((display_width(&long), display_width(&short)), (15, 7));
    for width in [30usize, 38, 39, 60, 120] {
        let vars = |name: &str| {
            Some(match name {
                "client_width" => width.to_string(),
                "window_zoomed_flag" => "1".to_owned(),
                "@cpd_workers" => long.clone(),
                "@cpd_workers_short" => short.clone(),
                _ => return None,
            })
        };
        let row = Row { vars: &vars }.expand(&STATUS_FORMAT);
        assert_eq!(display_width(&row), width, "width {width}: {row:?}");
        assert_eq!(row.contains("workers "), width >= 39, "the long text is drawn from 39 columns: {row:?}");
        assert!(row.contains(&short), "{row:?}");
    }
}

/// The substring of `STATUS_FORMAT` from `#[range=user|<name>]` to its `#[norange]`, inclusive.
fn range_of(name: &str) -> &'static str {
    let format = STATUS_FORMAT.as_str();
    let from = format.find(&format!("#[range=user|{name}]")).unwrap_or_else(|| panic!("no {name} range"));
    let len = format[from..].find("#[norange]").expect("a closed range") + "#[norange]".len();
    &format[from..from + len]
}

/// Each glyph button is the left cap, the glyph, ONE SPACE inside the pill, the right cap: 4 cells; the left group is 20.
#[test]
fn each_glyph_button_is_cap_glyph_space_cap_and_the_left_group_is_20_cells() {
    use super::{LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH};
    for (name, glyph) in [("reset", RESET_GLYPH), ("legend", LEGEND_GLYPH), ("logs", LOGS_GLYPH), ("costs", "\u{EEC1}")] {
        let pill = range_of(name);
        assert_eq!(
            pill,
            format!("#[range=user|{name}]#[fg=colour15]\u{e0b6}#[reverse]{glyph} #[noreverse]\u{e0b4}#[default]#[norange]"),
            "{name}: cap, glyph, one space, cap"
        );
        assert_eq!(display_width(pill), 4, "{name}");
    }
    let format = STATUS_FORMAT.as_str();
    let costs = range_of("costs");
    let end = format.find(costs).unwrap() + costs.len();
    assert_eq!(display_width(&format[..end]), 20, "the left group: `━`, 4 buttons of 4 cells and 3 joins");
    assert_eq!(
        range_of("runner"),
        "#[range=user|runner]#[fg=colour15]\u{e0b6}#[reverse]#{@cpd_runner}#[noreverse]\u{e0b4}#[default]#[norange]",
        "the runner pill is text, with no space"
    );
}

/// A click range wraps its whole pill (caps included); tmux's range also owns the cell after it, the join. The space is
/// inside the range, so the four ranges start at columns 1, 6, 11 and 16 and own 5 cells each, without overlap.
#[test]
fn a_click_range_wraps_its_whole_pill_and_the_space_shifts_no_other_range() {
    const JOIN: &str = "#[fg=colour8]\u{2501}#[default]";
    let format = STATUS_FORMAT.as_str();
    let mut owned: Vec<std::ops::RangeInclusive<usize>> = Vec::new();
    let mut previous_end = 0;
    for (name, start) in [("reset", 1), ("legend", 6), ("logs", 11), ("costs", 16)] {
        let pill = range_of(name);
        let at = format.find(pill).unwrap();
        assert_eq!(display_width(&format[..at]), start, "{name} starts at column {start}");
        assert!(pill.starts_with(&format!("#[range=user|{name}]#[fg=colour15]\u{e0b6}")), "the range opens before the left cap: {pill}");
        assert!(pill.ends_with("\u{e0b4}#[default]#[norange]"), "the range closes after the right cap: {pill}");
        assert_eq!(display_width(pill), 4, "{name} covers 4 cells");
        let own = start..=start + 4;
        assert!(owned.iter().all(|o| o.end() < own.start()), "{name} owns {own:?}, overlapping {owned:?}");
        owned.push(own);
        if previous_end > 0 {
            assert_eq!(&format[previous_end..at], JOIN, "exactly one join outside every range before {name}");
        } else {
            assert_eq!(&format[..at], JOIN, "the leading `━` (col 0) is outside every range");
        }
        previous_end = at + pill.len();
    }
    assert_eq!(owned, [1..=5, 6..=10, 11..=15, 16..=20]);
}
