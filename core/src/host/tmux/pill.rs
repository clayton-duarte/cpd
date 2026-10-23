//! The tmux adapter of the pill spec (STUB): pure string functions over `crate::pill::spec()`.

pub fn fg(_index: u8) -> String {
    unimplemented!()
}

pub fn fg_if(_cond: &str, _then: u8, _otherwise: u8) -> String {
    unimplemented!()
}

pub fn pill(_fg: &str, _lead: &str, _text: &str) -> String {
    unimplemented!()
}

pub fn range(_name: &str, _inner: &str) -> String {
    unimplemented!()
}
