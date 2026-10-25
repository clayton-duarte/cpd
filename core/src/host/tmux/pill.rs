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
