//! The typed tmux wrapper (contract §6.1, §6.3, with commands.md §19
//! amendment 9: `respawn-pane`).
//!
//! Every call is `tmux -L <socket> -f /dev/null …`, spawned from an argv (never
//! a shell), with `TMUX` and `TMUX_PANE` removed from its environment. `-f
//! /dev/null` only matters to the call that starts the server, but passing it
//! on every call means no call can ever start one that reads the User's config.
//! A call that exits non-zero where cpd did not expect it is `tmux_failed`
//! (exit 1) with `argv`, `exit` and the last 60 lines of stderr (commands.md §1.3).
//! A call that runs past [`DEADLINE`] is killed and fails `tool_timeout` (§6.3),
//! except the [`INTERACTIVE`] ones.
//!
//! Every call is a UTF-8 client, whatever the caller's locale (hs-d463; contract
//! §6.1): the argv is `tmux -u -L <socket> -f /dev/null …`. Without a UTF-8 locale
//! tmux prints the tabs of a `-F` format as `_`, and `list-panes` then fails to
//! parse. `-u` and not `LANG`: tmux reads `LC_ALL` before `LANG`, and `-u` needs no
//! installed locale. This file is the one place that spawns tmux (`Tmux::argv`,
//! `Tmux::command`, `client`, which `installed_version` and `doctor` use); a test
//! refuses a spawn of tmux anywhere else.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use crate::clock::{Clock, SystemClock};
use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;
use crate::pill::spec;
use crate::ports::Ports;

use super::deadline::Failed;
use super::runner::Runner;
use super::spawn::{Pid as ProcessId, Spawn as ProcessPort};
use super::layout::{LEGACY_VIEWS, View};

/// Every tmux call's deadline (contract §6.3): a local socket answers in
/// milliseconds, so a longer wait means a hung server.
const DEADLINE: Duration = Duration::from_secs(10);
/// The calls with no deadline (contract §6.3): `display-popup` returns when the
/// User closes the popup, which may be any time later.
const INTERACTIVE: &[&str] = &["display-popup"];

/// The deadline of the call `args`; `None` for an [`INTERACTIVE`] one.
fn deadline_of(args: &[String]) -> Option<Duration> {
    match args.first() {
        Some(call) if INTERACTIVE.contains(&call.as_str()) => None,
        _ => Some(DEADLINE),
    }
}

/// The tmux program, found on `PATH`.
const PROGRAM: &str = "tmux";
/// tmux's flag for a UTF-8 client (hs-d463): without it, and without a UTF-8 locale, tmux prints the tabs of a `-F`
/// format as `_`. It is the first argument after the program, the one place that puts it there is [`client`].
const UTF8: &str = "-u";

/// The one `Command` for a tmux program (hs-d463; contract §6.1): `<program> -u`, with `TMUX` and `TMUX_PANE` removed and no stdin.
/// Every spawn of tmux in cpd starts from it, so every call is a UTF-8 client whatever the caller's locale.
pub fn client(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.arg(UTF8).env_remove("TMUX").env_remove("TMUX_PANE").env_remove("CPD_SESSION").env_remove("CPD_ATTEMPT").stdin(Stdio::null());
    cmd
}

/// tmux's `-x`/`-y` for a new session (contract §6.3).
pub const SESSION_WIDTH: u32 = 215;
pub const SESSION_HEIGHT: u32 = 62;

/// The pane option naming the Session a pane runs (contract §6.2).
pub const OPT_SESSION: &str = "@cpd_session";
/// The pane option holding a Lead pane's header: its Plan's title, or a side
/// fork's name. A label for people that cpd keeps current, the one exception
/// to "pane options identify panes, never state" (contract §6.2).
pub const OPT_TITLE: &str = "@cpd_title";
/// The pane option naming the TUI view a pane runs (contract §6.2, amendment 2).
pub const OPT_VIEW: &str = "@cpd_view";
/// The global tmux option holding the Worker runner (`pi` or `durable`) the status line's runner pill shows.
pub const RUNNER_OPTION: &str = "@cpd_runner";
/// The pane option naming the Session a history pane shows (commands.md §10.12).
pub const OPT_HISTORY: &str = "@cpd_history";

/// The three dango base (unfocused) colours, in the cursor cycle order (hs-d648, hs-d671):
/// sakura 5, shiro 7, midori 2. Their clickable pair, `spec().clickable(base)` (13, 15, 10),
/// colours the tree pane's border mark (hs-d729); the status buttons use [`BUTTON_BASE`] (pro-d195).
const DANGO: [u8; 3] = [5, 7, 2];

/// U+F0BF2, the C of CPD.
const MARK_C: &str = "\u{f0bf2}";
/// U+F0C19, the P of CPD.
const MARK_P: &str = "\u{f0c19}";
/// U+F0BF5, the D of CPD.
const MARK_D: &str = "\u{f0bf5}";

/// The pane border titles (contract §6.1), keyed by the pane options, first
/// match: a Lead's pane (`@cpd_session`) shows its `@cpd_title` pill when set, else
/// nothing; the chat placeholder, a history pane, the plan view and the widgets view have
/// none; the tree view shows the CPD mark (hs-d729); any other pane shows its command as a pill.
/// A pill (hs-d266, hs-d475) is the text (reversed) on a block with no padding, between a
/// rounded cap on each side, `\u{e0b6}` and `\u{e0b4}`, grey (colour 7, hs-d534) and blue on the
/// focused pane, set in the format itself (`pane_active`). tmux draws two border line
/// characters before the format, then the pill's `━`, which carries no colour: it keeps the
/// base style the format is drawn from, the pane's own border style (blue on the active
/// pane, else colour 7; tmux 3.8 `window_make_pane_status`), so the left cap lands at
/// column 3 from the edge (hs-d478).
/// The mark is the three glyphs in the clickable dango colours (13, 15, 10), joined by `━`, with
/// no leading `━` (the first cell of the format is the C glyph, after tmux's two). Each glyph
/// ends with `#[default]`: a `#[fg=...]` would otherwise run into every later cell, and
/// `#[default]` returns to the base style, the border's own, so the `━` between them is
/// grey or blue like the line around it. One space follows each `#[default]` (hs-d730): the
/// terminal draws a Nerd Font glyph full size only with an empty cell after it (observed in
/// Ghostty; no option forces it), so the mark is `G #[default]`, a space, `━`, ... and a
/// trailing space after the D. tmux counts the space as a cell, so each glyph is 2 cells wide.
pub static BORDER_FORMAT: LazyLock<String> = LazyLock::new(|| {
    let plain = pill::fg_if("pane_active", spec().meanings.focus, spec().meanings.plain);
    let title = |text: &str| format!("━{}", pill::pill(&plain, "", text));
    let mark = [MARK_C, MARK_P, MARK_D]
        .iter()
        .zip(DANGO)
        .map(|(glyph, base)| format!("{}{glyph}#[default] ", pill::fg(spec().clickable(base))))
        .collect::<Vec<_>>()
        .join("━");
    format!(
        "#{{?#{{@cpd_session}},#{{?#{{@cpd_title}},{},}},#{{?#{{==:#{{@cpd_view}},chat}},,#{{?#{{==:#{{@cpd_view}},history}},,#{{?#{{==:#{{@cpd_view}},tree}},{mark},#{{?#{{==:#{{@cpd_view}},plan}},,#{{?#{{==:#{{@cpd_view}},widgets}},,{}}}}}}}}}}}}}",
        title("#{@cpd_title}"),
        title("#{pane_current_command}"),
    )
});

/// What the supervising hooks run when the engine's pane died or its window is
/// gone (contract §6.6): `watch --ensure --wait`, which starts the engine again
/// (commands.md §12). `cpd` is found on `PATH` as for every pane cpd starts
/// (`session::PROGRAM`), and its output is dropped, since a `run-shell -b` that
/// prints puts the active pane into view mode; `ensure` logs its failures in
/// `run/watch.log`. A macro, so both hooks' strings are built from this one.
macro_rules! engine_ensure {
    () => {
        "run-shell -b 'cpd watch --ensure --wait >/dev/null 2>&1'"
    };
}

/// The `pane-died` hook (contract §6.1, §6.6): a pane that carries `@cpd_view`
/// (a view of the cockpit) is respawned when its program dies; the pane of the
/// engine (window `watch`, `host::layout::WATCH_WINDOW`) starts the engine
/// again with `engine_ensure!`; any other pane (a Lead's) stays dead, as
/// `remain-on-exit` leaves it, so engine E2 sees its death. tmux runs it with
/// the dead pane as the current target.
pub const PANE_DIED_HOOK: &str = concat!(
    r##"if -F "#{@cpd_view}" "respawn-pane" "if -F '#{==:#{window_name},watch}' \""##,
    engine_ensure!(),
    r##"\"""##
);

/// The `window-unlinked` hook (contract §6.6): the engine's window is gone
/// (`kill-window`, an install's restart), so no pane died to say so: start the
/// engine again with `engine_ensure!`. The window is already gone, so its name
/// is what tells it was the engine's (`hook_window_name`).
pub const WINDOW_UNLINKED_HOOK: &str =
    concat!(r##"if -F "#{==:#{hook_window_name},watch}" ""##, engine_ensure!(), r##"""##);

/// Every global option, key binding and hook of contract §6.1, each as the
/// arguments after `tmux -L <socket>`, in the order of the reference's
/// `tui/scripts/tmux.conf` (lines 2–17), renamed (`@cpd_view`).
/// Set at every server start, whichever command starts it (see
/// [`new_session_args`]), and again by [`Tmux::apply_server_options`].
///
/// The click bindings: a click on a pane is tmux's own `MouseDown1Pane`
/// (`select-pane -t = ; send-keys -M`), bound again so that a server that
/// still has cpd's earlier StatusBar override gets the default back; a click
/// on a StatusBar button is [`STATUS_CLICK`].
pub static SERVER_OPTIONS: LazyLock<Vec<Vec<String>>> = LazyLock::new(|| {
    vec![
        strings(&["set", "-g", "mouse", "on"]),
        strings(&["set", "-g", "focus-events", "on"]),
        strings(&["set", "-g", "default-terminal", "tmux-256color"]),
        strings(&["set", "-g", "extended-keys", "on"]),
        strings(&["set", "-gq", "extended-keys-format", "csi-u"]),
        strings(&["set", "-as", "terminal-features", TERMINAL_FEATURES]),
        strings(&["set", "-g", "escape-time", "10"]),
        strings(&["set", "-g", "status", "off"]),
        strings(&["set", "-g", "pane-border-status", "top"]),
        strings(&["set", "-g", "pane-border-lines", "heavy"]),
        vec!["set".into(), "-g".into(), "pane-border-format".into(), BORDER_FORMAT.clone()],
        // Unfocused borders are grey like their titles (hs-d489).
        strings(&["set", "-g", "pane-border-style", "fg=colour7"]),
        strings(&["set", "-g", "pane-active-border-style", "fg=blue"]),
        // tmux 3.8 greys popups and menus by default (bg=themedarkgrey); the cockpit's dialogs keep the terminal's own
        // background (User, hs-d353); the popup frame is colour 7, a draggable line (§6.1.1 R4). `-q`: an older tmux does not know them.
        strings(&["set", "-gq", "popup-style", "default"]),
        strings(&["set", "-gq", "popup-border-style", "fg=colour7"]),
        // Menus and copy mode follow the palette (hs-d534): entries are clickable text, the border a muted line, a selection the blue block.
        strings(&["set", "-gq", "menu-style", "fg=colour15"]),
        strings(&["set", "-gq", "menu-border-style", "fg=colour8"]),
        strings(&["set", "-gq", "menu-selected-style", "fg=blue,reverse"]),
        strings(&["set", "-gq", "mode-style", "fg=blue,reverse"]),
        strings(&["set", "-g", "remain-on-exit", "on"]),
        strings(&["set", "-g", "set-titles", "on"]),
        strings(&["set", "-g", "set-titles-string", SET_TITLES_STRING]),
        strings(&["set-hook", "-g", "pane-died", PANE_DIED_HOOK]),
        strings(&["set-hook", "-g", "window-unlinked", WINDOW_UNLINKED_HOOK]),
        strings(&["set", "-s", "exit-empty", "off"]),
        strings(&["bind", "-T", "root", "MouseDown1Pane", "select-pane -t = ; send-keys -M"]),
        strings(&["bind", "-T", "root", "MouseDown1Status", "run-shell", "-b", STATUS_CLICK]),

    ]
});

/// The terminal window's title (contract §6.1, hs-d300): the config's machine
/// name, which `cpd tui` sets as the global `@cpd_machine`; the host's short
/// name until then, on a server another command started.
const SET_TITLES_STRING: &str = "φ · #{?@cpd_machine,#{@cpd_machine},#{host_short}}";

/// What a click on a StatusBar button runs (contract §6.1): `cpd tui action`
/// with the button's range and the clicking client, `cpd` found on `PATH` as
/// for every pane cpd starts (`session::PROGRAM`). Its stderr is dropped,
/// since a `run-shell -b` that fails or prints puts the active pane into view
/// mode (research `statusbar-no-row.md` 2c).
pub const STATUS_CLICK: &str = "cpd tui action #{mouse_status_range} --client #{client_name} 2>/dev/null";

// The StatusBar's button glyphs (hs-d235, Material Design). Macros, because `concat!` takes literals only.
macro_rules! reset_glyph { () => { "\u{F0A1D}" }; } // md-view_dashboard_outline
macro_rules! legend_glyph { () => { "\u{F0A01}" }; } // md-map_legend
macro_rules! logs_glyph { () => { "\u{F09ED}" }; } // md-text_box_outline
macro_rules! costs_glyph { () => { "\u{EEC1}" }; } // nf-fa-comment_dollar

/// The reset-layout button's glyph, U+F0A1D md-view_dashboard_outline.
pub const RESET_GLYPH: &str = reset_glyph!();
/// The legend button's glyph, U+F0A01 md-map_legend.
pub const LEGEND_GLYPH: &str = legend_glyph!();
/// The logs button's glyph, U+F09ED md-text_box_outline.
pub const LOGS_GLYPH: &str = logs_glyph!();
/// The costs button's glyph, U+EEC1 nf-fa-comment_dollar.
pub const COSTS_GLYPH: &str = costs_glyph!();

/// The base colour of every status button (pro-d195): drawn bright, `spec().clickable(7)` = 15, so the four look alike.
const BUTTON_BASE: u8 = 7;

// The runner pill's width (0 while `@cpd_runner` is unset): a join, two caps and the runner.
macro_rules! runner_width { () => { "#{?@cpd_runner,#{e|+:#{n:@cpd_runner},3},0}" }; }
// The preview pill's width (0 without a preview): a join, two caps and `preview:` (12 cells) plus the value.
macro_rules! preview_width { () => { "#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}" }; }
// Whether the long workers form fits: the long workers pill's width (tmux's `#{w:@cpd_workers}` plus its two caps,
// whose one space is the join), the closing `━`'s cell, the left group's 16 and a fill of at least 1 (18 in all), the
// preview, the version pill and the incident pill.
macro_rules! long_fits {
    () => {
        concat!("#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{e|+:#{w:@cpd_workers},2},18},", runner_width!(), "},", preview_width!(), "},", version_width!(), "},", incident_width!(), "}}")
    };
}
// The version pill's width (0 while `@cpd_version` is unset): a join, two caps and the version.
macro_rules! version_width { () => { "#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}" }; }
// The incident pill's width (0 while `@cpd_incident` is unset): a join, two caps and the 15-cell label
// `Incident Mode: ` plus the value.
macro_rules! incident_width { () => { "#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}" }; }
// The fill's first column, F0: the left group's 16 cells and the preview pill.
macro_rules! fill_start { () => { concat!("#{e|+:#{e|+:16,", runner_width!(), "},", preview_width!(), "}") }; }
// The fill's cell count: `#{client_width}` minus the left group's 16, the preview pill, the version pill, the incident
// pill and the right group's workers pill in the form drawn plus the closing `━`.
macro_rules! fill_count {
    () => {
        concat!(
            "#{e|-:#{e|-:#{e|-:#{e|-:#{e|-:#{e|-:#{client_width},16},", runner_width!(), "},", preview_width!(), "},", version_width!(), "},", incident_width!(), "},",
            "#{e|+:#{?", long_fits!(), ",#{e|+:#{w:@cpd_workers},2},#{e|+:#{w:@cpd_workers_short},2}},1}}"
        )
    };
}
// F1 = F0 + the count: the first column of the right group.
macro_rules! fill_end { () => { concat!("#{e|+:", fill_start!(), ",", fill_count!(), "}") }; }
// `max(a, b)` and `min(a, b)`: the format language has neither.
macro_rules! max { ($a:expr, $b:expr) => { concat!("#{?#{e|>:", $a, ",", $b, "},", $a, ",", $b, "}") }; }
macro_rules! min { ($a:expr, $b:expr) => { concat!("#{?#{e|<:", $a, ",", $b, "},", $a, ",", $b, "}") }; }
// The vertical border's column right of the visited pane.
macro_rules! border_column { () => { "#{e|+:#{pane_right},1}" }; }
// The fill: today's run of `━` in a zoomed window; else, per bottom-row pane in index order, its columns that lie in
// the fill as a run of `━`, then a `┻` at its right border's column when that is in the fill and the pane is not at the right edge.
macro_rules! fill {
    () => {
        concat!(
            "#{?window_zoomed_flag,#{R:\u{2501},", fill_count!(), "},",
            "#{P/i/:#{?pane_at_bottom,",
            "#{R:\u{2501},", max!("0", concat!("#{e|+:#{e|-:", min!("#{pane_right}", concat!("#{e|-:", fill_end!(), ",1}")), ",", max!("#{pane_left}", fill_start!()), "},1}")), "}",
            "#{?#{&&:#{!:#{pane_at_right}},#{&&:#{e|>=:", border_column!(), ",", fill_start!(), "},#{e|<:", border_column!(), ",", fill_end!(), "}}},\u{253B},}",
            ",}}}"
        )
    };
}
// A join: one grey `━`, outside the click ranges.
macro_rules! join { () => { "#[fg=colour8]\u{2501}#[default]" }; }

/// The StatusBar (contract §6.1, hs-d479): the cockpit session's `status-format[0]`,
/// one grey (colour 8) `\u{2501}` line across the full width that starts and ends with a
/// `\u{2501}`. Every item is a pill, its text between the caps with no padding. At the
/// left, the four one-glyph buttons (a cap `\u{e0b6}`, the glyph as a grey block, a cap
/// `\u{e0b4}`, 3 cells), each in its own click range, joined by a single `\u{2501}` outside
/// the ranges (a range also owns the cell after it, so the join does what the space did;
/// the leading `\u{2501}` is outside the first range): the left group is 16 cells.
/// Then the `\u{2501}` fill over the free width: `#{client_width}` minus
/// the left group's 16, the preview pill and the right group in the form drawn (the
/// workers pill's width, `#{w:@cpd_workers}` plus its two caps, plus the closing `\u{2501}`).
/// At the right, each its own pill on
/// bg 8 (text 0), joined by a single `\u{2501}`: the workers counts (User, hs-d198): the long form in
/// `@cpd_workers` when the fill would still be at least 1 cell (`#{client_width}` at
/// least `#{w:@cpd_workers}` plus the two caps and 18), else the short form in
/// `@cpd_workers_short`, whose width is `#{w:@cpd_workers_short}`.
///
/// The fill is split by a `#{P/i/:}` loop over the window's panes in index order (tmux 3.6+): each
/// bottom-row pane adds a run of `\u{2501}` over its columns inside the fill, then a `\u{253B}` (colour 8) at
/// its right border's column (`pane_right + 1`) when the pane is not at the right edge and that column
/// is in the fill (so none under a button, the preview pill or the right group); a zoomed window has
/// the plain fill. The row keeps its width; the contract (§6.1, the junctions) is the long form.
///
/// While `@cpd_preview` is set (`scripts/preview-tui.sh on`, hs-n264), a
/// pill `preview: <value>` follows the buttons, joined by a single `\u{2501}`, and the
/// fill is shorter by its width (`#{n:@cpd_preview}` + 12: the join, two caps and
/// `preview: `). Styles inside a conditional are one attribute per `#[…]`, because a
/// comma there would split the branch.
///
/// While `@cpd_version` is set (`cpd tui` sets it to `cpd --version`'s `version`), a second
/// right-hand pill, the version, follows the workers pill, joined by a single
/// `\u{2501}`; the fill and the long-form test are shorter by its width (`#{n:@cpd_version}` + 3).
/// While `@cpd_host_alert` is set (E6, hs-d618) the version pill is the bright red block (index 9,
/// the clickable pair of the failed colour) inside `#[range=user|host]`, the range a click runs;
/// else it stays the grey (index 8) pill it always was.
///
/// While `@cpd_incident` is set (incident mode is on, hs-d634, hs-d640), the incident pill, the literal
/// `Incident Mode: ` followed by the elapsed duration in colour 9 with reverse video and the `italics`
/// attribute, follows the fill before the workers pill, joined to it by a single `\u{2501}`; the
/// fill and the long-form test are shorter by its width (a join, two caps and the 15-cell label, 18
/// cells); the range `incident` wraps the whole pill, caps included, and a click opens the
/// incident's Lead.
///
/// Colours follow §6.1.1 (hs-d534): the reset, legend, logs and costs buttons take the one bright
/// colour 15 ([`BUTTON_BASE`] 7, pro-d195), the workers and version pills bg 8,
/// the preview pill bg 3, all text 0; lines colour 8.
pub static STATUS_FORMAT: LazyLock<String> = LazyLock::new(|| {
    let m = &spec().meanings;
    let button = |range: &str, glyph: &str, base: u8| {
        pill::range(range, &format!("{}#[default]", pill::pill(&pill::fg(spec().clickable(base)), "", glyph)))
    };
    let status = |index: u8, text: &str| format!("{}#[default]", pill::pill(&pill::fg(index), "", text));
    // The incident pill is red on a red block in the `incident` click range: the contract (§6.1.1) gives it the
    // `Meaning::Failed` bright pair (index 9).
    let incident = pill::range("incident", &format!("{}#[default]", pill::italic_pill(&pill::fg(spec().clickable(m.failed)), "", "Incident Mode: #{@cpd_incident}")));
    format!(
        "{join}{reset}{join}{legend}{join}{logs}{join}{costs}#{{?@cpd_runner,{join}{runner},}}#{{?#{{@cpd_preview}},#[fg=colour8]\u{2501}{preview},}}#[fg=colour8]{fill}#[default]#{{?@cpd_incident,{incident}{join},}}{workers}#{{?#{{@cpd_host_alert}},#[fg=colour8]\u{2501}{alert},{plain}}}{join}",
        join = join!(),
        reset = button("reset", reset_glyph!(), BUTTON_BASE),
        legend = button("legend", legend_glyph!(), BUTTON_BASE),
        logs = button("logs", logs_glyph!(), BUTTON_BASE),
        costs = button("costs", costs_glyph!(), BUTTON_BASE),
        runner = button("runner", "#{@cpd_runner}", BUTTON_BASE),
        preview = status(m.review, "preview: #{@cpd_preview}"),
        fill = fill!(),
        workers = status(m.nothing, concat!("#{?", long_fits!(), ",#{@cpd_workers},#{@cpd_workers_short}}")),
        incident = incident,
        // The red branch: the bright red clickable pair of the failed meaning, in the click range.
        alert = pill::range("host", &format!("{}#[default]", pill::pill(&pill::fg(spec().clickable(m.failed)), "", "#{@cpd_version}"))),
        // The plain branch: the grey pill, only while `@cpd_version` is set.
        plain = format!("#{{?#{{@cpd_version}},#[fg=colour8]\u{2501}{},}}", status(m.nothing, "#{@cpd_version}")),
    )
});

/// The cockpit session's own options (contract §6.1), set with `-t`: `status`
/// is a session option, and `-g` would give `cpd-work` a row too.
/// `status-style default` replaces tmux's green bar with the terminal's colours.
pub static STATUS_OPTIONS: LazyLock<[(&'static str, String); 3]> = LazyLock::new(|| {
    [("status", "on".into()), ("status-style", "default".into()), ("status-format[0]", STATUS_FORMAT.clone())]
});

/// The value `terminal-features` is appended with (`set -as`), the one entry of
/// [`SERVER_OPTIONS`] that is not idempotent: it runs once, when cpd starts
/// the server, and [`Tmux::apply_server_options`] skips it.
const TERMINAL_FEATURES: &str = ",xterm*:extkeys,xterm-ghostty:extkeys,xterm-ghostty:hyperlinks";

/// How many stderr lines a `tmux_failed` error carries (commands.md §1.3).
const STDERR_TAIL_LINES: usize = 60;

/// A tmux pane id, `%<n>`. Holding one proves the shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(String);

/// A tmux window id, `@<n>`. Holding one proves the shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(String);

fn tmux_id(s: &str, sigil: char) -> bool {
    s.strip_prefix(sigil).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl PaneId {
    /// `%<n>`, else `None`.
    pub fn parse(s: &str) -> Option<PaneId> {
        tmux_id(s, '%').then(|| PaneId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl WindowId {
    /// `@<n>`, else `None`.
    pub fn parse(s: &str) -> Option<WindowId> {
        tmux_id(s, '@').then(|| WindowId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One pane as `list-panes -a` reports it (contract §6.3, liveness).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub pane: PaneId,
    pub window: WindowId,
    pub session_name: String,
    /// The window's name: a label for people, used only to find `cpd:main`.
    pub window_name: String,
    /// `#{pane_dead}`: the program has exited and the pane remains (`remain-on-exit`).
    pub dead: bool,
    /// `#{pane_dead_status}`: the exit status of a dead pane's program, when it exited normally.
    pub dead_status: Option<i32>,
    /// `#{pane_pid}`: `None` once the pane's tty has closed on tmux >= 3.8,
    /// which then prints it empty: when the pane is dead, and also while it
    /// is dying (its program not yet reaped, so `dead` is still false).
    pub pid: Option<u32>,
    /// `@cpd_session`.
    pub cpd_session: Option<SessionId>,
    /// `@cpd_view`.
    pub cpd_view: Option<View>,
}

/// The fields of [`Pane`], tab-separated: the liveness format of contract §6.3
/// plus `window_name` and `@cpd_view`, which find the chat slot (§6.2). Tabs,
/// not spaces, because `pane_dead_status` and the options are empty when unset.
const LIST_PANES_FORMAT: &str = "#{pane_id}\t#{window_id}\t#{session_name}\t#{window_name}\t#{pane_dead}\t#{pane_dead_status}\t#{pane_pid}\t#{@cpd_session}\t#{@cpd_view}";

/// What a new window or pane runs: its argv and environment.
///
/// tmux runs a command of several arguments directly and a single argument
/// through a shell (tmux(1)), so `argv` should have at least two elements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spawn {
    /// Working directory (`-c`); none keeps tmux's default.
    pub cwd: Option<PathBuf>,
    /// Added environment (`-e K=V`).
    pub env: BTreeMap<String, String>,
    /// The program and its arguments, after `--`.
    pub argv: Vec<String>,
}

impl Spawn {
    fn push_args(&self, args: &mut Vec<String>) {
        if let Some(cwd) = &self.cwd {
            args.push("-c".into());
            args.push(cwd.display().to_string());
        }
        for (key, value) in &self.env {
            args.push("-e".into());
            args.push(format!("{key}={value}"));
        }
    }

    fn push_argv(&self, args: &mut Vec<String>) {
        if !self.argv.is_empty() {
            args.push("--".into());
            args.extend(self.argv.iter().cloned());
        }
    }
}

/// Where `split-window` puts the new pane relative to the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// Side by side (`-h`).
    Right,
    /// Stacked (`-v`).
    Below,
}

impl Split {
    /// The `split-window` flags that place the pane.
    fn flags(self) -> &'static [&'static str] {
        match self {
            Split::Right => &["-h"],
            Split::Below => &["-v"],
        }
    }
}

/// The tmux server cpd uses: `tmux -L <socket>` (contract §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tmux {
    socket: String,
    /// `TMUX_TMPDIR` for every call, when set; otherwise the process's own environment applies.
    tmpdir: Option<PathBuf>,
    /// The tmux program every call runs, when set (see [`Tmux::on_server_binary`]); otherwise `tmux` from `PATH`.
    program: Option<PathBuf>,
    /// What starts the processes: how a call runs, not which server it is, so it takes no part in equality.
    runner: Handle,
    /// What `kill-server` reads and naps on: the time, not which server it is, so it takes no part in equality.
    clock: Time,
    /// What names a process's executable (`server_exe`): how a pid is read, not which server it is, so it takes no part in equality.
    procs: Procs,
}

/// The [`ProcessPort`] of a [`Tmux`]: always equal to another, printed by name.
#[derive(Clone)]
struct Procs(Arc<dyn ProcessPort>);

impl Default for Procs {
    fn default() -> Procs {
        Procs(Ports::current().spawn)
    }
}

impl fmt::Debug for Procs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Spawn")
    }
}

impl PartialEq for Procs {
    fn eq(&self, _: &Procs) -> bool {
        true
    }
}

impl Eq for Procs {}

/// The [`Clock`] of a [`Tmux`]: always equal to another, printed by name.
#[derive(Clone)]
struct Time(Arc<dyn Clock>);

impl Default for Time {
    fn default() -> Time {
        Time(Arc::new(SystemClock))
    }
}

impl fmt::Debug for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock")
    }
}

impl PartialEq for Time {
    fn eq(&self, _: &Time) -> bool {
        true
    }
}

impl Eq for Time {}

/// The [`Runner`] of a [`Tmux`]: always equal to another, printed by name.
#[derive(Clone)]
struct Handle(Arc<dyn Runner>);

impl Default for Handle {
    fn default() -> Handle {
        Handle(Ports::current().runner)
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Runner")
    }
}

impl PartialEq for Handle {
    fn eq(&self, _: &Handle) -> bool {
        true
    }
}

impl Eq for Handle {}

impl Tmux {
    /// The server on socket `socket` (config `tmux.socket`).
    pub fn new(socket: impl Into<String>) -> Tmux {
        Tmux { socket: socket.into(), tmpdir: None, program: None, runner: Handle::default(), clock: Time::default(), procs: Procs::default() }
    }

    /// The server on socket `socket` whose socket file lives in `tmpdir`: every call sets
    /// `TMUX_TMPDIR` to it, so nothing depends on the process environment.
    pub fn with_tmpdir(socket: impl Into<String>, tmpdir: impl Into<PathBuf>) -> Tmux {
        Tmux { socket: socket.into(), tmpdir: Some(tmpdir.into()), program: None, runner: Handle::default(), clock: Time::default(), procs: Procs::default() }
    }

    /// This server, with every call run by `runner` instead of the real processes.
    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> Tmux {
        self.runner = Handle(runner);
        self
    }

    /// This server, whose `kill-server` waits on `clock` instead of the real time.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Tmux {
        self.clock = Time(clock);
        self
    }

    /// This server, whose `server_exe` asks `spawn` for the executable of a pid instead of the real process table.
    pub fn with_spawn(mut self, spawn: Arc<dyn ProcessPort>) -> Tmux {
        self.procs = Procs(spawn);
        self
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// This server, with every call run by `program` instead of `tmux` from `PATH`.
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Tmux {
        self.program = Some(program.into());
        self
    }

    /// This server, with every call run by the server's own tmux program ([`Tmux::server_exe`]): a
    /// client of another version cannot be trusted to talk to it, least of all while it dies (hs-n285).
    ///
    /// Linux: the program is the file `/proc/<pid>/exe` points to, which outlives the server; a file an upgrade
    /// has replaced is only reachable as `/proc/<pid>/exe`, which lasts as long as the server does.
    /// macOS: the `read_link` of a regular file fails, so the program is the path `server_exe` read.
    pub fn on_server_binary(&self) -> Result<Tmux, CpdError> {
        let exe = self.server_exe()?;
        let program = std::fs::read_link(&exe).ok().filter(|file| file.is_file()).unwrap_or(exe);
        Ok(self.clone().with_program(program))
    }

    /// The full argv of a call: `tmux -u -L <socket> -f /dev/null <args>` (`-u`: a UTF-8 client, hs-d463).
    pub fn argv(&self, args: &[String]) -> Vec<String> {
        let mut argv = vec![PROGRAM.to_owned(), UTF8.to_owned(), "-L".to_owned(), self.socket.clone(), "-f".to_owned(), "/dev/null".to_owned()];
        argv.extend(args.iter().cloned());
        argv
    }

    /// The `Command` for a call: its argv, without `TMUX` and `TMUX_PANE`.
    fn command(&self, args: &[String]) -> Command {
        let argv = self.argv(args);
        let mut cmd = client(self.program.as_deref().unwrap_or_else(|| Path::new(PROGRAM)));
        cmd.args(&argv[2..]);
        if let Some(tmpdir) = &self.tmpdir {
            cmd.env("TMUX_TMPDIR", tmpdir);
        }
        cmd
    }

    fn output(&self, args: &[String]) -> Result<Output, CpdError> {
        let argv = self.argv(args);
        // tmux falls back to `/tmp/tmux-<uid>/` for a `TMUX_TMPDIR` that does not exist: a late start would escape every
        // teardown (hygiene.md 3b), so a missing directory is refused.
        let tmpdir = self.tmpdir.clone().or_else(|| std::env::var_os("TMUX_TMPDIR").filter(|v| !v.is_empty()).map(Into::into));
        if let Some(dir) = tmpdir.filter(|d| !d.is_dir()) {
            return Err(CpdError::new(ErrorCode::TmuxFailed, format!("TMUX_TMPDIR {} is not a directory.", dir.display()))
                .with("argv", argv)
                .with("exit", serde_json::Value::Null));
        }
        let mut cmd = self.command(args);
        let ran = match deadline_of(args) {
            Some(limit) => self.runner.0.output(&mut cmd, "tmux", limit),
            // No deadline; the runner starts it in its own process group, as every other call.
            None => self.runner.0.output_unbounded(&mut cmd, "tmux").map_err(Failed::Io),
        };
        ran.map_err(|e| match e {
            Failed::Io(e) => CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux: {e}."))
                .with("argv", argv.clone())
                .with("exit", serde_json::Value::Null)
                .with("stderr", e.to_string()),
            Failed::Timeout(e) => e,
        })
    }

    fn failed(&self, args: &[String], out: &Output) -> CpdError {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let tail = lines[lines.len().saturating_sub(STDERR_TAIL_LINES)..].join("\n");
        let first = lines.first().copied().unwrap_or("no output");
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux {} failed: {first}", args.first().map_or("", String::as_str)))
            .with("argv", self.argv(args))
            .with("exit", out.status.code())
            .with("stderr", tail)
    }

    /// Runs a call that must succeed; returns its stdout.
    pub fn run(&self, args: &[String]) -> Result<String, CpdError> {
        let out = self.output(args)?;
        if !out.status.success() {
            return Err(self.failed(args, &out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Runs a call whose exit 1 means "no": true on 0, false on 1, else `tmux_failed`.
    fn check(&self, args: &[String]) -> Result<bool, CpdError> {
        let out = self.output(args)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(self.failed(args, &out)),
        }
    }

    /// Whether the server is running (it has at least one session). False
    /// only when tmux says no server runs ([`no_server`]); any other failure
    /// is `tmux_failed` (contract §6.3 liveness, engine-W8).
    pub fn server_running(&self) -> Result<bool, CpdError> {
        let args = strings(&["list-sessions", "-F", "#{session_id}"]);
        let out = self.output(&args)?;
        if out.status.success() {
            return Ok(true);
        }
        if no_server(&String::from_utf8_lossy(&out.stderr)) {
            return Ok(false);
        }
        Err(self.failed(&args, &out))
    }

    /// `has-session -t =<name>`: an exact name match. False when no server runs.
    pub fn has_session(&self, name: &str) -> Result<bool, CpdError> {
        self.check(&strings(&["has-session", "-t", &format!("={name}")]))
    }

    /// Makes sure session `name` exists (contract §6.1, §6.3): when it is
    /// missing, `new-session -d -s <name> -n <window> -x 215 -y 62 -c <dir>
    /// [-- argv]`; when that starts the server, sets [`SERVER_OPTIONS`] first
    /// (see [`new_session_args`]).
    /// Returns whether it created the session. Idempotent; a concurrent creator
    /// winning the race is not an error.
    pub fn ensure_session(&self, name: &str, window: &str, spawn: &Spawn) -> Result<bool, CpdError> {
        Ok(self.create_session(name, window, spawn)?.is_some())
    }

    /// [`Tmux::ensure_session`], returning the ids of the first window and its
    /// pane when it created the session (`-P -F '#{window_id} #{pane_id}'`),
    /// `None` when the session already existed.
    ///
    /// `new-session -e` sets the variables in the new session's environment,
    /// which every later window of the session would inherit (tmux(1)); they
    /// are unset from it at once (`set-environment -u`), so like
    /// [`Tmux::new_window`]'s `-e` they reach only this first pane.
    pub fn create_session(&self, name: &str, window: &str, spawn: &Spawn) -> Result<Option<(WindowId, PaneId)>, CpdError> {
        let fresh = !self.server_running()?;
        if !fresh && self.has_session(name)? {
            return Ok(None);
        }
        let args = new_session_args(name, window, spawn, fresh);
        let out = self.output(&args)?;
        if !out.status.success() {
            if self.has_session(name)? {
                return Ok(None);
            }
            return Err(self.failed(&args, &out));
        }
        let printed = String::from_utf8_lossy(&out.stdout);
        let ids = printed.trim_end_matches('\n');
        let parsed = ids.split_once(' ').and_then(|(w, p)| Some((WindowId::parse(w)?, PaneId::parse(p)?)));
        let parsed = parsed.ok_or_else(|| self.unexpected(&args, ids))?;
        for key in spawn.env.keys() {
            self.run(&strings(&["set-environment", "-t", &format!("={name}"), "-u", key]))?;
        }
        Ok(Some(parsed))
    }

    /// Sets [`SERVER_OPTIONS`] on a running server (contract §6.1: `cpd tui`
    /// applies them again), all but the `terminal-features` append, which the
    /// server's start already made. Rerunning is safe.
    pub fn apply_server_options(&self) -> Result<(), CpdError> {
        for option in SERVER_OPTIONS.iter().filter(|o| o[1] != "-as") {
            self.run(option)?;
        }
        Ok(())
    }

    /// Sets [`STATUS_OPTIONS`] on session `session` (`set -t =<session>:`;
    /// `set` resolves its target as a pane, so a bare `=<session>` is refused):
    /// the StatusBar, on that session only, and the global `@cpd_version` the version pill shows. Rerunning is safe.
    pub fn apply_status_line(&self, session: &str, version: &str) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-g", "@cpd_version", version]))?;
        for (option, value) in STATUS_OPTIONS.iter() {
            self.run(&strings(&["set", "-t", &format!("={session}:"), option, value]))?;
        }
        Ok(())
    }

    /// The server's option `@cpd_preview` (`show -gqv`), `None` when unset or empty:
    /// `scripts/preview-tui.sh on` sets it while the views run a preview build.
    pub fn preview(&self) -> Result<Option<String>, CpdError> {
        let out = self.run(&strings(&["show", "-gqv", "@cpd_preview"]))?;
        let value = out.trim();
        Ok((!value.is_empty()).then(|| value.to_owned()))
    }

    /// The path of the executable process `pid` runs, `None` when it cannot be read (the process is gone).
    /// Linux: `/proc/<pid>/exe`, which stays readable (and the same inode) when an upgrade replaced the file.
    /// macOS: `libproc`'s path of the pid, the file now at that path: a process started from a binary that was
    /// later replaced in place at the same path is not seen as running another one (what `libproc` returns for an
    /// unlinked executable: unverified).
    pub fn pid_exe(pid: u32) -> Option<PathBuf> {
        Tmux::pid_exe_with(&*Ports::current().spawn, pid)
    }

    /// [`Tmux::pid_exe`] asking `spawn` (`Spawn::exe_of`) whether the process exists and what it runs. On Linux the real
    /// `Spawn` is answered with `/proc/<pid>/exe` itself, not the file it points to: that stays readable after an upgrade.
    pub fn pid_exe_with(spawn: &dyn ProcessPort, pid: u32) -> Option<PathBuf> {
        let exe = spawn.exe_of(ProcessId(pid))?;
        #[cfg(not(target_os = "macos"))]
        if spawn.kind() == "ProcessSpawn" {
            return Some(PathBuf::from(format!("/proc/{pid}/exe")));
        }
        Some(exe)
    }

    /// `display -p '#{version}'`: the version of the tmux the server runs
    /// (`3.4`, `next-3.7`), which is the one it was started with.
    pub fn server_version(&self) -> Result<String, CpdError> {
        Ok(self.run(&strings(&["display", "-p", "#{version}"]))?.trim().to_owned())
    }

    /// The server's own tmux program. Linux: `/proc/<server pid>/exe`, which
    /// execs the binary the server runs even when an upgrade has replaced its
    /// file. macOS: the path of the server pid's executable (`libproc`); it has no
    /// equivalent of `/proc/<pid>/exe`, so after an upgrade the path names the new
    /// file (accepted). A client of another version cannot attach to the server (hs-n224).
    pub fn server_exe(&self) -> Result<PathBuf, CpdError> {
        let out = self.run(&strings(&["display", "-p", "#{pid}"]))?;
        let pid: u32 = out.trim().parse().map_err(|e| {
            CpdError::new(ErrorCode::TmuxFailed, format!("The tmux server's pid {:?} is not a number: {e}.", out.trim()))
        })?;
        let exe = Tmux::pid_exe_with(&*self.procs.0, pid).ok_or_else(|| {
            CpdError::new(ErrorCode::TmuxFailed, format!("Cannot find the tmux server's own binary (pid {pid})."))
                .with("pid", pid.to_string())
        })?;
        std::fs::metadata(&exe).map_err(|e| {
            CpdError::new(ErrorCode::TmuxFailed, format!("Cannot find the tmux server's own binary {}: {e}.", exe.display()))
                .with("path", exe.display().to_string())
        })?;
        Ok(exe)
    }

    /// `show -gv <option>`: a global option's value, without the newline.
    pub fn global_option(&self, option: &str) -> Result<String, CpdError> {
        Ok(self.run(&strings(&["show", "-gv", option]))?.trim_end_matches('\n').to_owned())
    }

    /// `set -p -t <pane> @cpd_session <id>`.
    pub fn set_pane_session(&self, pane: &PaneId, session: &SessionId) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_SESSION, session.as_str()])).map(drop)
    }

    /// `set -p -t <pane> @cpd_title <title>`, the title in Title Case (hs-d475, [`title_case`]).
    pub fn set_pane_title(&self, pane: &PaneId, title: &str) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_TITLE, &title_case(title)])).map(drop)
    }

    /// `set -p -t <pane> @cpd_view <view>`.
    pub fn set_pane_view(&self, pane: &PaneId, view: View) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_VIEW, view.as_str()])).map(drop)
    }

    /// `set -p -t <pane> @cpd_history <id>`.
    pub fn set_pane_history(&self, pane: &PaneId, session: &SessionId) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_HISTORY, session.as_str()])).map(drop)
    }

    /// `new-window -d -t =<session>: -n <name> [-c <cwd>] [-e K=V …] -P -F
    /// '#{window_id} #{pane_id}' -- <argv…>` (contract §6.3).
    pub fn new_window(&self, session: &str, name: &str, spawn: &Spawn) -> Result<(WindowId, PaneId), CpdError> {
        let mut args = strings(&["new-window", "-d", "-t", &format!("={session}:"), "-n", name]);
        spawn.push_args(&mut args);
        args.extend(strings(&["-P", "-F", "#{window_id} #{pane_id}"]));
        spawn.push_argv(&mut args);
        let out = self.run(&args)?;
        let ids = out.trim_end_matches('\n');
        let parsed = ids.split_once(' ').and_then(|(w, p)| Some((WindowId::parse(w)?, PaneId::parse(p)?)));
        parsed.ok_or_else(|| self.unexpected(&args, ids))
    }

    /// `split-window -d -t <pane> -h|-v [-l <size>] [-c <cwd>] [-e K=V …] -P -F
    /// '#{pane_id}' -- <argv…>`: a new pane beside `target` (the cockpit's
    /// layout, commands.md §13.1). `size` is tmux's `-l`: cells, or `NN%`.
    pub fn split_window(&self, target: &PaneId, split: Split, size: Option<&str>, spawn: &Spawn) -> Result<PaneId, CpdError> {
        let mut args = strings(&["split-window", "-d", "-t", target.as_str()]);
        args.extend(strings(split.flags()));
        if let Some(size) = size {
            args.extend(strings(&["-l", size]));
        }
        spawn.push_args(&mut args);
        args.extend(strings(&["-P", "-F", "#{pane_id}"]));
        spawn.push_argv(&mut args);
        let out = self.run(&args)?;
        let id = out.trim_end_matches('\n');
        PaneId::parse(id).ok_or_else(|| self.unexpected(&args, id))
    }

    /// `swap-pane -d -s <source> -t <target>`: both processes keep their pids,
    /// and pane options move with the panes (contract §6.2, §6.3).
    pub fn swap_pane(&self, source: &PaneId, target: &PaneId) -> Result<(), CpdError> {
        self.run(&strings(&["swap-pane", "-d", "-s", source.as_str(), "-t", target.as_str()])).map(drop)
    }

    /// `rename-window -t <window> <name>`.
    pub fn rename_window(&self, window: &WindowId, name: &str) -> Result<(), CpdError> {
        self.run(&strings(&["rename-window", "-t", window.as_str(), name])).map(drop)
    }

    /// `respawn-pane -t <pane> [-c <cwd>] [-e K=V …] [-- <argv…>]`: restarts a
    /// dead pane of the cockpit's layout (amendment 9). Without `-k`, so a live
    /// pane is refused (`tmux_failed`), never killed. An empty argv reruns the
    /// pane's original command.
    pub fn respawn_pane(&self, pane: &PaneId, spawn: &Spawn) -> Result<(), CpdError> {
        let mut args = strings(&["respawn-pane", "-t", pane.as_str()]);
        spawn.push_args(&mut args);
        spawn.push_argv(&mut args);
        self.run(&args).map(drop)
    }

    /// `respawn-pane -k -t <pane> [-c <cwd>] [-e K=V …] [-- <argv…>]`: kills
    /// the pane's program, live or dead, and starts it again in place (contract
    /// §6.3: `cpd tui --respawn`). An empty argv reruns the pane's own command.
    pub fn respawn_pane_killing(&self, pane: &PaneId, spawn: &Spawn) -> Result<(), CpdError> {
        let mut args = strings(&["respawn-pane", "-k", "-t", pane.as_str()]);
        spawn.push_args(&mut args);
        spawn.push_argv(&mut args);
        self.run(&args).map(drop)
    }

    /// `kill-pane -t <pane>`; the window goes with its last pane.
    pub fn kill_pane(&self, pane: &PaneId) -> Result<(), CpdError> {
        self.run(&strings(&["kill-pane", "-t", pane.as_str()])).map(drop)
    }

    /// `kill-server`: the server and everything in it end. Returns once the server is gone. A server that
    /// exits before it answers makes the call fail, and one that is dying answers `list-sessions` with
    /// `server exited unexpectedly` or a refused connection: neither is an error here (hs-n285).
    pub fn kill_server(&self) -> Result<(), CpdError> {
        let args = strings(&["kill-server"]);
        let out = self.output(&args)?;
        if !out.status.success() && !self.program_gone() && self.liveness()? == Liveness::Answering {
            return Err(self.failed(&args, &out));
        }
        let clock = &self.clock.0;
        let deadline = clock.now() + Duration::from_secs(10);
        loop {
            if self.program_gone() {
                return Ok(());
            }
            match self.liveness()? {
                Liveness::Gone => return Ok(()),
                Liveness::Answering | Liveness::Dying if clock.now() < deadline => {
                    clock.sleep(Duration::from_millis(50));
                }
                // Its connection closed on us: the server is dead, only its cleanup is left.
                Liveness::Dying => return Ok(()),
                Liveness::Answering => {
                    return Err(CpdError::new(ErrorCode::TmuxFailed, "The tmux server did not exit within 10 s of kill-server.")
                        .with("argv", self.argv(&args)));
                }
            }
        }
    }

    /// The program is the server's own and it is gone: the server went, and took it (Linux: its
    /// `/proc/<pid>/exe`). On macOS the program from [`Tmux::server_exe`] is a plain file, gone when the file is.
    fn program_gone(&self) -> bool {
        #[cfg(target_os = "macos")]
        let own = |p: &Path| p.is_absolute();
        #[cfg(not(target_os = "macos"))]
        let own = |p: &Path| p.starts_with("/proc/");
        self.program.as_deref().is_some_and(|p| own(p) && !p.exists())
    }

    /// `list-sessions`, read as the answer of a server that may be on its way out.
    fn liveness(&self) -> Result<Liveness, CpdError> {
        let args = strings(&["list-sessions", "-F", "#{session_id}"]);
        let out = self.output(&args)?;
        if out.status.success() {
            return Ok(Liveness::Answering);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if no_server(&stderr) || refused(&stderr) {
            Ok(Liveness::Gone)
        } else if dying(&stderr) {
            Ok(Liveness::Dying)
        } else {
            Err(self.failed(&args, &out))
        }
    }

    /// `kill-window -t <window>`.
    pub fn kill_window(&self, window: &WindowId) -> Result<(), CpdError> {
        self.run(&strings(&["kill-window", "-t", window.as_str()])).map(drop)
    }

    /// `#{pane_width}` of `pane`: its width in columns (the chat slot's, for E10's retitle, hs-d543).
    pub fn pane_width(&self, pane: &PaneId) -> Result<u32, CpdError> {
        let args = strings(&["display-message", "-p", "-t", pane.as_str(), "#{pane_width}"]);
        let out = self.run(&args)?;
        out.trim().parse().map_err(|_| self.unexpected(&args, out.trim()))
    }

    /// `list-panes -a`: every pane of the server; none only when tmux says no
    /// server runs ([`Tmux::server_running`]), else `tmux_failed`.
    pub fn list_panes(&self) -> Result<Vec<Pane>, CpdError> {
        let args = strings(&["list-panes", "-a", "-F", LIST_PANES_FORMAT]);
        let mut out = self.output(&args)?;
        if !out.status.success() {
            if !self.server_running()? {
                return Ok(Vec::new());
            }
            // The server was starting (another caller's `new-session`) when the
            // first call connected: list again, once.
            out = self.output(&args)?;
            // A server with no session (`exit-empty off`, §6.1) has no pane: tmux says "no current target".
            if !out.status.success() && String::from_utf8_lossy(&out.stderr).trim() == "no current target" {
                return Ok(Vec::new());
            }
            if !out.status.success() {
                return Err(self.failed(&args, &out));
            }
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut panes = Vec::new();
        for line in stdout.lines() {
            match parse_pane(line).ok_or_else(|| self.unexpected(&args, line))? {
                Line::Pane(pane) => panes.push(pane),
                Line::UnknownView(view) => warn_unknown_view(&view),
                Line::LegacyView => {}
            }
        }
        Ok(panes)
    }

    /// `tmux_failed` for output cpd cannot parse.
    fn unexpected(&self, args: &[String], output: &str) -> CpdError {
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux {} printed unexpected output.", args[0]))
            .with("argv", self.argv(args))
            .with("exit", 0)
            .with("stdout", output.to_owned())
    }
}

/// A tmux version as `#{version}` and `tmux -V` print it (`3.4`, `3.3a`, `next-3.7`), for the order of
/// two versions: major, minor, and the letter of a patch release (none before `a`). `None` when it
/// does not start with `<major>.<minor>` (after an optional `next-`).
fn version_key(version: &str) -> Option<(u32, u32, u8)> {
    let v = version.strip_prefix("next-").unwrap_or(version);
    let (major, rest) = v.split_once('.')?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let minor = rest[..digits].parse().ok()?;
    let letter = rest[digits..].bytes().next().filter(u8::is_ascii_lowercase).map_or(0, |b| b - b'a' + 1);
    Some((major.parse().ok()?, minor, letter))
}

/// Whether `version` (`tmux -V` without its `tmux ` prefix) is one cpd 2 supports: at least 3.6, or a development build
/// (`next-…`), as `cpd doctor` judges it. False when it does not parse.
pub fn supported_version(version: &str) -> bool {
    version.starts_with("next-") || version_key(version).is_some_and(|key| key >= (3, 6, 0))
}

/// Whether tmux `server` is an older version than tmux `installed`. False when either does not parse.
pub fn older_version(server: &str, installed: &str) -> bool {
    matches!((version_key(server), version_key(installed)), (Some(s), Some(i)) if s < i)
}

/// The version of the installed tmux, the one on `PATH`: `tmux -V` without its
/// `tmux ` prefix, in the form of [`Tmux::server_version`].
pub fn installed_version() -> Result<String, CpdError> {
    installed_version_with(&*Ports::current().runner)
}

/// [`installed_version`], the call run by `runner`.
pub fn installed_version_with(runner: &dyn Runner) -> Result<String, CpdError> {
    let argv = vec![PROGRAM.to_owned(), UTF8.to_owned(), "-V".to_owned()];
    let cannot = |e: std::io::Error| {
        CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux: {e}."))
            .with("argv", argv.clone())
            .with("exit", serde_json::Value::Null)
            .with("stderr", e.to_string())
    };
    let out = match runner.output(client(PROGRAM).arg("-V"), "tmux", DEADLINE) {
        Ok(out) => out,
        Err(Failed::Io(e)) => return Err(cannot(e)),
        Err(Failed::Timeout(e)) => return Err(e),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    match (out.status.success(), text.trim().strip_prefix("tmux ")) {
        (true, Some(version)) => Ok(version.to_owned()),
        _ => Err(CpdError::new(ErrorCode::TmuxFailed, "tmux -V did not print `tmux <version>`.")
            .with("argv", argv.clone())
            .with("exit", out.status.code())
            .with("stderr", String::from_utf8_lossy(&out.stderr).into_owned())),
    }
}

/// Whether a failed call's stderr is tmux saying that no server runs: its
/// socket file is missing (`error connecting to <socket> (No such file or
/// directory)`) or nothing listens on it (`no server running on <socket>`),
/// as tmux 3.4 prints them.
fn no_server(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("no server running on ")
            || (line.starts_with("error connecting to ") && line.ends_with("(No such file or directory)"))
    })
}

/// What a `list-sessions` after `kill-server` says about the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Liveness {
    /// It answered: the server runs.
    Answering,
    /// Its connection closed without an answer (`server exited unexpectedly`, `lost server`): it is exiting.
    Dying,
    /// No server runs: no socket file, nothing listens on it, or the connection was refused.
    Gone,
}

/// Whether a failed call's stderr says the connection to the server was refused (its socket file is
/// left behind, nothing listens on it).
fn refused(stderr: &str) -> bool {
    stderr.lines().any(|line| line.starts_with("error connecting to ") && line.ends_with("(Connection refused)"))
}

/// Whether a failed call's stderr says the server closed the connection without answering, as a
/// server does while it exits.
fn dying(stderr: &str) -> bool {
    stderr.lines().any(|line| line.starts_with("server exited unexpectedly") || line.starts_with("lost server"))
}

/// The arguments of [`Tmux::create_session`]'s call: `new-session -d -s <name>
/// -n <window> -x 215 -y 62 [-c <cwd>] [-e K=V …] -P -F '#{window_id}
/// #{pane_id}' [-- argv…]`. On a `fresh` server it is preceded, in the same
/// invocation (`;` separates tmux commands), by [`SERVER_OPTIONS`]: tmux runs
/// them before it spawns the first pane, so a program that exits at once
/// leaves a dead pane (`remain-on-exit`) instead of taking the session and
/// the server with it.
fn new_session_args(name: &str, window: &str, spawn: &Spawn, fresh: bool) -> Vec<String> {
    let mut args = Vec::new();
    if fresh {
        for option in SERVER_OPTIONS.iter() {
            args.extend(option.iter().cloned());
            args.push(";".into());
        }
    }
    args.extend(strings(&["new-session", "-d", "-s", name, "-n", window]));
    args.extend(strings(&["-x", &SESSION_WIDTH.to_string(), "-y", &SESSION_HEIGHT.to_string()]));
    spawn.push_args(&mut args);
    args.extend(strings(&["-P", "-F", "#{window_id} #{pane_id}"]));
    spawn.push_argv(&mut args);
    args
}

/// A parsed `list-panes` line.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    Pane(Pane),
    /// A well-formed pane whose `@cpd_view` this binary does not know (§6.3): a
    /// newer cpd made it, so it is skipped, not an error.
    UnknownView(String),
    /// A well-formed pane with `@cpd_view` in [`LEGACY_VIEWS`]: an older
    /// cpd's StatusBar or Worker pane, which `cpd tui` removes. Skipped, without a warning.
    LegacyView,
}

/// Logs an unknown `@cpd_view` value to stderr, once per value and process.
fn warn_unknown_view(view: &str) {
    static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    if SEEN.lock().unwrap_or_else(|e| e.into_inner()).insert(view.to_owned()) {
        eprintln!("cpd: skipping tmux panes with the unknown @cpd_view {view:?}: a newer cpd made them");
    }
}

fn parse_pane(line: &str) -> Option<Line> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [pane, window, session_name, window_name, dead, dead_status, pid, cpd_session, cpd_view] = fields[..] else {
        return None;
    };
    let pane = Pane {
        pane: PaneId::parse(pane)?,
        window: WindowId::parse(window)?,
        session_name: session_name.to_owned(),
        window_name: window_name.to_owned(),
        dead: match dead {
            "0" => false,
            "1" => true,
            _ => return None,
        },
        dead_status: if dead_status.is_empty() { None } else { Some(dead_status.parse().ok()?) },
        pid: if pid.is_empty() { None } else { Some(pid.parse().ok()?) },
        cpd_session: if cpd_session.is_empty() { None } else { Some(SessionId::parse(cpd_session).ok()?) },
        cpd_view: None,
    };
    if cpd_view.is_empty() {
        return Some(Line::Pane(pane));
    }
    match View::parse(cpd_view) {
        Some(view) => Some(Line::Pane(Pane { cpd_view: Some(view), ..pane })),
        None if LEGACY_VIEWS.contains(&cpd_view) => Some(Line::LegacyView),
        None => Some(Line::UnknownView(cpd_view.to_owned())),
    }
}

/// The text in Title Case (contract §6.1, hs-d475): the first letter of each space-separated word upper-cased, the
/// rest of each word unchanged, so ids, acronyms and `E10` stay.
pub fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word_start = true;
    for c in text.chars() {
        if word_start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        word_start = c == ' ';
    }
    out
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

pub mod pill;

#[cfg(test)]
mod pill_lines_tests;

#[cfg(test)]
mod title_tests;

#[cfg(test)]
mod pill_tests;

#[cfg(test)]
mod incident_tests;

#[cfg(test)]
mod no_inbox;

#[cfg(test)]
mod tests {
    use super::*;

    /// Agent identity must not leak into a tmux server started by cpd.
    #[test]
    fn client_strips_cpd_session_and_cpd_attempt() {
        let cmd = client("tmux");
        for k in ["CPD_SESSION", "CPD_ATTEMPT"] {
            assert!(cmd.get_envs().any(|(n, v)| n == k && v.is_none()), "{k} not removed");
        }
    }

    /// The version pill (§6.1): last on the right, after the workers pill, one `━` join, then the closing `━`, only while `@cpd_version` is set.
    #[test]
    fn the_version_pill_is_last_on_the_right_and_only_while_the_option_is_set() {
        let pill = "#{?#{@cpd_version},#[fg=colour8]\u{2501}#[fg=colour8]\u{e0b6}";
        let at = STATUS_FORMAT.find(pill).unwrap_or_else(|| panic!("no version pill in {}", *STATUS_FORMAT));
        let workers = STATUS_FORMAT.rfind("#{@cpd_workers_short}}").expect("the workers pill");
        assert!(at > workers, "the version pill follows the workers pill: {}", *STATUS_FORMAT);
        let tail = &STATUS_FORMAT[at..];
        assert!(tail.contains("#{@cpd_version}"), "{tail}");
        assert!(tail.contains("fg=colour8") && tail.contains("#[reverse]") && tail.contains("\u{e0b4}"), "{tail}");
        assert!(tail.ends_with("#[fg=colour8]\u{2501}#[default]"), "the closing ━ follows the pill: {tail}");
        assert_eq!(
            STATUS_FORMAT.matches("#[reverse]#{@cpd_version}#[noreverse]").count(),
            2,
            "each branch draws the text once: {}",
            *STATUS_FORMAT
        );
    }

    /// The fill and `long_fits!()` subtract `#{n:@cpd_version}` + 3 while `@cpd_version` is set, 0 otherwise.
    #[test]
    fn the_fill_and_long_fits_subtract_the_version_pills_width() {
        let width = version_width!();
        assert_eq!(width, "#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}");
        let fill = fill_count!();
        assert!(fill.contains(width), "the fill count: {fill}");
        assert!(long_fits!().contains(width), "long_fits: {}", long_fits!());
        assert!(STATUS_FORMAT.contains(fill), "the format uses the fill count: {}", *STATUS_FORMAT);
        assert!(STATUS_FORMAT.contains(long_fits!()), "the format uses long_fits: {}", *STATUS_FORMAT);
    }

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
    /// The left group: the leading `━`, four buttons of 4 cells and the joins between them.
    const LEFT_WIDTH: usize = 20;
    /// The long workers form in the pane tests: `workers 9·4·6/6` (15 cells) and two caps, `#{w:@cpd_workers}`.
    const LONG_WIDTH: usize = 15 + 2;

    /// `long_fits!()` as text: the client is at least the workers pill's width plus 22 (the left group's 20, the closing `━` and a
    /// fill of at least 1), the preview pill, the version pill and the incident pill.
    fn long_fits() -> String {
        format!("#{{e|>=:#{{client_width}},#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{e|+:#{{w:@cpd_workers}},2}},22}},{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}},{VERSION_WIDTH}}},{INCIDENT_WIDTH}}}}}")
    }

    /// A button of the left group: its click range, its glyph and one space between the two caps (4 cells), a block of the bright pair of `base`.
    fn button(range: &str, glyph: &str, base: u8) -> String {
        format!("#[range=user|{range}]#[fg=colour{}]\u{e0b6}#[reverse]{glyph} #[noreverse]\u{e0b4}#[default]#[norange]", spec().clickable(base))
    }

    /// A pill of colour `colour`: its text between the caps, with no padding, in reverse video.
    fn pill_of(colour: u8, text: &str) -> String {
        format!("#[fg=colour{colour}]\u{e0b6}#[reverse]{text}#[noreverse]\u{e0b4}#[default]")
    }

    /// The workers pill: the long form while the client is wide enough, else the short form.
    fn workers_pill() -> String {
        pill_of(8, &format!("#{{?{},#{{@cpd_workers}},#{{@cpd_workers_short}}}}", long_fits()))
    }

    /// The fill's cell count: the client's width less the left group, the preview, version and incident pills and the right
    /// group's workers pill in the form drawn plus the closing `━`.
    fn fill_count() -> String {
        format!(
            "#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{e|-:#{{client_width}},20}},{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}},{VERSION_WIDTH}}},{INCIDENT_WIDTH}}},#{{e|+:#{{?{},#{{e|+:#{{w:@cpd_workers}},2}},#{{e|+:#{{w:@cpd_workers_short}},2}}}},1}}}}",
            long_fits()
        )
    }

    /// The preview pill's conditional: the join and `preview: <value>`, empty while `@cpd_preview` is unset.
    fn preview_branch() -> String {
        format!("#{{?#{{@cpd_preview}},#[fg=colour8]\u{2501}{},}}", pill_of(3, "preview: #{@cpd_preview}"))
    }

    /// What follows the logs button in the format.
    fn after_the_buttons() -> String {
        let logs = button("costs", COSTS_GLYPH, BUTTON_BASE);
        let f: &str = &STATUS_FORMAT;
        let after = &f[f.find(&logs).expect("the costs button") + logs.len()..];
        // the runner pill (pro-d199) sits between the costs button and the preview marker
        after[after.find("#{?#{@cpd_preview}").expect("the preview pill")..].to_owned()
    }

    // The right group is 18 cells wide in the long form (a text of 15 cells, 2 caps, the closing `━`):
    // with the left group's 20 and a fill of at least 1 it needs a client of 39 columns; one less gets the short form.
    #[test]
    fn the_status_line_is_one_full_line_of_joined_pills_in_the_long_workers_form_from_39_columns() {
        let f: &str = &STATUS_FORMAT;
        assert_eq!(long_fits!(), long_fits(), "`client_width >= w:@cpd_workers + 2 + 22`, plus the pills that are set");
        assert_eq!(LEFT_WIDTH + 1 + 1, 22, "the left group's 20, the closing `━` and a fill of 1");
        assert_eq!(LONG_WIDTH + 22, 39, "the long form needs a client of 39 columns");
        assert!(f.contains(&workers_pill()), "the workers pill reads `@cpd_workers` when the long form fits: {f}");
        // the line is full: the fill is the width less the left group and the right group in the form drawn
        assert_eq!(fill_count!(), fill_count());
        assert!(f.contains(&format!("#{{?window_zoomed_flag,#{{R:\u{2501},{}}},", fill_count())), "{f}");
    }

    #[test]
    fn the_status_line_is_one_full_line_of_joined_pills_in_the_short_workers_form_below_39_columns() {
        let f: &str = &STATUS_FORMAT;
        // the short form is the else branch of the long form's test, and the fill subtracts its width then
        assert!(f.contains(&workers_pill()) && workers_pill().contains(",#{@cpd_workers},#{@cpd_workers_short}}"), "{f}");
        assert!(fill_count().contains(",#{e|+:#{w:@cpd_workers},2},#{e|+:#{w:@cpd_workers_short},2}}"), "{}", fill_count());
        // 39 fits and 38 does not: the test is `>=`
        assert!(long_fits().starts_with("#{e|>=:#{client_width},"), "{}", long_fits());
        // the format writes no `workers` word of its own: it is in `@cpd_workers`, which the engine sets
        assert!(!f.replace("@cpd_workers", "").contains("workers"), "{f}");
    }

    #[test]
    fn the_status_line_starts_and_ends_with_one_grey_line_cell_and_joins_its_pills_with_one() {
        let f: &str = &STATUS_FORMAT;
        let buttons = format!("{JOIN}{}{JOIN}{}{JOIN}{}{JOIN}{}", button("reset", RESET_GLYPH, BUTTON_BASE), button("legend", LEGEND_GLYPH, BUTTON_BASE), button("logs", LOGS_GLYPH, BUTTON_BASE), button("costs", COSTS_GLYPH, BUTTON_BASE));
        // the leading `━`, then the four buttons joined by a single `━` each: 4 joins and 4 buttons of 4 cells, 20 cells
        assert!(f.starts_with(&buttons), "{f}");
        assert_eq!(buttons.matches('\u{2501}').count() + 4 * 4, LEFT_WIDTH);
        assert!(f.starts_with(&format!("{JOIN}#[range=user|reset]#[fg=colour15]\u{e0b6}")), "the leading line cell, grey, then a button's cap: {f}");
        // the right group ends with the version conditional, then the closing `━`, grey
        assert!(f.ends_with(&format!("#{{?#{{@cpd_version}},#[fg=colour8]\u{2501}{},}}}}{JOIN}", pill_of(8, "#{@cpd_version}"))), "{f}");
        // the right pills are joined by one `━` (never a space), and the left group by the buttons' own
        assert!(!f.contains("inbox"), "{f}");
        assert!(!f.contains("\u{e0b4} \u{e0b6}") && !f.contains("\u{e0b4}\u{e0b6}"), "no space and no touching caps between two pills: {f}");
    }

    #[test]
    fn the_status_line_fill_is_grey_and_spans_the_free_width_of_any_client() {
        let f: &str = &STATUS_FORMAT;
        // the fill follows the logs button (and the preview pill, while set) in grey, and the right group follows it
        let after = after_the_buttons();
        assert!(after.starts_with(&preview_branch()), "{after}");
        assert!(after[preview_branch().len()..].starts_with("#[fg=colour8]#{?window_zoomed_flag,#{R:\u{2501},"), "{after}");
        assert!(after.contains("#[default]#{?@cpd_incident,"), "{after}");
        // a zoomed window's fill is one run of the whole free width, in either form (40 and 100 columns: the short form and the long one)
        assert!(f.contains(&format!("#{{?window_zoomed_flag,#{{R:\u{2501},{}}},", fill_count())), "{f}");
        // otherwise it starts after the left group's 20 cells and the preview pill
        assert!(f.contains(&format!("#{{e|+:#{{e|+:20,{RUNNER_WIDTH}}},{PREVIEW_WIDTH}}}")), "{f}");
    }

    const PREVIEW: &str = "cpd.tui-visuals.box-order 6c43886";

    // The preview pill is `preview: <value>` between caps, no padding, joined to the buttons by a `━`: 45 cells with
    // 33 of value. With the left group's 20, the long form's 22 and a fill of 1 it needs 84 columns.
    #[test]
    fn the_status_line_shows_the_unpadded_preview_pill_after_the_buttons_while_the_option_is_set() {
        let f: &str = &STATUS_FORMAT;
        // `#{n:@cpd_preview}` + 12: the join, two caps and `preview: `
        assert_eq!(1 + 2 + "preview: ".chars().count(), 12);
        assert_eq!(PREVIEW.chars().count(), 33);
        assert_eq!(LONG_WIDTH + 22 + PREVIEW.chars().count() + 12, 84, "the long form fits from 84 columns with this preview");
        // right after the buttons: a join, then the pill in reverse video on colour 3, unpadded and with no background
        assert!(after_the_buttons().starts_with(&preview_branch()), "{}", after_the_buttons());
        assert!(preview_branch().contains("\u{e0b6}#[reverse]preview: #{@cpd_preview}#[noreverse]\u{e0b4}"), "{}", preview_branch());
        assert!(!f.contains("bg="), "{f}");
        // the fill and the long form's test are shorter by the pill's width while the option is set
        assert!(fill_count().contains(PREVIEW_WIDTH) && long_fits().contains(PREVIEW_WIDTH), "{f}");
        assert!(f.contains(&fill_count()) && f.contains(&long_fits()), "the format uses them: {f}");
    }

    #[test]
    fn the_status_line_shows_no_preview_once_the_option_is_unset() {
        let f: &str = &STATUS_FORMAT;
        // the pill is a conditional on the option with an empty else branch, and the width one with 0
        assert!(f.contains(&preview_branch()) && preview_branch().ends_with(",}"), "{f}");
        assert!(PREVIEW_WIDTH.ends_with(",0}"), "{PREVIEW_WIDTH}");
        // and the option is used nowhere else: with those two forms taken out, no `@cpd_preview` is left
        let rest = f.replace(PREVIEW_WIDTH, "").replace(&preview_branch(), "");
        assert!(!rest.contains("@cpd_preview"), "{rest}");
    }

    #[test]
    fn a_split_is_placed_by_its_flags() {
        assert_eq!(Split::Right.flags(), ["-h"]);
        assert_eq!(Split::Below.flags(), ["-v"]);
    }

    #[test]
    fn versions_are_ordered_by_major_minor_and_patch_letter() {
        for (server, installed) in [("3.4", "3.8"), ("3.3a", "3.4"), ("3.3", "3.3a"), ("2.9", "3.0"), ("next-3.5", "3.6"), ("3.6", "3.10")] {
            assert!(older_version(server, installed), "{server} < {installed}");
            assert!(!older_version(installed, server), "{installed} !< {server}");
        }
        for same in ["3.4", "3.3a", "next-3.7"] {
            assert!(!older_version(same, same));
        }
        assert!(!older_version("master", "3.8") && !older_version("3.4", "unknown"));
    }

    #[test]
    fn title_case_upper_cases_the_first_letter_of_each_word_and_leaves_the_rest() {
        for (text, want) in [
            ("dynamic session titles", "Dynamic Session Titles"),
            ("E10 retitle fix", "E10 Retitle Fix"),
            ("cpd·tui visuals", "Cpd·tui Visuals"),
            ("Retry payouts", "Retry Payouts"),
            ("fix PAY-1 on hs-d475", "Fix PAY-1 On Hs-d475"),
            ("", ""),
        ] {
            assert_eq!(title_case(text), want, "{text:?}");
        }
        // Twice is once, and the spaces stay as they are.
        assert_eq!(title_case(&title_case("a  b")), "A  B");
    }

    #[test]
    fn ids_have_their_sigil() {
        assert_eq!(PaneId::parse("%12").unwrap().as_str(), "%12");
        assert_eq!(WindowId::parse("@3").unwrap().to_string(), "@3");
        for bad in ["", "%", "12", "@1", "%1a", "% 1"] {
            assert!(PaneId::parse(bad).is_none(), "{bad:?}");
        }
        assert!(WindowId::parse("%1").is_none());
    }

    fn pane_of(line: &str) -> Pane {
        match parse_pane(line) {
            Some(Line::Pane(pane)) => pane,
            other => panic!("{line:?}: {other:?}"),
        }
    }

    #[test]
    fn parses_a_list_panes_line() {
        let live = pane_of("%4\t@2\tcpd\tmain\t0\t\t4242\t596d007f-aaaa\t");
        assert_eq!(live.pane.as_str(), "%4");
        assert_eq!(live.window.as_str(), "@2");
        assert_eq!(live.session_name, "cpd");
        assert_eq!(live.window_name, "main");
        assert!(!live.dead);
        assert_eq!(live.dead_status, None);
        assert_eq!(live.pid, Some(4242));
        assert_eq!(live.cpd_session.unwrap().as_str(), "596d007f-aaaa");
        assert_eq!(live.cpd_view, None);

        let dead = pane_of("%5\t@2\tcpd\tmain\t1\t3\t4243\t\tchat");
        assert!(dead.dead);
        assert_eq!(dead.dead_status, Some(3));
        assert_eq!(dead.cpd_view, Some(View::Chat));
        assert_eq!(dead.pid, Some(4243));
    }

    #[test]
    fn a_pane_whose_tty_closed_has_no_pid() {
        // tmux >= 3.8 prints `#{pane_pid}` empty once the pane's tty has
        // closed: dead, or dying (not yet reaped, so `#{pane_dead}` is 0).
        let dead = pane_of("%5\t@2\tcpd-work\twatch\t1\t7\t\t\t");
        assert_eq!((dead.dead, dead.dead_status, dead.pid), (true, Some(7), None));
        let dying = pane_of("%4\t@2\tcpd\tmain\t0\t\t\t\t");
        assert_eq!((dying.dead, dying.pid), (false, None));
        assert!(parse_pane("%4\t@2\tcpd\tmain\t1\t\tx\t\t").is_none());
    }

    #[test]
    fn refuses_a_malformed_line() {
        assert!(parse_pane("%4 @2 cpd main 0  4242  ").is_none());
        assert!(parse_pane("%4\t@2\tcpd\tmain\t2\t\t4242\t\t").is_none());
    }

    #[test]
    fn a_pane_with_an_unknown_view_is_a_line_to_skip_not_a_malformed_one() {
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t0\t\t4242\t\tnope"), Some(Line::UnknownView("nope".into())));
        // Any other defect still fails.
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t2\t\t4242\t\tnope"), None);
    }

    #[test]
    fn an_old_statusbar_pane_is_a_line_to_skip_quietly() {
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t0\t\t4242\t\tstatusbar"), Some(Line::LegacyView));
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t2\t\t4242\t\tstatusbar"), None);
    }

    #[test]
    fn a_status_click_runs_cpd_quietly() {
        assert!(STATUS_CLICK.starts_with(&format!("{} tui action ", crate::session::PROGRAM)));
        assert!(STATUS_CLICK.ends_with(" 2>/dev/null"));
    }

    #[test]
    fn the_engines_hooks_name_its_window_and_start_it_again_quietly() {
        for hook in [PANE_DIED_HOOK, WINDOW_UNLINKED_HOOK] {
            assert!(hook.contains(&format!("_name}},{}}}", super::super::layout::WATCH_WINDOW)), "{hook}");
            assert!(hook.contains("cpd watch --ensure --wait >/dev/null 2>&1"), "{hook}");
        }
        assert!(WINDOW_UNLINKED_HOOK.contains("#{hook_window_name}"));
    }

    #[test]
    fn a_fresh_server_gets_its_options_before_the_first_pane_spawns() {
        let spawn = Spawn { cwd: None, env: BTreeMap::new(), argv: strings(&["cpd", "watch"]) };
        let new_session = strings(&[
            "new-session", "-d", "-s", "cpd-work", "-n", "watch", "-x", "215", "-y", "62", "-P", "-F", "#{window_id} #{pane_id}", "--",
            "cpd", "watch",
        ]);
        assert_eq!(new_session_args("cpd-work", "watch", &spawn, false), new_session);

        let fresh = new_session_args("cpd-work", "watch", &spawn, true);
        let commands: Vec<&[String]> = fresh.split(|a| a == ";").collect();
        let (last, options) = commands.split_last().unwrap();
        assert_eq!(*last, new_session.as_slice());
        let options: Vec<Vec<String>> = options.iter().map(|c| c.to_vec()).collect();
        let expected: Vec<Vec<String>> = SERVER_OPTIONS.clone();
        assert_eq!(options, expected);
    }

    #[test]
    fn every_call_has_the_deadline_but_display_popup() {
        assert_eq!(deadline_of(&strings(&["list-panes", "-a"])), Some(Duration::from_secs(10)));
        assert_eq!(deadline_of(&strings(&["kill-window", "-t", "@1"])), Some(Duration::from_secs(10)));
        assert_eq!(deadline_of(&strings(&["display-popup", "-E", "-T", "t", "cpd tui view legend"])), None);
    }

    #[test]
    fn every_call_names_the_socket_and_no_config_and_drops_tmux_vars() {
        let tmux = Tmux::new("cpdt");
        assert_eq!(tmux.argv(&strings(&["kill-server"])), ["tmux", "-u", "-L", "cpdt", "-f", "/dev/null", "kill-server"]);
        let cmd = tmux.command(&strings(&["kill-server"]));
        assert_eq!(cmd.get_program(), "tmux");
        assert_eq!(cmd.get_args().next().unwrap(), "-u");
        let removed: Vec<_> = cmd.get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k.to_owned()).collect();
        assert_eq!(removed, ["CPD_ATTEMPT", "CPD_SESSION", "TMUX", "TMUX_PANE"]);
    }

    #[test]
    fn every_call_runs_the_program_it_was_given() {
        let tmux = Tmux::new("s").with_program("/opt/tmux-3.4");
        assert_eq!(tmux.command(&strings(&["kill-server"])).get_program(), "/opt/tmux-3.4");
        assert_eq!(tmux.argv(&strings(&["kill-server"]))[0], "tmux");
    }

    #[test]
    fn what_a_server_says_while_it_exits() {
        assert!(dying("server exited unexpectedly\n") && dying("lost server\n") && !dying("no server running on /tmp/x"));
        assert!(refused("error connecting to /tmp/x (Connection refused)") && !refused("error connecting to /tmp/x (No such file or directory)"));
        assert!(no_server("error connecting to /tmp/x (No such file or directory)"));
    }
}
