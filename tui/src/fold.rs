//! The fold widget (commands.md §17, "Collapsible blocks"; decision hs-d309): the one look of every block of the
//! cockpit that folds, drawn here and nowhere else. The tree's Contexts and archived `…` groups and the Inbox's
//! toggle use it; a popup or a Plan view panel that folds would too.
//!
//! - **Expanded:** the bold title line, the content lines each led by a muted branch (`│`, or `├` in the tree, which
//!   keeps its own branches), then a muted `└ collapse` line.
//! - **Folded:** the title line, then a muted `└ expand` line.
//! - **Muted** is `palette::fg(Meaning::Nothing)`: the branch, the `└` and the words `expand` and `collapse`. The
//!   content is not muted. The title is bold.
//! - **Toggling:** only the `expand` / `collapse` line toggles. [`Drawn::toggle`] is its row, so the caller can
//!   hit-test it; a click on the title line keeps the caller's own action.
//!
//! Pure: it only builds lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette::{self, Meaning};

/// The branch of a content line.
pub const BRANCH: &str = "│";
/// The branch of a content line in the tree, which keeps its own branches.
pub const TREE_BRANCH: &str = "├";
/// The glyph of a title that has no icon of its own.
pub const GLYPH: &str = "┌";

/// The closing glyph of the toggle line.
const CLOSE: &str = "└";

fn muted() -> Style {
    palette::fg(Meaning::Nothing)
}

/// A title line: `glyph`, a space and `text` (the glyph alone when `text` is empty), unstyled; [`Fold`] makes it bold.
pub fn title(glyph: &str, text: &str) -> Line<'static> {
    if text.is_empty() { Line::from(glyph.to_owned()) } else { Line::from(format!("{glyph} {text}")) }
}

/// The toggle line: a muted `└ expand` when folded, `└ collapse` when expanded.
pub fn toggle(expanded: bool) -> Line<'static> {
    Line::from(Span::styled(format!("{CLOSE} {}", if expanded { "collapse" } else { "expand" }), muted()))
}

/// A content line: the muted `branch` and a space, then the content as it is.
pub fn content(branch: &str, line: Line<'static>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{branch} "), muted())];
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
    /// The title, the content (only when expanded) and the toggle line.
    pub lines: Vec<Line<'static>>,
    /// The row, in `lines`, of the `└ expand` / `└ collapse` line: the only one that toggles.
    pub toggle: usize,
}

impl Fold {
    /// A block with `title` and `content`, led by [`BRANCH`].
    pub fn new(title: Line<'static>, content: Vec<Line<'static>>, expanded: bool) -> Fold {
        Fold { title, content, expanded, branch: BRANCH }
    }

    /// The same, its content led by `branch`.
    pub fn branch(self, branch: &'static str) -> Fold {
        Fold { branch, ..self }
    }

    /// The lines to draw. Folded, the content is not drawn.
    pub fn drawn(self) -> Drawn {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let title = Line { spans: self.title.spans.into_iter().map(|s| s.patch_style(bold)).collect(), ..self.title };
        let mut lines = vec![title];
        if self.expanded {
            lines.extend(self.content.into_iter().map(|line| content(self.branch, line)));
        }
        lines.push(toggle(self.expanded));
        Drawn { toggle: lines.len() - 1, lines }
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

    fn block(expanded: bool) -> Drawn {
        Fold::new(title(GLYPH, "Plan"), vec![Line::from("one"), Line::from("two")], expanded).drawn()
    }

    fn is_muted(span: &Span<'_>) -> bool {
        span.style.fg == Some(palette::color(Meaning::Nothing))
    }

    #[test]
    fn expanded_is_a_bold_title_the_content_behind_a_muted_branch_and_a_muted_collapse() {
        let drawn = block(true);
        assert_eq!(words(&drawn), ["┌ Plan", "│ one", "│ two", "└ collapse"]);
        assert_eq!(drawn.toggle, 3, "the toggle is the last row");
        assert!(drawn.lines[0].spans.iter().all(|s| s.style.add_modifier.contains(Modifier::BOLD)), "the title is bold");
        for row in [1, 2] {
            let spans = &drawn.lines[row].spans;
            assert_eq!(spans[0].content, "│ ");
            assert!(is_muted(&spans[0]), "the branch is muted");
            assert!(!is_muted(&spans[1]) && spans[1].style.fg.is_none(), "the content is not");
        }
        assert!(drawn.lines[3].spans.iter().all(is_muted), "`└ collapse` is muted, both the glyph and the word");
        assert!(!drawn.lines[3].spans.iter().any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn folded_is_the_title_and_a_muted_expand_and_the_content_is_not_drawn() {
        let drawn = block(false);
        assert_eq!(words(&drawn), ["┌ Plan", "└ expand"]);
        assert_eq!(drawn.toggle, 1);
        assert!(drawn.lines[0].spans.iter().all(|s| s.style.add_modifier.contains(Modifier::BOLD)));
        assert!(drawn.lines[1].spans.iter().all(is_muted));
    }

    #[test]
    fn the_tree_leads_its_content_with_its_own_branch() {
        let drawn = Fold::new(title("󰇘", ""), vec![Line::from("one")], true).branch(TREE_BRANCH).drawn();
        assert_eq!(words(&drawn), ["󰇘", "├ one", "└ collapse"]);
        assert!(is_muted(&drawn.lines[1].spans[0]));
    }

    #[test]
    fn a_block_with_no_content_is_its_title_and_its_toggle() {
        assert_eq!(words(&Fold::new(title(GLYPH, "x"), vec![], true).drawn()), ["┌ x", "└ collapse"]);
    }

    #[test]
    fn the_toggle_line_alone_is_what_the_inbox_draws() {
        assert_eq!(flat(&toggle(false)), "└ expand");
        assert_eq!(flat(&toggle(true)), "└ collapse");
        assert!(toggle(true).spans.iter().all(is_muted));
    }
}
