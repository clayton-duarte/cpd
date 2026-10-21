//! Red tests for the palette of contract §6.1.1 (hs-d534), written from the docs alone: the 16 terminal colours by number, base for
//! read-only and bright for clickable, the 15 > 7 > 8 text levels, blocks with text 0, no DIM and no REVERSED.
//! Colours are named through `palette::numbered` (no colour outside the palette), and the needles of the scan below are built at
//! run time so that this file does not trip it.
//!
//! Not pinned: `Meaning::Grounding`. §6.1.1 gives the phase no colour of its own (only "grey: the default when no meaning applies"),
//! so the implementer decides it.

use ratatui::style::{Modifier, Style};

use super::numbered::{C0, C1, C10, C11, C13, C14, C15, C2, C3, C4, C5, C6, C7, C8, C9};
use super::{
    archived_text, asleep_text, bright, clickable_text, color, color_clickable, job_pill, muted_text, pill_focus, pill_interactive,
    pill_static, read_only_text, selected, toggle_text, JobStatus, Meaning,
};
use crate::section;

/// A block: the colour as background, text 0, no modifier.
fn block(bg: ratatui::style::Color) -> Style {
    Style::new().bg(bg).fg(C0)
}

/// Text: the colour as foreground, no modifier, no background.
fn text(fg: ratatui::style::Color) -> Style {
    Style::new().fg(fg)
}

#[test]
fn job_pills_are_their_status_colour_bright_with_black_text() {
    // (status, merging, stuck, the pill's background number): draft 13, queued 15, running 14, merging 11, merged 10,
    // failed (its Lead handles it) 14, failed and stuck 9, cancelled 8; text 0.
    let table = [
        ("draft", JobStatus::Draft, false, false, C13),
        ("queued", JobStatus::Queued, false, false, C15),
        ("running", JobStatus::Running, false, false, C14),
        ("merging", JobStatus::Running, true, false, C11),
        ("merged", JobStatus::Merged, false, false, C10),
        ("failed, handled", JobStatus::Failed, false, false, C14),
        ("failed, stuck", JobStatus::Failed, false, true, C9),
        ("cancelled", JobStatus::Cancelled, false, false, C8),
    ];
    for (name, status, merging, stuck, bg) in table {
        assert_eq!(job_pill(status, merging, stuck), block(bg), "the {name} Job's pill");
    }
    assert_eq!(job_pill(JobStatus::Queued, false, false), pill_interactive(), "a queued pill is the clickable pill");
}

#[test]
fn colourless_text_follows_15_7_8() {
    let table = [
        ("clickable text", clickable_text(), C15),
        ("read-only text", read_only_text(), C7),
        ("an asleep Session's row", asleep_text(), C7),
        ("the fold toggles", toggle_text(), C7),
        ("muted text and lines", muted_text(), C8),
        ("an archived row", archived_text(), C8),
    ];
    for (name, style, fg) in table {
        assert_eq!(style, text(fg), "{name}: foreground {fg:?}, no DIM, no background");
    }
}

#[test]
fn each_meaning_has_its_decided_base_colour_and_bright_pair() {
    // (meaning, base n on what cannot be clicked, n+8 on what can; 8 has no pair)
    let table = [
        (Meaning::NotStarted, C7, C15),
        (Meaning::InProgress, C6, C14),
        (Meaning::Review, C3, C11),
        (Meaning::Done, C2, C10),
        (Meaning::Failed, C1, C9),
        (Meaning::Planning, C5, C13),
        (Meaning::Nothing, C8, C8),
        (Meaning::Waiting, C6, C14),
    ];
    let named: Vec<Meaning> = table.iter().map(|t| t.0).collect();
    let others: Vec<Meaning> = Meaning::ALL.into_iter().filter(|m| !named.contains(m)).collect();
    assert_eq!(others, [Meaning::Grounding], "every Meaning but Grounding (not pinned) is in the table");
    for (m, base, clickable) in table {
        assert_eq!(color(m), base, "{m:?} on what cannot be clicked");
        assert_eq!(color_clickable(m), clickable, "{m:?} on what can be clicked");
    }
    // the lookup the views share: n -> n+8 for 1-3, 5-7; blue is always 4 (hs-d351) and 8 has no pair
    for (base, pair) in [(C1, C9), (C2, C10), (C3, C11), (C5, C13), (C6, C14), (C7, C15), (C4, C4), (C8, C8)] {
        assert_eq!(bright(base), pair, "the pair of {base:?}");
    }
}

#[test]
fn selection_is_a_blue_block_with_black_text() {
    let expected = block(C4);
    assert_eq!(selected(), expected, "a cursor and a selection: background 4, text 0");
    assert_eq!(pill_focus(), expected, "the focused pill is the same blue block");
    for style in [selected(), pill_focus()] {
        assert!(!style.add_modifier.contains(Modifier::REVERSED), "a blue block, not reverse video: {style:?}");
    }
}

#[test]
fn read_only_pills_are_grey_blocks_with_black_text() {
    assert_eq!(pill_static(), block(C7), "a read-only pill: background 7, text 0");
    // the title pill and the count pill as the views draw them: every block of the row is background 7, text 0
    for line in [section::title("Inbox", Some(3), 30), section::collapsible_title("Inbox", Some(3), 30)] {
        let blocks: Vec<_> = line.spans.iter().filter(|s| s.style.bg.is_some()).collect();
        assert_eq!(blocks.len(), 2, "a title pill and a count pill: {line:?}");
        for span in blocks {
            assert_eq!((span.style.bg, span.style.fg), (Some(C7), Some(C0)), "{span:?}");
        }
    }
}

/// No `DIM` anywhere under `tui/` (it becomes colour 8) and no `REVERSED` outside the palette module, `palette.rs` and `palette/` (a cursor is a blue block).
#[test]
fn no_dim_or_reversed_outside_the_palette() {
    let dim = [format!("Modifier::{}", "DIM"), format!(".{}()", "dim")];
    let reversed = [format!("Modifier::{}", "REVERSED"), format!(".{}()", "reversed")];

    fn walk(dir: &std::path::Path, dim: &[String], reversed: &[String], offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, dim, reversed, offenders);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let in_palette = path.file_name().is_some_and(|n| n == "palette.rs") || path.ancestors().any(|a| a.file_name().is_some_and(|n| n == "palette"));
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    let named = dim.iter().any(|s| line.contains(s)) || (!in_palette && reversed.iter().any(|s| line.contains(s)));
                    if named {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
    let mut offenders = Vec::new();
    walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &dim, &reversed, &mut offenders);
    assert!(
        offenders.is_empty(),
        "no DIM (colour 8 instead) and no REVERSED (a blue block from palette::selected() instead) outside tui/src/palette.rs:\n{}",
        offenders.join("\n")
    );
}
