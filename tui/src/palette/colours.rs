//! Red tests for the palette of contract §6.1.1 (hs-d534), written from the docs alone: the 16 terminal colours by number, base for
//! read-only and bright for clickable, the 15 > 7 > 8 text levels, blocks inverted, no DIM.
//! Colours are named through `palette::numbered` (no colour outside the palette), and the needles of the scan below are built at
//! run time so that this file does not trip it.
//!
//! Not pinned: `Meaning::Grounding`. §6.1.1 gives the phase no colour of its own (only "grey: the default when no meaning applies"),
//! so the implementer decides it.

use ratatui::style::{Modifier, Style};

use super::numbered::{C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C13, C14, C15};
use super::{
    JobStatus, Meaning, archived_text, asleep_text, bright, clickable_text, color, color_clickable,
    count_style, dot_span, job_pill, muted_text, pill_focus, pill_static, read_only_text, selected,
    toggle_text,
};
use crate::read::Dot;
use crate::section;

/// A block: the colour as foreground, reset background, reverse video.
fn block(c: ratatui::style::Color) -> Style {
    Style::new()
        .fg(c)
        .bg(ratatui::style::Color::Reset)
        .add_modifier(Modifier::REVERSED)
}

/// Text: the colour as foreground, no modifier, no background.
fn text(fg: ratatui::style::Color) -> Style {
    Style::new().fg(fg)
}

#[test]
fn job_pills_are_their_status_colour_base_inverted() {
    // (status, merging, stuck, the pill's background number): draft 5, queued 7, running 6, merging 3, merged 2,
    // failed 1, cancelled 8; the base colour, inverted (a pill that is not clickable).
    let table = [
        ("draft", JobStatus::Draft, false, false, C5),
        ("queued", JobStatus::Queued, false, false, C7),
        ("running", JobStatus::Running, false, false, C6),
        ("merging", JobStatus::Running, true, false, C3),
        ("merged", JobStatus::Merged, false, false, C2),
        ("failed", JobStatus::Failed, false, false, C1),
        ("failed, stuck", JobStatus::Failed, false, true, C1),
        ("cancelled", JobStatus::Cancelled, false, false, C8),
    ];
    for (name, status, merging, stuck, bg) in table {
        assert_eq!(
            job_pill(status, merging, stuck),
            block(bg),
            "the {name} Job's pill"
        );
    }
    assert_eq!(
        job_pill(JobStatus::Queued, false, false),
        pill_static(),
        "a queued pill is the read-only pill"
    );
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
        assert_eq!(
            style,
            text(fg),
            "{name}: foreground {fg:?}, no DIM, no background"
        );
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
    let others: Vec<Meaning> = Meaning::ALL
        .into_iter()
        .filter(|m| !named.contains(m))
        .collect();
    assert_eq!(
        others,
        [Meaning::Grounding],
        "every Meaning but Grounding (not pinned) is in the table"
    );
    for (m, base, clickable) in table {
        assert_eq!(color(m), base, "{m:?} on what cannot be clicked");
        assert_eq!(
            color_clickable(m),
            clickable,
            "{m:?} on what can be clicked"
        );
    }
    // the lookup the views share: n -> n+8 for 1-3, 5-7; blue is always 4 (hs-d351) and 8 has no pair
    for (base, pair) in [
        (C1, C9),
        (C2, C10),
        (C3, C11),
        (C5, C13),
        (C6, C14),
        (C7, C15),
        (C4, C4),
        (C8, C8),
    ] {
        assert_eq!(bright(base), pair, "the pair of {base:?}");
    }
}

#[test]
fn selection_is_a_blue_block_inverted() {
    let expected = block(C4);
    assert_eq!(
        selected(),
        expected,
        "a cursor and a selection: background 4, inverted"
    );
    assert_eq!(
        pill_focus(),
        expected,
        "the focused pill is the same blue block"
    );
    for style in [selected(), pill_focus()] {
        assert!(
            style.add_modifier.contains(Modifier::REVERSED),
            "a blue block is reverse video: {style:?}"
        );
    }
}

#[test]
fn read_only_pills_are_grey_inverted_blocks() {
    assert_eq!(
        pill_static(),
        block(C7),
        "a read-only pill: colour 7, inverted"
    );
    // the title pill and the count pill as the views draw them: every block of the row is colour 7, inverted
    for line in [
        section::title("Inbox", Some(3), 30),
        section::flush_title("Inbox", Some(3), 30),
    ] {
        let blocks: Vec<_> = line
            .spans
            .iter()
            .filter(|s| s.style.add_modifier.contains(Modifier::REVERSED))
            .collect();
        assert_eq!(blocks.len(), 2, "a title pill and a count pill: {line:?}");
        for span in blocks {
            assert_eq!(
                (span.style.fg, span.style.bg),
                (Some(C7), Some(ratatui::style::Color::Reset)),
                "{span:?}"
            );
        }
    }
}

/// No `DIM` anywhere under `tui/` (it becomes colour 8) and no `REVERSED` outside the palette module, `palette.rs`, `palette/` and the tests that pin the blocks' reverse video (a cursor is a blue block).
#[test]
fn no_dim_or_reversed_outside_the_palette() {
    let dim = [format!("Modifier::{}", "DIM"), format!(".{}()", "dim")];
    let reversed = [
        format!("Modifier::{}", "REVERSED"),
        format!(".{}()", "reversed"),
    ];

    fn walk(
        dir: &std::path::Path,
        dim: &[String],
        reversed: &[String],
        offenders: &mut Vec<String>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, dim, reversed, offenders);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let pins_blocks = [
                    "pill_colours.rs",
                    "pill_tests.rs",
                    "view_tests.rs",
                    "input_tests.rs",
                    "input_box_tests.rs",
                    "layout_tests.rs",
                    "box_widget_tests.rs",
                ];
                let in_palette = path
                    .file_name()
                    .is_some_and(|n| n == "palette.rs" || pins_blocks.iter().any(|p| n == *p))
                    || path
                        .ancestors()
                        .any(|a| a.file_name().is_some_and(|n| n == "palette"));
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    let named = dim.iter().any(|s| line.contains(s))
                        || (!in_palette && reversed.iter().any(|s| line.contains(s)));
                    if named {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
    let mut offenders = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &dim,
        &reversed,
        &mut offenders,
    );
    assert!(
        offenders.is_empty(),
        "no DIM (colour 8 instead) and no REVERSED (a blue block from palette::selected() instead) outside tui/src/palette.rs:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn counters_are_in_their_status_colour_bright_and_a_failed_count_is_red() {
    let table = [
        (JobStatus::Draft, false, C13),
        (JobStatus::Queued, false, C15),
        (JobStatus::Running, false, C14),
        (JobStatus::Running, true, C11),
        (JobStatus::Merged, false, C10),
        (JobStatus::Failed, false, C9),
        (JobStatus::Cancelled, false, C8),
    ];
    for (status, merging, want) in table {
        assert_eq!(
            count_style(status, merging, 3),
            text(want),
            "{status:?} merging={merging}"
        );
        assert_eq!(count_style(status, merging, 0), text(C8), "{status:?} zero");
    }
}

#[test]
fn the_red_loader_is_the_28_moon_frames_in_red_and_archived_the_hollow_ring_in_colour_8() {
    let moons = [
        "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}",
        "\u{E3CE}", "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}",
        "\u{E3D5}", "\u{E3D6}", "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}",
        "\u{E3DC}", "\u{E3DD}", "\u{E3DE}", "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
    ];
    for f in 0..56 {
        let span = dot_span(Dot::Fixing, f, false);
        assert_eq!(span.content, moons[f % 28], "frame {f}");
        assert_eq!(span.style, text(C9), "frame {f}: red 9");
        let archived = dot_span(Dot::Fixing, f, true);
        assert_eq!(
            archived.content, "\u{E3D5}",
            "archived, frame {f}: the hollow ring"
        );
        assert_eq!(archived.style, archived_text(), "archived, frame {f}");
        assert_eq!(archived.style, text(C8));
    }
    assert_eq!(
        dot_span(Dot::Working(None), 3, false).style,
        clickable_text(),
        "a working Session with no Plan is white"
    );
}
