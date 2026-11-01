//! The cockpit's icon swap (hs-d582, hs-d583). Expected glyphs are code points written here, never the constants' own
//! values, so a wrong constant cannot agree with itself. This file spells no old code point: the guard builds them from
//! numbers.

use std::fs;
use std::path::{Path, PathBuf};

use ratatui::text::Line;

use crate::palette::{self, Meaning};
use crate::read::JobStatus;
use crate::widgets::legend;
use crate::{fold, link};

/// The code points the swap removed (none of the moon_alt frames is among them).
const OLD: [u32; 8] = [
    0xF1787, 0xF0996, 0xF073A, 0xF03CC, 0xF0B95, 0xF004B, 0xF0063, 0xF005A,
];

fn text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn section_text(title: &str) -> String {
    let sections = legend::sections();
    let s = sections
        .iter()
        .find(|s| s.title == title)
        .unwrap_or_else(|| panic!("the {title} section"));
    s.rows
        .iter()
        .flatten()
        .map(text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_job_and_check_glyphs_are_the_decided_code_points() {
    assert_eq!(palette::DRAFT, "\u{F059}");
    assert_eq!(palette::CHECKS_PENDING, "\u{F0B94}");
    assert_eq!(palette::CANCELLED, "\u{F0159}");
    assert_eq!(palette::MERGING, "\u{F1464}");
    assert_eq!(palette::FAILED, "\u{F0159}");
    assert_eq!(palette::MERGED, "\u{F05E0}");
    assert_eq!(palette::QUEUED, "\u{F0766}");
    assert_eq!(palette::job_status(JobStatus::Draft, false).0, "\u{F059}");
    assert_eq!(palette::job_status(JobStatus::Running, true).0, "\u{F1464}");
    assert_eq!(
        palette::job_status(JobStatus::Cancelled, false).0,
        "\u{F0159}"
    );
}

#[test]
fn the_waiting_glyph_is_circle_opacity_and_the_moon_alt_loader_has_28_frames() {
    assert_eq!(palette::WAITING, "\u{F1853}");
    let frames: [&str; 28] = [
        "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}",
        "\u{E3CE}", "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}",
        "\u{E3D5}", "\u{E3D6}", "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}",
        "\u{E3DC}", "\u{E3DD}", "\u{E3DE}", "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
    ];
    assert_eq!(palette::SPINNER.to_vec(), frames.to_vec());
    assert!(!palette::SPINNER.contains(&palette::WAITING));
    assert_eq!(palette::IDLE, "\u{E3D5}");
    assert_eq!(palette::NEEDS_YOU, "\u{E3E3}");
}

#[test]
fn the_link_cue_is_share_circle() {
    assert_eq!(link::CUE, "\u{F11AD}");
}

#[test]
fn the_fold_arrows_and_the_terminator_are_the_chevrons() {
    let (expand, collapse, plain) = (fold::toggle(false), fold::toggle(true), fold::plain());
    assert_eq!(text(&expand), "╰\u{F0B26} expand");
    assert_eq!(text(&collapse), "╰\u{F0B2C} collapse");
    assert_eq!(text(&plain), "╰\u{F0B2A}");
    for line in [&expand, &collapse] {
        assert_eq!(
            line.spans[0].style,
            palette::muted_text(),
            "the corner is muted"
        );
        assert_eq!(
            line.spans[1].style,
            palette::toggle_text(),
            "the glyph and the word"
        );
    }
    {
        let line = &plain;
        assert_eq!(line.spans.len(), 1);
        assert_eq!(
            line.spans[0].style,
            palette::muted_text(),
            "the terminator rows are muted"
        );
    }
}

#[test]
fn the_legend_draws_the_new_glyphs() {
    let sections = legend::sections();
    let sample = |name: &str| -> String {
        let jobs = sections
            .iter()
            .flat_map(|s| s.rows.iter())
            .find(|r| r.len() == 3 && text(&r[0]) == name)
            .unwrap_or_else(|| panic!("{name} row"));
        text(&jobs[1])
    };
    assert!(sample("draft").contains('\u{F059}'), "{}", sample("draft"));
    assert!(
        sample("landing").contains('\u{F1464}'),
        "{}",
        sample("landing")
    );
    let dots = section_text("Session Dots");
    assert!(dots.contains('\u{F1853}'), "{dots:?}");
    let precedence = "> \u{E3E3} > \u{F1853} > \u{E3D5} > \u{E3D5}";
    assert!(dots.contains(precedence), "{dots:?}");
    let _ = Meaning::ALL;
    for s in &sections {
        let all = section_text(s.title);
        for o in OLD {
            let c = char::from_u32(o).unwrap();
            assert!(!all.contains(c), "{}: U+{o:X}", s.title);
        }
    }
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Whether `line` holds the old code point `o` as the character or as a whole hex token.
fn spells(line: &str, o: u32) -> bool {
    if line.contains(char::from_u32(o).unwrap()) {
        return true;
    }
    let hex = format!("{o:X}");
    let lower = line.to_ascii_uppercase();
    lower.match_indices(&hex).any(|(i, _)| {
        let before = lower[..i].chars().next_back();
        let after = lower[i + hex.len()..].chars().next();
        !before.is_some_and(is_hex) && !after.is_some_and(is_hex)
    })
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// `path:line` of every line of `paths` that spells an old code point.
fn offenders(paths: &[PathBuf], skip: &Path) -> Vec<String> {
    let mut bad = vec![];
    for p in paths.iter().filter(|p| p.as_path() != skip) {
        let Ok(body) = fs::read_to_string(p) else {
            continue;
        };
        for (i, line) in body.lines().enumerate() {
            if OLD.iter().any(|o| spells(line, *o)) {
                bad.push(format!("{}:{}", p.display(), i + 1));
            }
        }
    }
    bad
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn the_cockpit_sources_and_tests_spell_none_of_the_old_glyphs() {
    let root = root();
    let mut paths = vec![];
    files(&root.join("tui/src"), &mut paths);
    files(&root.join("cli/tests"), &mut paths);
    let bad = offenders(&paths, &root.join("tui/src/icons_tests.rs"));
    assert!(bad.is_empty(), "old glyphs left:\n{}", bad.join("\n"));
}

#[test]
fn the_docs_spell_none_of_the_old_glyphs() {
    let root = root();
    let paths: Vec<PathBuf> = ["commands.md", "contract.md", "implementing.md"]
        .iter()
        .map(|f| root.join("docs").join(f))
        .collect();
    let bad = offenders(&paths, Path::new(""));
    assert!(bad.is_empty(), "old glyphs left:\n{}", bad.join("\n"));
}
