//! The Dango palette on tmux's side (contract §6.1.1, hs-d534): pane title pills, the status line's pills, tmux's menus and copy mode.
//!
//! The formats are drawn by tmux itself: each test sets the options a format reads on a private server (its socket in a
//! temp dir, killed when the test ends), has tmux expand the format (`display-message -p`), and reads back, cell by cell,
//! the colours the `#[…]` styles give. So they pin the colours the User sees, not how the format spells them.

use super::*;

/// One cell of an expanded format: its character and the colours in force (`None` is the terminal's own).
#[derive(Debug, Clone, Copy)]
pub(super) struct Cell {
    pub(super) ch: char,
    pub(super) fg: Option<u8>,
    pub(super) bg: Option<u8>,
    pub(super) reverse: bool,
}

/// A colour as tmux writes it, as its number 0-15 (`None`: `default`). Panics on a name outside the palette.
fn colour(name: &str) -> Option<u8> {
    const NAMES: [&str; 8] = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    if name == "default" {
        return None;
    }
    if let Some(n) = name.strip_prefix("colour").or_else(|| name.strip_prefix("color")).and_then(|n| n.parse().ok()).or_else(|| name.parse().ok()) {
        return Some(n);
    }
    if let Some(at) = NAMES.iter().position(|n| *n == name) {
        return Some(at as u8);
    }
    if let Some(at) = name.strip_prefix("bright").and_then(|n| NAMES.iter().position(|b| *b == n)) {
        return Some(at as u8 + 8);
    }
    panic!("a colour outside the palette: {name}")
}

/// What a style option such as `fg=blue,reverse` sets: (fg, bg, reverse).
fn style(value: &str) -> (Option<u8>, Option<u8>, bool) {
    let (mut fg, mut bg, mut reverse) = (None, None, false);
    for token in value.split([',', ' ']).filter(|t| !t.is_empty()) {
        match token.split_once('=') {
            Some(("fg", c)) => fg = colour(c),
            Some(("bg", c)) => bg = colour(c),
            _ if token == "reverse" => reverse = true,
            _ => {}
        }
    }
    (fg, bg, reverse)
}

/// The cells of `text`, which tmux expanded but left the `#[…]` styles of.
pub(super) fn cells(text: &str) -> Vec<Cell> {
    let (mut fg, mut bg, mut reverse) = (None, None, false);
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if let Some(inner) = rest.strip_prefix("#[") {
            let end = inner.find(']').unwrap_or_else(|| panic!("an unclosed style in {text}"));
            for token in inner[..end].split(',') {
                match token.split_once('=') {
                    Some(("fg", c)) => fg = colour(c),
                    Some(("bg", c)) => bg = colour(c),
                    _ if token == "default" => (fg, bg, reverse) = (None, None, false),
                    _ if token == "reverse" => reverse = true,
                    _ if token == "noreverse" => reverse = false,
                    _ => {}
                }
            }
            rest = &inner[end + 1..];
            continue;
        }
        out.push(Cell { ch, fg, bg, reverse });
        rest = &rest[ch.len_utf8()..];
    }
    out
}

pub(super) fn show(cells: &[Cell]) -> String {
    cells.iter().map(|c| format!("{}[{:?},{:?}{}]", c.ch, c.fg, c.bg, if c.reverse { ",reverse" } else { "" })).collect::<Vec<_>>().join(" ")
}

/// Where the pill `<left cap>text<right cap>` starts in `cells` (at its left cap); checks the §6.1.1 rule (3): the caps are
/// `colour` on the terminal's background, not reversed; the text is reversed, in `colour` as foreground with no background.
#[track_caller]
fn pill(cells: &[Cell], text: &str, colour: u8) -> usize {
    let want: Vec<char> = std::iter::once('\u{e0b6}').chain(text.chars()).chain(std::iter::once('\u{e0b4}')).collect();
    let at = (0..cells.len())
        .find(|&i| cells[i..].iter().map(|c| c.ch).take(want.len()).eq(want.iter().copied()))
        .unwrap_or_else(|| panic!("no pill {text:?} in {}", show(cells)));
    let pill = &cells[at..at + want.len()];
    let (left, body, right) = (&pill[0], &pill[1..pill.len() - 1], &pill[pill.len() - 1]);
    for cap in [left, right] {
        assert!(cap.fg == Some(colour) && cap.bg.is_none() && !cap.reverse, "the caps of {text:?} are colour {colour} on the terminal's own background: {}", show(pill));
    }
    for cell in body {
        assert!(cell.fg == Some(colour) && cell.bg.is_none() && cell.reverse, "{text:?} is reversed, in colour {colour} with no background: {}", show(pill));
    }
    at
}

/// A private server with one session, whose socket is in a temp dir and which dies with the value.
pub(super) struct Server {
    tmux: Tmux,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.tmux.run(&strings(&["kill-server"]));
    }
}

impl Server {
    /// A server with `panes` panes in one window, `%0` focused.
    pub(super) fn new(panes: usize) -> Server {
        // tmux's socket is `<dir>/tmux-<uid>/cpd-palette-test`, which must fit `sun_path` (104 bytes on macOS): a
        // long default TMPDIR (resolved: /var is /private/var) keeps the directory in /tmp.
        let socket = "cpd-palette-test";
        let default = tempfile::tempdir().unwrap();
        let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(default.path()).unwrap());
        let path_len = std::fs::canonicalize(default.path()).unwrap().join(format!("tmux-{uid}")).join(socket).as_os_str().len();
        let dir = if path_len > 100 {
            drop(default);
            tempfile::Builder::new().tempdir_in("/tmp").unwrap()
        } else {
            default
        };
        let tmux = Tmux::with_tmpdir(socket, dir.path());
        tmux.run(&strings(&["new-session", "-d", "-s", "t", "-x", "200", "-y", "20", "sleep 600"])).unwrap();
        let server = Server { tmux, _dir: dir };
        for _ in 1..panes {
            server.run(&["split-window", "-d", "-t", "%0", "sleep 600"]);
        }
        server
    }

    pub(super) fn run(&self, args: &[&str]) -> String {
        self.tmux.run(&strings(args)).unwrap_or_else(|e| panic!("tmux {args:?}: {e:?}"))
    }

    /// `format` as tmux expands it for `pane`.
    fn render(&self, pane: &str, format: &str) -> Vec<Cell> {
        let out = self.run(&["display-message", "-p", "-t", pane, "-F", format]);
        cells(out.strip_suffix('\n').unwrap_or(&out))
    }

    /// The status line as drawn for a client `width` cells wide (a server with no client has no `client_width`).
    pub(super) fn status(&self, width: usize) -> Vec<Cell> {
        self.render("%0", &STATUS_FORMAT.replace("#{client_width}", &width.to_string()))
    }
}

/// `format` with each `#{?pane_active,A,B}` replaced by its branch for a pane that is (or is not) focused. The branches
/// are single `#[…]` styles, so they hold no comma or brace.
fn resolve_pane_active(format: &str, focused: bool) -> String {
    const OPEN: &str = "#{?pane_active,";
    let mut out = String::new();
    let mut rest = format;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let branches = &rest[at + OPEN.len()..];
        let end = branches.find('}').expect("a closed pane_active conditional");
        let (then, otherwise) = branches[..end].split_once(',').expect("two branches");
        out.push_str(if focused { then } else { otherwise });
        rest = &branches[end + 1..];
    }
    out.push_str(rest);
    out
}

/// §6.1.1: every pane title is a read-only title pill, an inverted block of 7, and blue (4) on the focused pane; the `━`
/// before the left cap and the caps take the block's colour; no colour 8. Pure: the format text is decoded, not run.
#[test]
fn title_pills_are_grey_blocks_unfocused_and_blue_blocks_focused() {
    let format: &str = &BORDER_FORMAT;
    let texts = [
        "#{@cpd_title}",
        "#{?#{==:#{@cpd_view},worker},Worker · #{@cpd_job} (Read-Only),#{?#{==:#{@cpd_view},tree},Context,#{?#{==:#{@cpd_view},plan},Plan,#{?#{==:#{@cpd_view},widgets},Deploy,#{pane_current_command}}}}}",
    ];
    for (focused, colour) in [(true, 4), (false, 7)] {
        let drawn = cells(&resolve_pane_active(format, focused));
        for text in texts {
            let at = pill(&drawn, text, colour);
            assert!(at > 0 && drawn[at - 1].ch == '━' && drawn[at - 1].fg == Some(colour), "the ━ before the pill (focused: {focused}) is colour {colour}: {}", show(&drawn));
        }
    }
}

/// §6.1.1: the workers pill (long and short form) and the version pill are inverted blocks of 8, with no colour inside; the preview pill is an inverted block of yellow (3). Pure: the format text is decoded, not run.
#[test]
fn the_workers_and_version_pills_are_grey_blocks_inverted() {
    let format: &str = &STATUS_FORMAT;
    let drawn = cells(format);
    pill(&drawn, "#{@cpd_version}", 8);
    pill(&drawn, "preview: #{@cpd_preview}", 3);
    // Every pill of colour 8 (the workers' and the version's) and of 3 is an inverted block with its caps in the colour.
    for (colour, count) in [(8, 2), (3, 1)] {
        let lefts: Vec<usize> = (0..drawn.len()).filter(|&i| drawn[i].ch == '\u{e0b6}' && drawn[i].fg == Some(colour)).collect();
        assert_eq!(lefts.len(), count, "pills of colour {colour}: {}", show(&drawn));
        for at in lefts {
            let end = at + drawn[at..].iter().position(|c| c.ch == '\u{e0b4}').expect("a right cap");
            assert!(drawn[at + 1..end].iter().all(|c| c.fg == Some(colour) && c.bg.is_none() && c.reverse), "block of {colour} is reversed: {}", show(&drawn[at..=end]));
            assert!(drawn[end].fg == Some(colour) && !drawn[end].reverse, "the right cap is colour {colour}, not reversed");
        }
    }
}

/// §6.1: `mode-style` and `menu-selected-style` are the blue block (foreground 4, reverse video, no background) in place of
/// tmux 3.8's yellow theme; `menu-style` is clickable text, 15 (no background: the terminal's own stays); `menu-border-style`
/// is a muted line, 8.
#[test]
fn selections_menus_and_copy_mode_are_blue_blocks() {
    let value = |name: &str| -> String {
        SERVER_OPTIONS.iter().rev().find(|o| o.get(2).map(|n| n.to_string()).as_deref() == Some(name)).unwrap_or_else(|| panic!("SERVER_OPTIONS sets no {name}"))[3].to_string()
    };
    for name in ["mode-style", "menu-selected-style"] {
        assert_eq!(style(&value(name)), (Some(4), None, true), "{name} is the blue block, reversed: {}", value(name));
    }
    assert_eq!(style(&value("menu-style")), (Some(15), None, false), "menu-style: {}", value("menu-style"));
    assert_eq!(style(&value("menu-border-style")), (Some(8), None, false), "menu-border-style: {}", value("menu-border-style"));
}
