//! The palette on tmux's side (contract §6.1.1, hs-d534): pane title pills, the status line's pills, tmux's menus and copy mode.
//!
//! The formats are drawn by tmux itself: each test sets the options a format reads on a private server (its socket in a
//! temp dir, killed when the test ends), has tmux expand the format (`display-message -p`), and reads back, cell by cell,
//! the colours the `#[…]` styles give. So they pin the colours the User sees, not how the format spells them.

use super::*;

/// One cell of an expanded format: its character and the colours in force (`None` is the terminal's own).
#[derive(Debug, Clone, Copy)]
struct Cell {
    ch: char,
    fg: Option<u8>,
    bg: Option<u8>,
    reverse: bool,
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

/// The colours a style option such as `bg=blue,fg=colour0` sets: (fg, bg).
fn style(value: &str) -> (Option<u8>, Option<u8>) {
    let (mut fg, mut bg) = (None, None);
    for token in value.split([',', ' ']).filter(|t| !t.is_empty()) {
        match token.split_once('=') {
            Some(("fg", c)) => fg = colour(c),
            Some(("bg", c)) => bg = colour(c),
            _ => {}
        }
    }
    (fg, bg)
}

/// The cells of `text`, which tmux expanded but left the `#[…]` styles of.
fn cells(text: &str) -> Vec<Cell> {
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

fn text(cells: &[Cell]) -> String {
    cells.iter().map(|c| c.ch).collect()
}

fn show(cells: &[Cell]) -> String {
    cells.iter().map(|c| format!("{}[{:?},{:?}{}]", c.ch, c.fg, c.bg, if c.reverse { ",reverse" } else { "" })).collect::<Vec<_>>().join(" ")
}

/// Where the pill `<left cap>text<right cap>` starts in `cells` (at its left cap); checks the §6.1.1 rule (3): the caps are
/// `colour` on the terminal's background, the text is 0 on `colour`, and nothing is reversed.
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
        assert!(cell.fg == Some(0) && cell.bg == Some(colour) && !cell.reverse, "{text:?} is text 0 on a block of colour {colour}: {}", show(pill));
    }
    at
}

/// A private server with one session, whose socket is in a temp dir and which dies with the value.
struct Server {
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
    fn new(panes: usize) -> Server {
        let dir = tempfile::tempdir().unwrap();
        let tmux = Tmux::with_tmpdir("cpd-palette-test", dir.path());
        tmux.run(&strings(&["new-session", "-d", "-s", "t", "-x", "200", "-y", "20", "sleep 600"])).unwrap();
        let server = Server { tmux, _dir: dir };
        for _ in 1..panes {
            server.run(&["split-window", "-d", "-t", "%0", "sleep 600"]);
        }
        server
    }

    fn run(&self, args: &[&str]) -> String {
        self.tmux.run(&strings(args)).unwrap_or_else(|e| panic!("tmux {args:?}: {e:?}"))
    }

    /// `format` as tmux expands it for `pane`.
    fn render(&self, pane: &str, format: &str) -> Vec<Cell> {
        let out = self.run(&["display-message", "-p", "-t", pane, "-F", format]);
        cells(out.strip_suffix('\n').unwrap_or(&out))
    }

    /// The status line as drawn for a client `width` cells wide (a server with no client has no `client_width`).
    fn status(&self, width: usize) -> Vec<Cell> {
        self.render("%0", &STATUS_FORMAT.replace("#{client_width}", &width.to_string()))
    }
}

/// §6.1.1: every pane title is a read-only title pill, text 0 on a block of 7, and blue (4) on the focused pane; the `━`
/// before the left cap and the caps take the block's colour; no `reverse`, no colour 8.
#[test]
fn title_pills_are_grey_blocks_unfocused_and_blue_blocks_focused() {
    let server = Server::new(3);
    server.run(&["set-option", "-p", "-t", "%0", "@cpd_session", "s1"]);
    server.run(&["set-option", "-p", "-t", "%0", "@cpd_title", "Plan Title"]);
    server.run(&["set-option", "-p", "-t", "%1", "@cpd_view", "tree"]);
    let titles = [("%0", "Plan Title"), ("%1", "Context"), ("%2", "sleep")];
    for focused in ["%0", "%1", "%2"] {
        server.run(&["select-pane", "-t", focused]);
        for (pane, text) in titles {
            let colour = if pane == focused { 4 } else { 7 };
            let drawn = server.render(pane, BORDER_FORMAT);
            let at = pill(&drawn, text, colour);
            assert!(at > 0 && drawn[at - 1].ch == '━' && drawn[at - 1].fg == Some(colour), "the ━ before the pill of {pane} (focus on {focused}) is colour {colour}: {}", show(&drawn));
        }
    }
    for (view, job, text) in [("plan", "", "Plan"), ("widgets", "", "Deploy"), ("worker", "j1", "Worker · j1 (Read-Only)")] {
        server.run(&["set-option", "-p", "-t", "%1", "@cpd_view", view]);
        server.run(&["set-option", "-p", "-t", "%1", "@cpd_job", job]);
        server.run(&["select-pane", "-t", "%1"]);
        pill(&server.render("%1", BORDER_FORMAT), text, 4);
        server.run(&["select-pane", "-t", "%0"]);
        pill(&server.render("%1", BORDER_FORMAT), text, 7);
    }
}

/// §6.1.1 `TIER`: the Inbox counter's block is its tier's colour, text 0: red 1, yellow 3, magenta 5, white 7, and 8 for
/// `none`, for an unset option and for a value the engine does not set; the caps take the block's colour.
#[test]
fn the_inbox_pill_takes_its_tier_colour_inverted() {
    let server = Server::new(1);
    server.run(&["set", "-g", "@cpd_workers", "workers 1/2 · 0 waiting · 3 queued"]);
    server.run(&["set", "-g", "@cpd_workers_short", "1/2·0·3"]);
    server.run(&["set", "-g", "@cpd_workers_width", "34"]);
    server.run(&["set", "-g", "@cpd_workers_short_width", "7"]);
    let tiers = [(Some("red"), 1), (Some("yellow"), 3), (Some("magenta"), 5), (Some("white"), 7), (Some("none"), 8), (Some("other"), 8), (None, 8)];
    for (tier, colour) in tiers {
        match tier {
            Some(tier) => server.run(&["set", "-g", "@cpd_inbox_tier", tier]),
            None => server.run(&["set", "-gu", "@cpd_inbox_tier"]),
        };
        let text = if tier == Some("none") { "inbox 0" } else { "inbox 3" };
        server.run(&["set", "-g", "@cpd_inbox", text]);
        pill(&server.status(300), text, colour);
    }
}

/// §6.1.1: the workers pill (long and short form) and the version pill are blocks of 7 with text 0, whatever the Inbox's
/// tier, with no colour inside; the preview pill is yellow (3) with text 0.
#[test]
fn the_workers_and_version_pills_are_grey_blocks_with_black_text() {
    let server = Server::new(1);
    let (long, short) = ("workers 1/2 · 0 waiting · 3 queued", "1/2·0·3");
    for (option, value) in [
        ("@cpd_inbox", "inbox 3"),
        ("@cpd_inbox_tier", "red"),
        ("@cpd_workers", long),
        ("@cpd_workers_short", short),
        ("@cpd_workers_width", "34"),
        ("@cpd_workers_short_width", "7"),
        ("@cpd_version", "0.0.5-abc"),
    ] {
        server.run(&["set", "-g", option, value]);
    }
    let wide = server.status(300);
    pill(&wide, long, 7);
    pill(&wide, "0.0.5-abc", 7);
    assert!(!text(&wide).contains(short), "a wide client draws the long form: {}", show(&wide));
    // A narrow one: the long form (34 cells) and the version (12) do not fit with the left group and a fill of 1.
    let narrow = server.status(59);
    pill(&narrow, short, 7);
    assert!(!text(&narrow).contains(long), "a narrow client draws the short form: {}", show(&narrow));
    pill(&narrow, "0.0.5-abc", 7);
    server.run(&["set", "-g", "@cpd_preview", "x y"]);
    pill(&server.status(300), "preview: x y", 3);
}

/// §6.1: `mode-style` and `menu-selected-style` are the blue block (background 4, text 0) in place of reverse video or
/// tmux 3.8's yellow theme; `menu-style` is clickable text, 15 (no background: the terminal's own stays); `menu-border-style`
/// is a muted line, 8.
#[test]
fn selections_menus_and_copy_mode_are_blue_blocks() {
    let value = |name: &str| -> String {
        SERVER_OPTIONS.iter().rev().find(|o| o.get(2) == Some(&name)).unwrap_or_else(|| panic!("SERVER_OPTIONS sets no {name}"))[3].to_owned()
    };
    for name in ["mode-style", "menu-selected-style"] {
        assert_eq!(style(&value(name)), (Some(0), Some(4)), "{name} is text 0 on a block of 4: {}", value(name));
    }
    assert_eq!(style(&value("menu-style")), (Some(15), None), "menu-style: {}", value("menu-style"));
    assert_eq!(style(&value("menu-border-style")), (Some(8), None), "menu-border-style: {}", value("menu-border-style"));
}
