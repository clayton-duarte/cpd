//! cpd's mark φ is always bold (contract §6.1.1 rule (4), hs-d708): only the glyph
//! carries [`Modifier::BOLD`], never the colon or the text that follows it. One source of
//! truth for every view that draws a label containing the mark.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

/// `text` as spans: only each `φ` carries [`Modifier::BOLD`] (on top of `base`), every other
/// character takes `base` as it is.
pub fn spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let bold = base.add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('φ') {
        if at > 0 {
            spans.push(Span::styled(rest[..at].to_owned(), base));
        }
        spans.push(Span::styled("φ".to_owned(), bold));
        rest = &rest[at + 'φ'.len_utf8()..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_owned(), base));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette;

    fn rendered(spans: &[Span<'static>]) -> Vec<(String, Style)> {
        spans
            .iter()
            .map(|s| (s.content.to_string(), s.style))
            .collect()
    }

    #[test]
    fn only_the_phi_glyph_is_bold() {
        let base = palette::muted_text();
        let bold = base.add_modifier(Modifier::BOLD);
        assert_eq!(
            rendered(&spans("a φ: b φ", base)),
            [
                ("a ".to_owned(), base),
                ("φ".to_owned(), bold),
                (": b ".to_owned(), base),
                ("φ".to_owned(), bold),
            ]
        );
        assert_eq!(
            rendered(&spans("no mark", base)),
            [("no mark".to_owned(), base)]
        );
        assert_eq!(rendered(&spans("", base)), Vec::<(String, Style)>::new());
        assert_eq!(rendered(&spans("φ", base)), [("φ".to_owned(), bold)]);
    }
}
