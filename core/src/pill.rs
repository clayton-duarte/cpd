//! The pill spec, loaded from `pi/lib/pill-spec.json` (STUB).
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Spec {
    pub caps: Caps,
    pub clickable: Clickable,
    pub meanings: Meanings,
}
#[derive(Debug, Deserialize)]
pub struct Caps {
    pub left: String,
    pub right: String,
}
#[derive(Debug, Deserialize)]
pub struct Clickable {
    pub add: u8,
    pub from: u8,
    pub to: u8,
}
#[derive(Debug, Deserialize)]
pub struct Meanings {
    pub plain: u8,
    pub in_progress: u8,
    pub review: u8,
    pub done: u8,
    pub failed: u8,
    pub planning: u8,
    pub nothing: u8,
    pub focus: u8,
}

impl Spec {
    pub fn parse(_json: &str) -> Result<Spec, String> {
        unimplemented!()
    }
    pub fn clickable(&self, _index: u8) -> u8 {
        unimplemented!()
    }
}

pub fn spec() -> &'static Spec {
    unimplemented!()
}
