//! The pill spec, loaded from `pi/lib/pill-spec.json` (STUB).
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub caps: Caps,
    pub clickable: Clickable,
    pub meanings: Meanings,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    pub left: String,
    pub right: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clickable {
    pub add: u8,
    pub from: u8,
    pub to: u8,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    pub fn parse(json: &str) -> Result<Spec, String> {
        let spec: Spec = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let m = &spec.meanings;
        let indices = [
            ("meanings.plain", m.plain),
            ("meanings.in_progress", m.in_progress),
            ("meanings.review", m.review),
            ("meanings.done", m.done),
            ("meanings.failed", m.failed),
            ("meanings.planning", m.planning),
            ("meanings.nothing", m.nothing),
            ("meanings.focus", m.focus),
            ("clickable.from", spec.clickable.from),
            ("clickable.to", spec.clickable.to),
        ];
        for (key, value) in indices {
            if !(1..=15).contains(&value) {
                return Err(format!("{key}: index {value} is outside 1..=15"));
            }
        }
        Ok(spec)
    }

    pub fn clickable(&self, index: u8) -> u8 {
        let c = &self.clickable;
        if c.from <= index && index <= c.to {
            index + c.add
        } else {
            index
        }
    }
}

pub fn spec() -> &'static Spec {
    static CELL: std::sync::LazyLock<Spec> = std::sync::LazyLock::new(|| {
        Spec::parse(include_str!("../../pi/lib/pill-spec.json"))
            .expect("pi/lib/pill-spec.json is a valid pill spec")
    });
    &CELL
}
