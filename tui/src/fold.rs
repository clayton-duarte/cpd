//! The fold widget (commands.md §17, "Collapsible blocks"; decisions hs-d337, hs-d339): the one look of every block of
//! the cockpit that folds, drawn here and nowhere else. The tree's Contexts and the Inbox's list use it; a popup or a
//! Plan view panel that folds would too.
//!
//! - **Expanded:** the title row `╭` and the title's content (bold), for example `╭󰪥 building-cpd`; each content row
//!   led by a muted branch with no indent, `├─` in the tree, which puts a glyph straight against it
//!   (`├─󰪥 cross-repo`), or `│` (the Inbox's rows, `│  <item>`); then **one** closing line, `╰─ collapse`.
//! - **Folded:** the title row (`╭󰝦 dotfiles`), then `╰─ expand`.
//! - **Muted** is `palette::fg(Meaning::Nothing)`: the lines (`╭`, `├─`, `│`, `╰─`) and the words `expand` and
//!   `collapse`. The content is not muted.
//! - **Toggling:** only the `╰─` line toggles. [`Drawn::toggle`] is its row, so the caller can hit-test it; a click on
//!   the title row keeps the caller's own action.
//!
//! Pure: it only builds lines.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette::{self, Meaning};

/// The branch of a content row that has no glyph of its own (the Inbox's).
pub const BRANCH: &str = "│";
/// The branch of a content row in the tree, whose rows lead with their glyph.
pub const TREE_BRANCH: &str = "├─";

/// The start of the title row.
const OPEN: &str = "╭";
/// The start of the toggle row.
const CLOSE: &str = "╰─";

fn muted() -> Style {
    palette::fg(Meaning::Nothing)
}

/// A title row: the muted `╭` and `content` (the glyph and the name, for example); [`Fold`] makes the content bold.
pub fn title(content: Line<'static>) -> Line<'static> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::styled(OPEN, muted())];
    spans.extend(content.spans.into_iter().map(|s| s.patch_style(bold)));
    Line { spans, ..content }
}

/// The toggle row: a muted `╰─ expand` when folded, `╰─ collapse` when expanded.
pub fn toggle(expanded: bool) -> Line<'static> {
    Line::from(Span::styled(format!("{CLOSE} {}", if expanded { "collapse" } else { "expand" }), muted()))
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
    /// The title, the content (only when expanded) and the toggle row.
    pub lines: Vec<Line<'static>>,
    /// The row, in `lines`, of the `╰─ expand` / `╰─ collapse` line: the only one that toggles.
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

    /// The lines to draw. Folded, the content is not drawn.
    pub fn drawn(self) -> Drawn {
        let mut lines = vec![title(self.title)];
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
        Fold::new(Line::from("󰪥 building-cpd"), vec![Line::from("󰪥 cross-repo"), Line::from("󰝦 other")], expanded).branch(TREE_BRANCH).drawn()
    }

    fn is_muted(span: &Span<'_>) -> bool {
        span.style.fg == Some(palette::color(Meaning::Nothing))
    }

    fn bold(span: &Span<'_>) -> bool {
        span.style.add_modifier.contains(Modifier::BOLD)
    }

    #[test]
    fn expanded_is_the_title_the_content_straight_against_its_branch_and_one_collapse() {
        let drawn = block(true);
        assert_eq!(words(&drawn), ["╭󰪥 building-cpd", "├─󰪥 cross-repo", "├─󰝦 other", "╰─ collapse"]);
        assert_eq!(drawn.toggle, 3, "the toggle is the last row, and the only one");
    }

    #[test]
    fn folded_is_the_title_and_expand_and_the_content_is_not_drawn() {
        let drawn = Fold::new(Line::from("󰝦 dotfiles"), vec![Line::from("one")], false).branch(TREE_BRANCH).drawn();
        assert_eq!(words(&drawn), ["╭󰝦 dotfiles", "╰─ expand"]);
        assert_eq!(drawn.toggle, 1);
    }

    #[test]
    fn the_lines_and_the_words_are_muted_the_content_is_not_and_the_title_is_bold() {
        let drawn = block(true);
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
        assert!(drawn.lines[3].spans.iter().all(is_muted), "`╰─ collapse` is muted, the lines and the word");
        assert!(!drawn.lines[3].spans.iter().any(bold));
        assert!(Fold::new(Line::from("x"), vec![], false).drawn().lines[1].spans.iter().all(is_muted), "`╰─ expand` too");
    }

    #[test]
    fn a_block_with_no_glyph_leads_its_content_with_the_plain_line() {
        let drawn = Fold::new(Line::from("t"), vec![Line::from(" one")], true).drawn();
        assert_eq!(words(&drawn), ["╭t", "│ one", "╰─ collapse"]);
        assert!(is_muted(&drawn.lines[1].spans[0]));
    }

    #[test]
    fn a_block_with_no_content_is_its_title_and_its_toggle() {
        assert_eq!(words(&Fold::new(Line::from("x"), vec![], true).drawn()), ["╭x", "╰─ collapse"]);
    }

    #[test]
    fn the_toggle_row_alone_is_what_the_inbox_draws() {
        assert_eq!(flat(&toggle(false)), "╰─ expand");
        assert_eq!(flat(&toggle(true)), "╰─ collapse");
        assert!(toggle(true).spans.iter().all(is_muted));
    }
}
