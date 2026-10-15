//! When the Context tree's cursor is drawn: only while the pane has keyboard focus and the last input in it was a key.
//! A mouse click, or losing focus, hides it until the next arrow key.

/// The last input that reached the Context pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    /// An arrow / j / k key: the User is driving the cursor.
    Arrow,
    /// A mouse click, or the pane losing focus.
    Click,
}

/// Whether the cursor is drawn (and `n`/`x` act on it).
pub fn shown(focused: bool, last: Input, has_cursor: bool) -> bool {
    focused && has_cursor && last == Input::Arrow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_hides() {
        assert!(!shown(true, Input::Click, true));
    }

    #[test]
    fn arrow_shows() {
        assert!(shown(true, Input::Arrow, true));
    }

    #[test]
    fn focus_lost_hides() {
        assert!(!shown(false, Input::Arrow, true));
    }

    #[test]
    fn focus_regained_after_click_stays_hidden() {
        let last = Input::Click; // the click; focus then leaves and returns without a key
        assert!(!shown(false, last, true));
        assert!(!shown(true, last, true));
    }

    #[test]
    fn no_cursor_nothing_to_show() {
        assert!(!shown(true, Input::Arrow, false));
    }
}
