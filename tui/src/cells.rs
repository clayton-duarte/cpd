//! Display-column arithmetic: the one place text is measured, cut or taken in terminal columns (the columns
//! `ratatui::text::Span::width()` gives, `unicode-width`'s), never in `char`s. A wide character (a CJK Plan
//! title, an Inbox label, a Worker's free text) takes two columns; `chars().count()` lays it out as one and
//! breaks the row (audit-tree F1, audit-plan F1, audit-shell F1). ASCII text is unchanged.

use ratatui::text::Span;

/// The display width of `text` in terminal columns (wide characters take two, combining marks none).
pub(crate) fn width(text: &str) -> usize {
    Span::raw(text).width()
}

/// The display width of one character in terminal columns.
pub(crate) fn char_width(c: char) -> usize {
    width(c.encode_utf8(&mut [0; 4]))
}

/// `text` cut to `width` columns with a final `…`. Counted in display columns, not chars (names are free text).
pub(crate) fn truncate(text: &str, width: usize) -> String {
    if self::width(text) <= width {
        return text.to_string();
    }
    let Some(room) = width.checked_sub(1) else {
        return String::new();
    };
    format!("{}…", take(text, room))
}

/// The longest prefix of `text` within `columns` display columns: a wide character that does not fit is left
/// out whole, for the next row.
pub(crate) fn take(text: &str, columns: usize) -> &str {
    let mut used = 0;
    for (i, c) in text.char_indices() {
        used += char_width(c);
        if used > columns {
            return &text[..i];
        }
    }
    text
}
