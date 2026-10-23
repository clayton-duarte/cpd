//! The Inbox's notice tier is cyan; blue is only for focus (commands.md §13.2 "Inbox section", §17 legend; contract §6.1.1;
//! pro-d16). `cpd tui view tree` runs in a pane of the env's private tmux server over a store whose Inbox is written as
//! rows (the tree reads the Inbox, it does not compute it); the legend is drawn with ratatui's `TestBackend`.

use crate::common;

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::widgets::legend;
use nix::fcntl::{Flock, FlockArg};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

const SESSION: &str = "tree";
/// The tier glyphs, repeated here as the tests of `tui_tree.rs` do: white, cyan, magenta, yellow, red.
const WHITE: &str = "\u{F0366}";
const CYAN: &str = "\u{F06F1}";
const RED: &str = "\u{F0362}";

// ---------- the tree's Inbox section ----------

/// An env with an engine running (`engine-down` is no row of the Inbox) and an Inbox of one item per tier: a red dialog, a
/// cyan review request (it has a `url`: a click copies it, so the row is clickable, §13.2), a white unread.
fn env() -> TestEnv {
    let env = TestEnv::new();
    let config = format!("version = 1\nmachine = \"m1\"\n[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    let path = env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = Flock::lock(std::fs::File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    std::mem::forget(lock);
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    let ago = |seconds: i64| {
        let at = time::OffsetDateTime::now_utc() - time::Duration::seconds(seconds);
        at.format(&time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap()
    };
    sql(
        &mut store,
        &format!(
            "INSERT INTO inbox_items (kind, urgency, subject, label, url, opened_at) VALUES
               ('dialog', 'red', 'session:r', 'red label', NULL, '{}'),
               ('review-requested', 'cyan', 'pr:acme/web#5', 'cyan label', 'https://x.io/review', '{}'),
               ('unread', 'white', 'session:w', 'white label', NULL, '{}');",
            ago(900),
            ago(600),
            ago(300)
        ),
    );
    env
}

fn start(env: &TestEnv) {
    let out = env.tmux(&["-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", SESSION, "-x", "46", "-y", "12", &format!("{CPD} tui view tree")]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// `text` without its OSC 8 hyperlink sequences (`ESC ] 8 ; ; <url>` ended by BEL or ST): tmux 3.8's `capture-pane -e` writes them
/// around each piece of a link kind's label, and the rows are read as SGR text only.
fn strip_osc8(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("\x1b]8;;") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail[5..].find(['\x07', '\x1b']).map(|i| if tail.as_bytes()[5 + i] == 0x07 { 5 + i + 1 } else { 5 + i + 2 });
        rest = &tail[end.unwrap_or(tail.len())..];
    }
    out.push_str(rest);
    out
}

/// The screen with its escapes, once the three rows are drawn.
fn styled_screen(env: &TestEnv) -> String {
    common::wait_some("the Inbox's three rows", common::WAIT, || {
        let out = env.tmux(&["capture-pane", "-p", "-e", "-t", SESSION]);
        let text = strip_osc8(&String::from_utf8(out.stdout).unwrap());
        if ["red label", "cyan label", "white label"].iter().all(|l| text.contains(l)) { Ok(text) } else { Err(text) }
    })
}

/// The cells of one screen row with escapes: each glyph and its foreground as an SGR colour index, `None` the terminal's own.
fn cells_of(styled: &str) -> Vec<(char, Option<u8>)> {
    let (mut out, mut fg) = (Vec::new(), None);
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push((c, fg));
            continue;
        }
        if chars.next() == Some('[') {
            let mut params = String::new();
            for c in chars.by_ref() {
                if c.is_ascii_digit() || c == ';' || c == ':' {
                    params.push(c);
                    continue;
                }
                if c == 'm' {
                    let mut p = params.split(';').map(|p| p.parse::<u8>().unwrap_or(0));
                    while let Some(n) = p.next() {
                        match n {
                            0 | 39 => fg = None,
                            30..=37 => fg = Some(n - 30),
                            90..=97 => fg = Some(n - 90 + 8),
                            38 => match p.next() {
                                Some(5) => fg = p.next(),
                                _ => {
                                    p.by_ref().take(3).for_each(drop);
                                    fg = Some(u8::MAX);
                                }
                            },
                            _ => {}
                        }
                    }
                }
                break;
            }
        }
    }
    out
}

/// The foreground of the glyph of the Inbox row that says `label`: the first glyph after the rail.
fn glyph_fg(screen: &str, label: &str) -> (char, Option<u8>) {
    let cells = screen.lines().map(cells_of).find(|c| c.iter().map(|(c, _)| *c).collect::<String>().contains(label)).unwrap_or_else(|| panic!("{label} on\n{screen}"));
    let rail = cells.iter().position(|(c, _)| *c == '│').expect("the rail");
    *cells[rail + 1..].iter().find(|(c, _)| *c != ' ').expect("the glyph")
}

#[test]
fn a_clickable_cyan_inbox_row_draws_its_glyph_in_bright_cyan_and_no_tier_is_blue() {
    let env = env();
    start(&env);
    let screen = styled_screen(&env);
    assert_eq!(glyph_fg(&screen, "cyan label"), (CYAN.chars().next().unwrap(), Some(14)), "{screen:?}");
    for (label, glyph, fg) in [("red label", RED, 9), ("white label", WHITE, 15)] {
        assert_eq!(glyph_fg(&screen, label), (glyph.chars().next().unwrap(), Some(fg)), "{label}");
    }
    for label in ["red label", "cyan label", "white label"] {
        let fg = glyph_fg(&screen, label).1;
        assert!(fg != Some(4) && fg != Some(12), "no Inbox tier is blue (4, 12): {label}: {fg:?}");
    }
    // the order of the rows: red, cyan, white
    let at = |l: &str| screen.find(l).unwrap();
    assert!(at("red label") < at("cyan label") && at("cyan label") < at("white label"));
}

#[test]
fn the_notice_tiers_colour_is_cyan_6_read_only_and_14_clickable_never_blue() {
    assert_eq!(palette::color(Meaning::Notice), Color::Cyan, "base, read-only: 6");
    assert_eq!(palette::color_clickable(Meaning::Notice), Color::LightCyan, "bright, clickable: 14");
    assert_eq!(palette::fg(Meaning::Notice).fg, Some(Color::Cyan));
    for blue in [Color::Blue, Color::LightBlue] {
        assert!(palette::color(Meaning::Notice) != blue && palette::color_clickable(Meaning::Notice) != blue);
    }
}

// ---------- the legend ----------

fn legend_buffer() -> Buffer {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_owned()).collect::<String>()).collect()
}

#[test]
fn the_legends_inbox_section_has_the_cyan_tier_line_in_bright_cyan_and_none_for_blue() {
    let buf = legend_buffer();
    let lines = rows(&buf);
    let notice = lines.iter().position(|r| r.contains("a cpd notice: curation, nothing blocks")).expect("the notice tier's line");
    let glyph = (0..buf.area.width).find(|x| buf[(*x, notice as u16)].symbol() == CYAN).expect("U+F06F1 on the notice line");
    assert_eq!(buf[(glyph, notice as u16)].fg, Color::LightCyan, "bright cyan (14): the legend's sample is drawn as the Inbox row is");
    assert!(lines[notice].contains("cyan"), "the line names the tier: {:?}", lines[notice]);
    // the other four tiers keep their lines
    for text in ["a blocker only you can clear", "pull-request work", "a decision before the work runs", "an unread reply"] {
        assert!(lines.iter().any(|r| r.contains(text)), "{text}");
    }
    assert!(!lines.iter().any(|r| r.contains("blue:")), "no line for blue as a tier or a dot:\n{}", lines.join("\n"));
    assert!(lines.iter().any(|r| r.contains("cyan:")), "the Session Dots' cyan line:\n{}", lines.join("\n"));
}

#[test]
fn the_session_dots_precedence_line_reads_red_yellow_magenta_cyan_white() {
    let lines = rows(&legend_buffer());
    assert!(lines.iter().any(|r| r.contains("red > yellow > magenta > cyan > white > ")), "{}", lines.join("\n"));
    assert!(!lines.iter().any(|r| r.contains("magenta > blue")), "{}", lines.join("\n"));
}
