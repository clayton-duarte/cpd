//! The fold widget (commands.md §17, "Collapsible blocks"; decisions hs-d337, hs-d339): the one look of every block of
//! the cockpit that folds, drawn here and nowhere else. The tree's Contexts use it; a popup or a
//! Plan view panel that folds would too.
//!
//! - **Margins** (hs-d476): the widget has **one empty row above and one below**, and none inside. The tree's Contexts draw their own margins (two blocks share one empty row).
//! - **Expanded:** the title row `╭` and the title's content (bold), for example `╭󰪥 building-cpd`; each content row
//!   led by a muted branch with no indent, `├─` in the tree, which puts a glyph straight against it
//!   (`├─󰪥 cross-repo`); then **one** closing row, `╰󰬬 collapse`.
//! - **Folded:** the title row (`╭󰝦 dotfiles`), then `╰󰬦 expand`.
//! - **Nothing to fold:** a block of rows ends with the terminator ` ╰󰬪` (U+F0B2A, colour 8, no word, no toggle; hs-d574, replacing hs-d477's and hs-d459's plain `╰─` line). A tree Context block with no closing row keeps its last row's branch as `╰─` ([`LAST_BRANCH`]); a block with no row at all closes with the terminator ([`plain`]).
//! - **Muted** is `palette::muted_text()` (colour 8): the lines (`╭`, `├─`, `╰`). The toggle's arrow and word
//!   (`expand`, `collapse`) are `palette::toggle_text()` (colour 7, hs-d534). The content is not muted, and a row the widget
//!   wraps keeps its own glyphs and colours: the fold only adds muted lines.
//! - **Toggling:** only the closing row toggles. The caller hit-tests its row; a click
//!   on the title row keeps the caller's own action.
//!
//! Pure: it only builds lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

/// The branch of a content row in the tree, whose rows lead with their glyph.
pub const TREE_BRANCH: &str = "├─";

/// The branch of the last content row in the tree, in a block that has nothing to fold (hs-d477): it closes the block.
pub const LAST_BRANCH: &str = "╰─";

/// The start of the title row.
const OPEN: &str = "╭";
/// The start of the closing row.
const CLOSE: &str = "╰";
/// U+F0B26 nf-md-chevron_down_circle: the folded block's arrow.
/// The terminator glyph (hs-d574).
pub(crate) const TERMINATOR: &str = "\u{F0B2A}";

pub(crate) const DOWN: &str = "\u{F0B26}";
/// U+F0B2C nf-md-chevron_up_circle: the expanded block's arrow.
pub(crate) const UP: &str = "\u{F0B2C}";

fn muted() -> Style {
    palette::muted_text()
}

/// A title row: the muted `╭` and `content` (the glyph and the name, for example); [`Fold`] makes the content bold.
pub fn title(content: Line<'static>) -> Line<'static> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::styled(OPEN, muted())];
    spans.extend(content.spans.into_iter().map(|s| s.patch_style(bold)));
    Line { spans, ..content }
}

/// The closing row: `╰󰬦 expand` when folded, `╰󰬬 collapse` when expanded: the muted `╰`, then the arrow and the word in colour 7.
pub fn toggle(expanded: bool) -> Line<'static> {
    let (arrow, word) = if expanded {
        (UP, "collapse")
    } else {
        (DOWN, "expand")
    };
    Line::from(vec![
        Span::styled(CLOSE, muted()),
        Span::styled(format!("{arrow} {word}"), palette::toggle_text()),
    ])
}

/// The closing row of a block with nothing to fold and no row to close it (a Context with no row at all): the muted terminator `╰󰬪` and no word (commands.md §17, "Collapsible blocks"; hs-d574).
pub fn plain() -> Line<'static> {
    Line::from(Span::styled(format!("{CLOSE}{TERMINATOR}"), muted()))
}

/// The closing row of a LIVE Context with no row at all (hs-d688): the muted corner `╰` and terminator glyph, then the
/// clickable word ` new`; a left click on it opens the Context's New Plan popup, the context menu's New Plan… does.
pub fn new_plan() -> Line<'static> {
    Line::from(vec![
        Span::styled(CLOSE, muted()),
        Span::styled(TERMINATOR, muted()),
        Span::styled(" new", palette::clickable_text()),
    ])
}

/// A content row: the muted `branch`, then the content as it is.
pub fn content(branch: &str, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::styled(branch.to_owned(), muted())];
    spans.extend(line.spans);
    Line { spans, ..line }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn is_muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::muted_text().fg
    }

    fn bold(span: &Span<'_>) -> bool {
        span.style.add_modifier.contains(Modifier::BOLD)
    }

    #[test]
    fn the_toggle_row_alone_is_what_the_inbox_draws() {
        assert_eq!(flat(&toggle(false)), format!("╰{DOWN} expand"));
        assert_eq!(flat(&toggle(true)), format!("╰{UP} collapse"));
        assert_eq!(toggle(true).spans[1].style, palette::toggle_text());
    }

    // ---- section look v3 (commands.md §17, "Collapsible blocks"; hs-d349, hs-d345): red until `fold` draws the arrows ----

    #[test]
    fn the_closing_row_is_the_corner_and_the_arrow_and_the_word_folded_or_expanded() {
        assert_eq!(flat(&toggle(false)), format!("╰{DOWN} expand"));
        assert_eq!(flat(&toggle(true)), format!("╰{UP} collapse"));
    }

    #[test]
    fn the_closing_rows_corner_is_muted_and_its_arrow_and_word_are_colour_7() {
        for expanded in [false, true] {
            let row = toggle(expanded);
            assert!(
                is_muted(&row.spans[0]),
                "expanded {expanded}: {:?}",
                row.spans
            );
            assert!(
                row.spans[1..]
                    .iter()
                    .all(|s| s.style == palette::toggle_text()),
                "expanded {expanded}: {:?}",
                row.spans
            );
            assert!(!row.spans.iter().any(bold));
            assert!(
                !flat(&row).contains("╰─"),
                "the arrow replaces the `─` of the old `╰─ word`: {:?}",
                flat(&row)
            );
        }
    }

    #[test]
    fn a_block_with_nothing_to_fold_closes_with_the_muted_terminator_and_no_word() {
        let row = plain();
        assert_eq!(flat(&row), format!("╰{TERMINATOR}"));
        assert!(row.spans.iter().all(is_muted), "{:?}", row.spans);
    }
}
