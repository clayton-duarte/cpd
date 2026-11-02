//! The Agent Tiers state machine of the `settings` view: rows, offered rungs, ranges, provider and model rules,
//! re-read, save steps and their baseline, keys. No drawing and no process. Stubs: the implementation fills it in.
#![allow(unused_variables)] // stub: only the tests call it until the implementation lands

use cpd_core::config::{Catalog, Letter, Role, RoleTiers};
use cpd_core::error::CpdError;
use ratatui::crossterm::event::KeyEvent;

/// A preset of `cpd config`'s `presets`: its name and its four model refs in the order O, S, H, G.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    pub models: [String; 4],
}

/// A provider of `cpd models show` and the model ids it offers (without the provider).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub provider: String,
    pub ids: Vec<String>,
}

/// What the view reads: `cpd config` (the four tier keys, the presets), `cpd tiers` and `cpd models show`.
/// The Preset row starts on the first preset; the Provider row on the provider of the catalog's O ref.
#[derive(Clone, Debug)]
pub struct Reads {
    pub tiers: RoleTiers,
    pub presets: Vec<Preset>,
    pub catalog: Catalog,
    pub providers: Vec<Provider>,
}

/// A row of the view, in drawing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Preset,
    Provider,
    Model(Letter),
    Tier(Role),
}

/// What a row shows: `label` ("Preset", "Provider", "O  opus", "S  sonnet", "H  haiku", "G  gemini", then
/// "auxiliar", "worker", "lead", "manager"); `value` (a model without its provider, `(unset)` when none, a Tier);
/// `range` ("" except on a Tier row: "default only", "<floor>..<ceiling>" or "no rung").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowView {
    pub row: Row,
    pub label: String,
    pub value: String,
    pub range: String,
    pub focused: bool,
    pub open: bool,
    pub changed: bool,
    pub invalid: bool,
    pub dimmed: bool,
}

/// The open list: the filter text, the entries it leaves, the highlighted entry and the current value's entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListView {
    pub filter: String,
    pub entries: Vec<String>,
    pub highlight: usize,
    pub current: Option<usize>,
}

/// What the view does after an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Close,
    /// Run `cpd tiers --model <letter>=<ref>` for these four candidates (O, S, H, G; full refs).
    Reread(Vec<(Letter, String)>),
    /// Run the steps of [`Model::steps`].
    Save,
}

/// A write of Save, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// `cpd preset set`: the four refs O, S, H, G.
    PresetSet([String; 4]),
    /// `cpd config tiers set`.
    TiersSet(RoleTiers),
    /// `cpd session retier --all`.
    Retier,
}

pub struct Model;

impl Model {
    pub fn new(reads: Reads) -> Model {
        todo!()
    }
    pub fn rows(&self) -> Vec<RowView> {
        todo!()
    }
    pub fn list(&self) -> Option<ListView> {
        todo!()
    }
    /// A key: Up/Down move the focus or the list's highlight, Enter opens the focused row's list or picks, a
    /// character or Backspace edits the filter, Esc closes the list, or else the view; `q` closes without a list.
    pub fn key(&mut self, key: KeyEvent) -> Effect {
        todo!()
    }
    /// Opens the list of `row`.
    pub fn open(&mut self, row: Row) {
        todo!()
    }
    /// The unfiltered entries of `row`'s list; empty when the row is dimmed.
    pub fn offered(&self, row: Row) -> Vec<String> {
        todo!()
    }
    /// Picks entry `index` of `offered(row)`.
    pub fn pick(&mut self, row: Row, index: usize) -> Effect {
        todo!()
    }
    /// `[ Load ]`: fills the Provider and the four models from the selected preset; no write.
    pub fn load(&mut self) -> Effect {
        todo!()
    }
    /// `[ Save ]`: `Save` when there are steps, `Close` when nothing changed, `None` when blocked.
    pub fn save(&mut self) -> Effect {
        todo!()
    }
    pub fn blocked(&self) -> Option<String> {
        todo!()
    }
    pub fn steps(&self) -> Vec<Step> {
        todo!()
    }
    pub fn step_done(&mut self, step: &Step) {
        todo!()
    }
    pub fn step_failed(&mut self, step: &Step, error: &CpdError) {
        todo!()
    }
    pub fn reread_done(&mut self, catalog: Catalog) {
        todo!()
    }
    pub fn reread_failed(&mut self, error: &CpdError) {
        todo!()
    }
    /// The last refusal as `<code>: <message>`, else the reason Save is blocked, else nothing.
    pub fn footer(&self) -> Option<String> {
        todo!()
    }
}
