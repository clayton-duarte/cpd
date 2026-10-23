//! The fold widget (commands.md §17, "Collapsible blocks"; decisions hs-d337, hs-d339): the one look of every block of
//! the cockpit that folds, drawn here and nowhere else. The tree's Contexts and the Inbox's list use it; a popup or a
//! Plan view panel that folds would too.
//!
//! - **Margins** (hs-d476): the widget has **one empty row above and one below**, and none inside: [`Fold::drawn`]
//!   draws them. The tree's Contexts and the Inbox each draw their own margins (two blocks share one empty row).
//! - **Expanded:** the title row `╭` and the title's content (bold), for example `╭󰪥 building-cpd`; each content row
//!   led by a muted branch with no indent, `├─` in the tree, which puts a glyph straight against it
//!   (`├─󰪥 cross-repo`), or `│` (the Inbox's rows, `│ <icon> <item>`); then **one** closing row, `╰󰬬 collapse`.
//! - **Folded:** the title row (`╭󰝦 dotfiles`), then `╰󰬦 expand`.
//! - **Nothing to fold:** a block of rows ends with the terminator ` ╰󰬪` (U+F0B2A, colour 8, no word, no toggle; hs-d574, replacing hs-d477's and hs-d459's plain `╰─` line). A tree Context block with no closing row keeps its last row's branch as `╰─` ([`LAST_BRANCH`]); a block with no row at all closes with the terminator ([`done`]).
//! - **Muted** is `palette::muted_text()` (colour 8): the lines (`╭`, `├─`, `│`, `╰`). The toggle's arrow and word
//!   (`expand`, `collapse`) are `palette::toggle_text()` (colour 7, hs-d534). The content is not muted, and a row the widget
//!   wraps keeps its own glyphs and colours: the fold only adds muted lines.
//! - **Toggling:** only the closing row toggles. [`Drawn::toggle`] is its row, so the caller can hit-test it; a click
//!   on the title row keeps the caller's own action.
//!
//! Pure: it only builds lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette;

/// The branch of a content row that has no glyph of its own (the Inbox's).
pub const BRANCH: &str = "│";
/// The branch of a content row in the tree, whose rows lead with their glyph.
pub const TREE_BRANCH: &str = "├─";

/// The branch of the last content row in the tree, in a block that has nothing to fold (hs-d477): it closes the block.
pub const LAST_BRANCH: &str = "╰─";

/// The start of the title row.
const OPEN: &str = "╭";
/// The start of the closing row.
const CLOSE: &str = "╰";
/// U+F0B26 nf-md-chevron_down_circle: the folded block's arrow.
/// The terminator glyph (hs-d574), also the empty Inbox's closing row glyph (hs-d487).
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
    let (arrow, word) = if expanded { (UP, "collapse") } else { (DOWN, "expand") };
    Line::from(vec![Span::styled(CLOSE, muted()), Span::styled(format!("{arrow} {word}"), palette::toggle_text())])
}

/// The closing row of a block with nothing to fold and no row to close it (a Context with no row at all; the Inbox with 3
/// items or fewer): the muted terminator `╰󰬪` and no word (commands.md §17, "Collapsible blocks"; hs-d574).
pub fn plain() -> Line<'static> {
    Line::from(Span::styled(format!("{CLOSE}{TERMINATOR}"), muted()))
}

/// The closing row of a block with nothing to fold and no row to close it (a Context with no row at all; the Inbox with 3
/// items or fewer): a muted `╰`, the terminator glyph U+F0B2A and the word (the empty Inbox; hs-d487, hs-d574).
pub fn done(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("{CLOSE}{TERMINATOR} {text}"), muted()))
}

/// A content row: the muted `branch`, then the content as it is.
pub fn content(branch: &str, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::styled(branch.to_owned(), muted())];
    spans.extend(line.spans);
    Line { spans, ..line }
}

/// One block that folds.
pub struct Fold {
    title: Line<'static>,
    content: Vec<Line<'static>>,
    expanded: bool,
    branch: &'static str,
}

/// The lines of a block, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    /// The empty row above, the title, the content (only when expanded), the toggle row and the empty row below.
    pub lines: Vec<Line<'static>>,
    /// The row, in `lines`, of the `╰󰬦 expand` / `╰󰬬 collapse` closing row: the only one that toggles (the empty row below follows).
    pub toggle: usize,
}

impl Fold {
    /// A block with the title `title` (what follows the `╭`) and `content`, led by [`BRANCH`].
    pub fn new(title: Line<'static>, content: Vec<Line<'static>>, expanded: bool) -> Fold {
        Fold { title, content, expanded, branch: BRANCH }
    }

    /// The same, its content led by `branch`.
    pub fn branch(self, branch: &'static str) -> Fold {
        Fold { branch, ..self }
    }

    /// The lines to draw, between the margin rows. Folded, the content is not drawn.
    pub fn drawn(self) -> Drawn {
        let mut lines = vec![Line::default(), title(self.title)];
        if self.expanded {
            lines.extend(self.content.into_iter().map(|line| content(self.branch, line)));
        }
        lines.push(toggle(self.expanded));
        let toggle = lines.len() - 1;
        lines.push(Line::default());
        Drawn { toggle, lines }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn words(drawn: &Drawn) -> Vec<String> {
        drawn.lines.iter().map(flat).collect()
    }

    /// `drawn` without its margin rows: the block itself.
    fn bare(mut drawn: Drawn) -> Drawn {
        drawn.lines.pop();
        drawn.lines.remove(0);
        drawn.toggle -= 1;
        drawn
    }

    fn block(expanded: bool) -> Drawn {
        Fold::new(Line::from("󰪥 building-cpd"), vec![Line::from("󰪥 cross-repo"), Line::from("󰝦 other")], expanded).branch(TREE_BRANCH).drawn()
    }

    fn is_muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::muted_text().fg
    }

    fn bold(span: &Span<'_>) -> bool {
        span.style.add_modifier.contains(Modifier::BOLD)
    }

    #[test]
    fn expanded_is_the_title_the_content_straight_against_its_branch_and_one_collapse() {
        let drawn = bare(block(true));
        assert_eq!(words(&drawn), ["╭󰪥 building-cpd", "├─󰪥 cross-repo", "├─󰝦 other", format!("╰{UP} collapse").as_str()]);
        assert_eq!(drawn.toggle, 3, "the toggle is the last row, and the only one");
    }

    #[test]
    fn folded_is_the_title_and_expand_and_the_content_is_not_drawn() {
        let drawn = bare(Fold::new(Line::from("󰝦 dotfiles"), vec![Line::from("one")], false).branch(TREE_BRANCH).drawn());
        assert_eq!(words(&drawn), ["╭󰝦 dotfiles", format!("╰{DOWN} expand").as_str()]);
        assert_eq!(drawn.toggle, 1);
    }

    #[test]
    fn the_lines_and_the_words_are_muted_the_content_is_not_and_the_title_is_bold() {
        let drawn = bare(block(true));
        let title = &drawn.lines[0].spans;
        assert_eq!(title[0].content, "╭");
        assert!(is_muted(&title[0]) && !bold(&title[0]), "the `╭` is muted");
        assert!(title[1..].iter().all(|s| bold(s) && !is_muted(s)), "the title's content is bold, not muted");
        for row in [1, 2] {
            let spans = &drawn.lines[row].spans;
            assert_eq!(spans[0].content, "├─");
            assert!(is_muted(&spans[0]), "the branch is muted");
            assert!(spans[1..].iter().all(|s| !is_muted(s) && s.style.fg.is_none()), "the content is not");
        }
        assert!(is_muted(&drawn.lines[3].spans[0]), "the closing row's corner is muted");
        assert!(!drawn.lines[3].spans.iter().any(bold));
        assert!(is_muted(&bare(Fold::new(Line::from("x"), vec![], false).drawn()).lines[1].spans[0]), "folded too");
    }

    #[test]
    fn a_block_with_no_glyph_leads_its_content_with_the_plain_line() {
        let drawn = bare(Fold::new(Line::from("t"), vec![Line::from(" one")], true).drawn());
        assert_eq!(words(&drawn), ["╭t", "│ one", format!("╰{UP} collapse").as_str()]);
        assert!(is_muted(&drawn.lines[1].spans[0]));
    }

    #[test]
    fn a_block_with_no_content_is_its_title_and_its_toggle() {
        assert_eq!(words(&bare(Fold::new(Line::from("x"), vec![], true).drawn())), ["╭x", format!("╰{UP} collapse").as_str()]);
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
            assert!(is_muted(&row.spans[0]), "expanded {expanded}: {:?}", row.spans);
            assert!(row.spans[1..].iter().all(|s| s.style == palette::toggle_text()), "expanded {expanded}: {:?}", row.spans);
            assert!(!row.spans.iter().any(bold));
            assert!(!flat(&row).contains("╰─"), "the arrow replaces the `─` of the old `╰─ word`: {:?}", flat(&row));
        }
    }

    #[test]
    fn a_block_with_nothing_to_fold_closes_with_the_muted_terminator_and_no_word() {
        let row = plain();
        assert_eq!(flat(&row), format!("╰{TERMINATOR}"));
        assert!(row.spans.iter().all(is_muted), "{:?}", row.spans);
    }

    #[test]
    fn a_block_drawn_by_the_widget_ends_in_one_arrow_row_and_the_toggle_is_that_row() {
        let folded = bare(Fold::new(Line::from("t"), vec![Line::from(" one")], false).drawn());
        assert_eq!(folded.lines.last().map(flat), Some(format!("╰{DOWN} expand")));
        let open = bare(Fold::new(Line::from("t"), vec![Line::from(" one")], true).drawn());
        assert_eq!(open.lines.last().map(flat), Some(format!("╰{UP} collapse")));
        assert_eq!(open.toggle, open.lines.len() - 1);
        assert_eq!(open.lines.iter().filter(|l| flat(l).starts_with('╰')).count(), 1, "one closing row");
    }

    #[test]
    fn a_wrapped_row_keeps_its_own_glyphs_and_colours_and_the_fold_only_adds_muted_lines() {
        let item = Line::from(vec![
            Span::raw(" "),
            Span::styled(palette::NEEDS_YOU, palette::fg(palette::Meaning::Failed)),
            Span::raw(" cpd#129 merge conflict"),
            Span::styled("  18m", palette::muted_text()),
        ]);
        let drawn = bare(Fold::new(Line::from("title"), vec![item.clone()], true).drawn());
        let row = &drawn.lines[1];
        assert_eq!(flat(row), format!("│ {} cpd#129 merge conflict  18m", palette::NEEDS_YOU));
        assert_eq!(row.spans[0].content, BRANCH);
        assert!(is_muted(&row.spans[0]), "only the line is added, and it is muted");
        assert_eq!(&row.spans[1..], &item.spans[..], "every span of the wrapped row, as it was: glyph, colour and age");
        assert_eq!(row.spans[2].style.fg, Some(palette::color(palette::Meaning::Failed)), "the icon keeps its colour");
        assert_eq!(flat(drawn.lines.last().unwrap()), format!("╰{UP} collapse"), "and the block closes with the arrow row");
    }

    // ---- spacing v2 (commands.md §17, "Collapsible blocks"; hs-d476): red until `Fold::drawn` adds the margins ----

    #[test]
    fn a_collapsible_widget_has_one_empty_row_above_and_one_below_and_none_inside() {
        assert_eq!(
            words(&block(true)),
            ["", "╭󰪥 building-cpd", "├─󰪥 cross-repo", "├─󰝦 other", format!("╰{UP} collapse").as_str(), ""],
            "expanded: the margins, and the body with no vertical spacing"
        );
        let folded = Fold::new(Line::from("󰝦 dotfiles"), vec![Line::from("one")], false).branch(TREE_BRANCH).drawn();
        assert_eq!(words(&folded), ["", "╭󰝦 dotfiles", format!("╰{DOWN} expand").as_str(), ""], "folded");
        assert_eq!(words(&Fold::new(Line::from("x"), vec![], true).drawn()), ["", "╭x", format!("╰{UP} collapse").as_str(), ""], "no content");
    }

    #[test]
    fn the_toggle_is_still_the_closing_row_between_the_margins() {
        for (drawn, word) in [(block(true), "collapse"), (block(false), "expand")] {
            assert_eq!(drawn.toggle, drawn.lines.len() - 2, "the row above the bottom margin");
            assert!(flat(&drawn.lines[drawn.toggle]).starts_with('╰') && flat(&drawn.lines[drawn.toggle]).ends_with(word), "{:?}", words(&drawn));
            assert_eq!(drawn.lines.iter().filter(|l| flat(l).starts_with('╰')).count(), 1, "one closing row");
            assert_eq!(drawn.lines.iter().filter(|l| l.spans.iter().all(|s| s.content.is_empty())).count(), 2, "two empty rows, no more");
        }
    }
}
