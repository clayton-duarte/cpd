//! The tmux adapter of the pill spec: pure string functions over `crate::pill::spec()`.
//! Padding 0, reverse video for the text and caps not reversed are constants here, not data.

use crate::pill::spec;

/// `#[fg=colourN]`.
pub fn fg(index: u8) -> String {
    format!("#[fg=colour{index}]")
}

/// `#{?cond,#[fg=colourA],#[fg=colourB]}`.
pub fn fg_if(cond: &str, then: u8, otherwise: u8) -> String {
    format!("#{{?{cond},{},{}}}", fg(then), fg(otherwise))
}

/// `fg` + `lead` + left cap + reversed `text` + right cap.
pub fn pill(fg: &str, lead: &str, text: &str) -> String {
    let caps = &spec().caps;
    format!("{fg}{lead}{}#[reverse]{text}#[noreverse]{}", caps.left, caps.right)
}

/// `pill`, its reversed text also carrying the `italics` attribute (the incident pill, contract §6.1).
pub fn italic_pill(fg: &str, lead: &str, text: &str) -> String {
    let caps = &spec().caps;
    format!("{fg}{lead}{}#[reverse]#[italics]{text}#[noitalics]#[noreverse]{}", caps.left, caps.right)
}

/// `#[range=user|name]` + `inner` + `#[norange]`.
pub fn range(name: &str, inner: &str) -> String {
    format!("#[range=user|{name}]{inner}#[norange]")
}

/// A coloured glyph run drawn as TEXT on the pill's reversed block, then the pill's own style again.
/// The order matters: tmux sends reverse as SGR 7, so an `fg` under reverse would colour the block,
/// hence `noreverse` first; `bg` sets the block colour so the run is coloured text on it; then
/// `reverse`, `fg=colour<block>` and `bg=default` restore the pill's style. `#[default]` must not be
/// used mid-pill (it would clear the pill's style), and the colours are written `colour8`, never a bare 8 (default).
pub fn lit(block: u8, colour: u8, text: &str) -> String {
    format!("#[noreverse]#[fg=colour{colour}]#[bg=colour{block}]{text}#[reverse]#[fg=colour{block}]#[bg=default]")
}
