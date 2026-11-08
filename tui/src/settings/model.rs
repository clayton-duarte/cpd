//! The Agent Tiers state machine of the `settings` view: rows, offered rungs, ranges, provider and model rules,
//! re-read, save steps and their baseline, keys. No drawing and no process.

use cpd_core::config::Tier;
use cpd_core::config::{Catalog, Letter, Role, RoleTiers};
use cpd_core::error::{CpdError, ErrorCode};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use serde_json::Value;

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

/// The level of the two-level popup: the menu that opens it, and its two screens (commands.md §13.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Menu,
    Tiers,
    Workers,
}

/// The menu's entries, in drawing order; picking one opens its screen.
pub const MENU: [&str; 2] = ["Agent Tiers", "Workers"];

/// A row of the view, in drawing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Preset,
    Provider,
    Model(Letter),
    Tier(Role),
    Experimental(Feature),
}

/// An Experimental Feature's switch, in the order of docs/commands.md §14.2b.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    DurableWorkers,
    WriteCuration,
    FailureTriage,
    TurnQuestionGuard,
}

impl Feature {
    pub const ALL: [Feature; 4] = [
        Feature::DurableWorkers,
        Feature::WriteCuration,
        Feature::FailureTriage,
        Feature::TurnQuestionGuard,
    ];

    /// The key in `[experimental]` and the argument of `cpd config experimental`.
    pub fn key(self) -> &'static str {
        match self {
            Feature::DurableWorkers => "durable_workers",
            Feature::WriteCuration => "write_curation",
            Feature::FailureTriage => "failure_triage",
            Feature::TurnQuestionGuard => "turn_question_guard",
        }
    }

    fn at(self) -> usize {
        Feature::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }
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
    /// `cpd config concurrency set <n>`.
    Concurrency(u32),
    /// `cpd config experimental <feature> <on|off>`.
    Experimental(Feature, bool),
    /// `cpd session retier --all`.
    Retier,
}

const LETTERS: [Letter; 4] = [Letter::O, Letter::S, Letter::H, Letter::G];
const ROLES: [Role; 4] = [Role::Auxiliar, Role::Worker, Role::Lead, Role::Manager];

fn index(letter: Letter) -> usize {
    LETTERS.iter().position(|l| *l == letter).unwrap_or(0)
}

/// The provider of a ref: the text before its first `/`.
fn provider_of(model_ref: &str) -> &str {
    model_ref.split_once('/').map_or(model_ref, |(p, _)| p)
}

fn tier_of(tiers: &RoleTiers, role: Role) -> Tier {
    match role {
        Role::Auxiliar => tiers.auxiliar,
        Role::Worker => tiers.worker,
        Role::Lead => tiers.lead,
        Role::Manager => tiers.manager,
    }
}

fn set_tier(tiers: &mut RoleTiers, role: Role, tier: Tier) {
    match role {
        Role::Auxiliar => tiers.auxiliar = tier,
        Role::Worker => tiers.worker = tier,
        Role::Lead => tiers.lead = tier,
        Role::Manager => tiers.manager = tier,
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Auxiliar => "auxiliar",
        Role::Worker => "worker",
        Role::Lead => "lead",
        Role::Manager => "manager",
    }
}

fn shape(what: &str) -> CpdError {
    CpdError::new(
        ErrorCode::Internal,
        format!("Unexpected output shape: {what}."),
    )
}

fn text<'a>(v: &'a Value, key: &str, what: &str) -> Result<&'a str, CpdError> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| shape(&format!("{what}.{key} is not a string")))
}

fn flag(v: &Value, key: &str) -> Result<bool, CpdError> {
    v.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| shape(&format!("config.experimental.{key} is not a boolean")))
}

/// Parses the `concurrency` of `cpd config`: the machine's concurrent Worker count (commands.md §14.2c).
pub fn concurrency(config: &Value) -> Result<u32, CpdError> {
    let n = config
        .get("config")
        .and_then(|c| c.get("concurrency"))
        .and_then(Value::as_u64)
        .ok_or_else(|| shape("config.concurrency is not an integer"))?;
    u32::try_from(n).map_err(|_| shape("config.concurrency is out of range"))
}

/// Parses the `experimental` object of `cpd config`: the four switches, in [`Feature::ALL`] order.
pub fn experimental(config: &Value) -> Result<[bool; 4], CpdError> {
    let obj = config
        .get("config")
        .and_then(|c| c.get("experimental"))
        .ok_or_else(|| shape("config.experimental is missing"))?;
    let mut on = [false; 4];
    for f in Feature::ALL {
        on[f.at()] = flag(obj, f.key())?;
    }
    Ok(on)
}

fn tier_model(v: &Value, letter: Letter) -> Result<cpd_core::config::TierModel, CpdError> {
    let what = format!("tiers.{letter}");
    let entry = v
        .get(letter.to_string())
        .ok_or_else(|| shape(&format!("{what} is missing")))?;
    let efforts = entry
        .get("efforts")
        .and_then(Value::as_array)
        .ok_or_else(|| shape(&format!("{what}.efforts is not an array")))?
        .iter()
        .map(|e| {
            e.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| shape(&format!("{what}.efforts holds a non-effort")))
        })
        .collect::<Result<Vec<u8>, CpdError>>()?;
    Ok(cpd_core::config::TierModel {
        model: text(entry, "model", &what)?.to_owned(),
        efforts,
    })
}

/// Parses the JSON of `cpd tiers`: the catalog of the four letters.
pub fn catalog(tiers: &Value) -> Result<Catalog, CpdError> {
    let tm = tiers
        .get("tiers")
        .ok_or_else(|| shape("tiers.tiers is missing"))?;
    Ok(Catalog {
        pi_version: text(tiers, "pi_version", "tiers")?.to_owned(),
        o: tier_model(tm, Letter::O)?,
        s: tier_model(tm, Letter::S)?,
        h: tier_model(tm, Letter::H)?,
        g: tier_model(tm, Letter::G)?,
    })
}

impl Reads {
    /// Parses the JSON of `cpd config`, `cpd tiers` and `cpd models show`; a wrong shape is an error.
    pub fn parse(config: &Value, tiers: &Value, models: &Value) -> Result<Reads, CpdError> {
        let cfg = config
            .get("config")
            .ok_or_else(|| shape("config.config is missing"))?;
        let tier = |key: &str| Tier::parse(text(cfg, key, "config")?);
        let role_tiers = RoleTiers {
            auxiliar: tier("context_distill_tier")?,
            worker: tier("worker_tier")?,
            lead: tier("lead_tier")?,
            manager: tier("manager_tier")?,
        };
        let mut presets = Vec::new();
        if let Some(map) = cfg.get("presets") {
            let map = map
                .as_object()
                .ok_or_else(|| shape("config.presets is not an object"))?;
            for (name, refs) in map {
                let get = |l: Letter| -> Result<String, CpdError> {
                    Ok(text(refs, &l.to_string(), &format!("presets.{name}"))?.to_owned())
                };
                presets.push(Preset {
                    name: name.clone(),
                    models: [
                        get(Letter::O)?,
                        get(Letter::S)?,
                        get(Letter::H)?,
                        get(Letter::G)?,
                    ],
                });
            }
        }
        let catalog = catalog(tiers)?;
        let providers = models
            .get("providers")
            .and_then(Value::as_array)
            .ok_or_else(|| shape("models.providers is not an array"))?
            .iter()
            .map(|p| {
                let ids = p
                    .get("ids")
                    .and_then(Value::as_array)
                    .ok_or_else(|| shape("providers[].ids is not an array"))?
                    .iter()
                    .map(|i| {
                        i.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| shape("providers[].ids holds a non-string"))
                    })
                    .collect::<Result<Vec<String>, CpdError>>()?;
                Ok(Provider {
                    provider: text(p, "provider", "providers[]")?.to_owned(),
                    ids,
                })
            })
            .collect::<Result<Vec<Provider>, CpdError>>()?;
        Ok(Reads {
            tiers: role_tiers,
            presets,
            catalog,
            providers,
        })
    }
}

#[derive(Clone, Debug)]
struct OpenList {
    row: Row,
    filter: String,
    highlight: usize,
}

pub struct Model {
    providers: Vec<Provider>,
    presets: Vec<Preset>,
    catalog: Catalog,
    preset: usize,
    provider: String,
    models: [Option<String>; 4],
    tiers: RoleTiers,
    saved_provider: String,
    base_models: [Option<String>; 4],
    base_tiers: RoleTiers,
    concurrency: u32,
    base_concurrency: u32,
    experimental: [bool; 4],
    base_experimental: [bool; 4],
    retier_pending: bool,
    level: Level,
    menu_focus: usize,
    editing: Option<String>,
    focus: usize,
    open: Option<OpenList>,
    reread_error: Option<String>,
    refusal: Option<String>,
}

impl Model {
    /// The rows before the Experimental ones.
    const TIERS_END: usize = 10;
    const ROWS: [Row; 14] = [
        Row::Preset,
        Row::Provider,
        Row::Model(Letter::O),
        Row::Model(Letter::S),
        Row::Model(Letter::H),
        Row::Model(Letter::G),
        Row::Tier(Role::Auxiliar),
        Row::Tier(Role::Worker),
        Row::Tier(Role::Lead),
        Row::Tier(Role::Manager),
        Row::Experimental(Feature::DurableWorkers),
        Row::Experimental(Feature::WriteCuration),
        Row::Experimental(Feature::FailureTriage),
        Row::Experimental(Feature::TurnQuestionGuard),
    ];

    pub fn new(reads: Reads) -> Model {
        let refs = LETTERS.map(|l| Some(reads.catalog.model(l).model.clone()));
        let provider = provider_of(&reads.catalog.o.model).to_owned();
        Model {
            providers: reads.providers,
            presets: reads.presets,
            preset: 0,
            saved_provider: provider.clone(),
            provider,
            base_models: refs.clone(),
            models: refs,
            base_tiers: reads.tiers,
            tiers: reads.tiers,
            catalog: reads.catalog,
            concurrency: 0,
            base_concurrency: 0,
            experimental: [false; 4],
            base_experimental: [false; 4],
            retier_pending: false,
            level: Level::Tiers,
            menu_focus: 0,
            editing: None,
            focus: 0,
            open: None,
            reread_error: None,
            refusal: None,
        }
    }

    /// The switches as `cpd config` read them: the baseline of the Experimental rows.
    pub fn with_experimental(mut self, on: [bool; 4]) -> Model {
        self.experimental = on;
        self.base_experimental = on;
        self
    }

    /// The count as `cpd config` read it: the current value and the baseline of the Workers row.
    pub fn with_concurrency(mut self, count: u32) -> Model {
        self.concurrency = count;
        self.base_concurrency = count;
        self
    }

    /// The popup opens on the menu (commands.md §13.2); its screens are entered from there.
    pub fn at_menu(mut self) -> Model {
        self.level = Level::Menu;
        self
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn menu_focus(&self) -> usize {
        self.menu_focus
    }

    /// The Workers row's value: the committed count, or the digits typed so far and the cursor while the editor is open.
    pub fn concurrency_value(&self) -> String {
        match &self.editing {
            Some(digits) => format!("{digits}▌"),
            None => self.concurrency.to_string(),
        }
    }

    /// Whether the committed count differs from the baseline.
    pub fn concurrency_changed(&self) -> bool {
        self.concurrency != self.base_concurrency
    }

    /// Opens the menu entry `i` (a click, or Enter on the focused entry).
    pub fn enter(&mut self, i: usize) {
        if i < MENU.len() {
            self.level = if i == 0 { Level::Tiers } else { Level::Workers };
            self.menu_focus = i;
            self.open = None;
        }
    }

    /// Opens the Workers count's editor with nothing typed yet.
    pub fn edit_concurrency(&mut self) {
        if self.level == Level::Workers {
            self.editing = Some(String::new());
        }
    }

    fn menu_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Up => self.menu_focus = self.menu_focus.saturating_sub(1),
            KeyCode::Down => self.menu_focus = (self.menu_focus + 1).min(MENU.len() - 1),
            KeyCode::Enter => self.enter(self.menu_focus),
            KeyCode::Esc | KeyCode::Char('q') => return Effect::Close,
            _ => {}
        }
        Effect::None
    }

    /// A key on the Workers screen: an editor open edits its digits; else Enter opens the editor and Esc or `q` closes.
    fn workers_key(&mut self, key: KeyEvent) -> Effect {
        let Some(digits) = self.editing.clone() else {
            match key.code {
                KeyCode::Enter => self.editing = Some(String::new()),
                KeyCode::Esc | KeyCode::Char('q') => return Effect::Close,
                _ => {}
            }
            return Effect::None;
        };
        match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(d) = self.editing.as_mut() {
                    d.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Some(d) = self.editing.as_mut() {
                    d.pop();
                }
            }
            // an empty editor commits nothing: it closes and the value is unchanged
            KeyCode::Enter => {
                self.editing = None;
                if !digits.is_empty()
                    && let Ok(n) = digits.parse::<u32>()
                {
                    self.concurrency = n;
                }
            }
            KeyCode::Esc => self.editing = None,
            _ => {}
        }
        Effect::None
    }

    /// Flips an Experimental row's switch, and focuses it.
    pub fn flip(&mut self, feature: Feature) {
        self.open = None;
        self.refusal = None;
        self.focus = Self::ROWS
            .iter()
            .position(|r| *r == Row::Experimental(feature))
            .unwrap_or(0);
        self.experimental[feature.at()] = !self.experimental[feature.at()];
    }

    fn id_of(&self, letter: Letter) -> Option<String> {
        let prefix = format!("{}/", self.provider);
        self.models[index(letter)]
            .as_deref()
            .map(|r| r.strip_prefix(&prefix).unwrap_or(r).to_owned())
    }

    fn dimmed(&self, row: Row) -> bool {
        matches!(row, Row::Tier(_)) && self.reread_error.is_some()
    }

    fn tier_valid(&self, role: Role) -> bool {
        self.catalog.is_valid(tier_of(&self.tiers, role))
    }

    /// The current value's text in the lists of `row`.
    fn current(&self, row: Row) -> Option<String> {
        match row {
            Row::Preset => self.presets.get(self.preset).map(|p| p.name.clone()),
            Row::Provider => Some(self.provider.clone()),
            Row::Model(l) => self.id_of(l),
            Row::Tier(r) => Some(tier_of(&self.tiers, r).to_string()),
            Row::Experimental(f) => Some(
                if self.experimental[f.at()] {
                    "on"
                } else {
                    "off"
                }
                .to_owned(),
            ),
        }
    }

    /// The Models and Tiers rows.
    pub fn rows(&self) -> Vec<RowView> {
        self.views(0..Self::TIERS_END)
    }

    /// The Experimental rows, in the order of [`Feature::ALL`].
    pub fn switches(&self) -> Vec<RowView> {
        self.views(Self::TIERS_END..Self::ROWS.len())
    }

    fn views(&self, range: std::ops::Range<usize>) -> Vec<RowView> {
        Self::ROWS[range.clone()]
            .iter()
            .zip(range)
            .map(|(row, at)| (at, row))
            .map(|(at, &row)| {
                let dimmed = self.dimmed(row);
                let (label, range, changed, invalid) = match row {
                    Row::Preset => ("Preset".to_owned(), String::new(), false, false),
                    Row::Provider => (
                        "Provider".to_owned(),
                        String::new(),
                        self.provider != self.saved_provider,
                        false,
                    ),
                    Row::Model(l) => {
                        let name = match l {
                            Letter::O => "opus",
                            Letter::S => "sonnet",
                            Letter::H => "haiku",
                            Letter::G => "gemini",
                        };
                        (
                            format!("{l}  {name}"),
                            String::new(),
                            self.models[index(l)] != self.base_models[index(l)],
                            self.models[index(l)].is_none(),
                        )
                    }
                    Row::Experimental(f) => (
                        f.key().to_owned(),
                        String::new(),
                        self.experimental[f.at()] != self.base_experimental[f.at()],
                        false,
                    ),
                    Row::Tier(r) => {
                        let range = if r == Role::Auxiliar {
                            "default only".to_owned()
                        } else {
                            self.catalog.range(r, &self.tiers).map_or_else(
                                || "no rung".to_owned(),
                                |g| format!("{}..{}", g.floor, g.ceiling),
                            )
                        };
                        (
                            role_name(r).to_owned(),
                            range,
                            tier_of(&self.tiers, r) != tier_of(&self.base_tiers, r),
                            !dimmed && !self.tier_valid(r),
                        )
                    }
                };
                RowView {
                    row,
                    label,
                    value: self.current(row).unwrap_or_else(|| match row {
                        Row::Preset => "(none)".to_owned(),
                        _ => "(unset)".to_owned(),
                    }),
                    range,
                    focused: at == self.focus,
                    open: self.open.as_ref().is_some_and(|o| o.row == row),
                    changed,
                    invalid,
                    dimmed,
                }
            })
            .collect()
    }

    fn filtered(&self, open: &OpenList) -> Vec<String> {
        self.offered(open.row)
            .into_iter()
            .filter(|e| e.contains(&open.filter))
            .collect()
    }

    pub fn list(&self) -> Option<ListView> {
        self.list_for(self.open.as_ref()?)
    }

    /// A key: Up/Down move the focus or the list's highlight, Enter opens the focused row's list or picks, a
    /// character or Backspace edits the filter, Esc closes the list, or else the view; `q` closes without a list.
    pub fn key(&mut self, key: KeyEvent) -> Effect {
        match self.level {
            Level::Menu => return self.menu_key(key),
            Level::Workers => return self.workers_key(key),
            Level::Tiers => {}
        }
        let Some(open) = self.open.clone() else {
            match key.code {
                KeyCode::Up => self.focus = self.focus.saturating_sub(1),
                KeyCode::Down => self.focus = (self.focus + 1).min(Self::ROWS.len() - 1),
                KeyCode::Enter => match Self::ROWS[self.focus] {
                    Row::Experimental(f) => self.flip(f),
                    row => self.open(row),
                },
                KeyCode::Esc | KeyCode::Char('q') => return Effect::Close,
                _ => {}
            }
            return Effect::None;
        };
        let entries = self.filtered(&open);
        let Some(state) = self.open.as_mut() else {
            return Effect::None;
        };
        match key.code {
            KeyCode::Up => state.highlight = state.highlight.saturating_sub(1),
            KeyCode::Down => {
                state.highlight = (state.highlight + 1).min(entries.len().saturating_sub(1));
            }
            KeyCode::Char(c) => {
                state.filter.push(c);
                state.highlight = 0;
            }
            KeyCode::Backspace => {
                state.filter.pop();
                state.highlight = 0;
            }
            KeyCode::Esc => self.open = None,
            KeyCode::Enter => {
                if let Some(entry) = entries.get(open.highlight) {
                    let at = self.offered(open.row).iter().position(|e| e == entry);
                    if let Some(at) = at {
                        return self.pick(open.row, at);
                    }
                }
            }
            _ => {}
        }
        Effect::None
    }

    /// Opens the list of `row`.
    pub fn open(&mut self, row: Row) {
        if self.offered(row).is_empty() {
            return;
        }
        self.focus = Self::ROWS.iter().position(|r| *r == row).unwrap_or(0);
        let mut list = OpenList {
            row,
            filter: String::new(),
            highlight: 0,
        };
        if let Some(l) = self.list_for(&list) {
            list.highlight = l.current.unwrap_or(0);
        }
        self.open = Some(list);
    }

    fn list_for(&self, open: &OpenList) -> Option<ListView> {
        let entries = self.filtered(open);
        let current = self
            .current(open.row)
            .and_then(|c| entries.iter().position(|e| *e == c));
        Some(ListView {
            filter: open.filter.clone(),
            entries,
            highlight: open.highlight,
            current,
        })
    }

    fn tier_offers(&self, role: Role) -> Vec<Tier> {
        let below = ROLES
            .iter()
            .position(|r| *r == role)
            .and_then(|i| i.checked_sub(1))
            .map(|i| tier_of(&self.tiers, ROLES[i]));
        let above = ROLES
            .get(ROLES.iter().position(|r| *r == role).unwrap_or(0) + 1)
            .map(|r| tier_of(&self.tiers, *r));
        self.catalog
            .ladder()
            .into_iter()
            .filter(|t| below.is_none_or(|b| *t > b) && above.is_none_or(|a| *t < a))
            .collect()
    }

    /// The unfiltered entries of `row`'s list; empty when the row is dimmed.
    pub fn offered(&self, row: Row) -> Vec<String> {
        if self.dimmed(row) {
            return Vec::new();
        }
        match row {
            Row::Preset => self.presets.iter().map(|p| p.name.clone()).collect(),
            Row::Provider => self.providers.iter().map(|p| p.provider.clone()).collect(),
            Row::Model(_) => self
                .providers
                .iter()
                .find(|p| p.provider == self.provider)
                .map(|p| p.ids.clone())
                .unwrap_or_default(),
            Row::Tier(r) => self.tier_offers(r).iter().map(Tier::to_string).collect(),
            Row::Experimental(_) => Vec::new(),
        }
    }

    /// Picks entry `index` of `offered(row)`.
    pub fn pick(&mut self, row: Row, at: usize) -> Effect {
        self.open = None;
        let Some(entry) = self.offered(row).get(at).cloned() else {
            return Effect::None;
        };
        self.refusal = None;
        match row {
            Row::Preset => {
                self.preset = at;
                Effect::None
            }
            Row::Provider => {
                let kept = LETTERS.map(|l| {
                    let id = self.id_of(l)?;
                    let ids = &self.providers.iter().find(|p| p.provider == entry)?.ids;
                    ids.contains(&id).then(|| format!("{entry}/{id}"))
                });
                self.models = kept;
                self.provider = entry;
                self.reread()
            }
            Row::Model(l) => {
                self.models[index(l)] = Some(format!("{}/{entry}", self.provider));
                self.reread()
            }
            Row::Experimental(_) => Effect::None,
            Row::Tier(r) => {
                if let Ok(t) = Tier::parse(&entry) {
                    set_tier(&mut self.tiers, r, t);
                }
                Effect::None
            }
        }
    }

    /// A re-read of the four candidates, when all four are set.
    fn reread(&self) -> Effect {
        let refs: Option<Vec<(Letter, String)>> = LETTERS
            .iter()
            .zip(&self.models)
            .map(|(l, m)| m.clone().map(|m| (*l, m)))
            .collect();
        refs.map_or(Effect::None, Effect::Reread)
    }

    /// `[ Load ]`: fills the Provider and the four models from the selected preset; no write.
    pub fn load(&mut self) -> Effect {
        let Some(preset) = self.presets.get(self.preset) else {
            return Effect::None;
        };
        self.provider = provider_of(&preset.models[0]).to_owned();
        self.models = preset.models.clone().map(Some);
        self.refusal = None;
        self.reread()
    }

    /// `[ Save ]`: `Save` when there are steps, `Close` when nothing changed, `None` when blocked.
    pub fn save(&mut self) -> Effect {
        self.refusal = None;
        if self.blocked().is_some() {
            return Effect::None;
        }
        if self.steps().is_empty() {
            Effect::Close
        } else {
            Effect::Save
        }
    }

    pub fn blocked(&self) -> Option<String> {
        if let Some(l) = LETTERS.iter().find(|l| self.models[index(**l)].is_none()) {
            return Some(format!("Model {l} is not set."));
        }
        if let Some(e) = &self.reread_error {
            return Some(e.clone());
        }
        ROLES.iter().find(|r| !self.tier_valid(**r)).map(|r| {
            let letter = tier_of(&self.tiers, *r).letter();
            format!(
                "{} {} is not a rung of {letter}'s model.",
                role_name(*r),
                tier_of(&self.tiers, *r)
            )
        })
    }

    pub fn steps(&self) -> Vec<Step> {
        let mut steps = Vec::new();
        let refs: Option<Vec<String>> = self.models.iter().cloned().collect();
        if let Some(refs) = refs
            && self.models != self.base_models
            && let Ok(four) = <[String; 4]>::try_from(refs)
        {
            steps.push(Step::PresetSet(four));
        }
        if self.tiers != self.base_tiers {
            steps.push(Step::TiersSet(self.tiers));
        }
        // a concurrency change and an Experimental switch change no Tier, so neither needs a retier
        let retier = !steps.is_empty() || self.retier_pending;
        if self.concurrency != self.base_concurrency {
            steps.push(Step::Concurrency(self.concurrency));
        }
        for (i, f) in Feature::ALL.into_iter().enumerate() {
            if self.experimental[i] != self.base_experimental[i] {
                steps.push(Step::Experimental(f, self.experimental[i]));
            }
        }
        if retier {
            steps.push(Step::Retier);
        }
        steps
    }

    pub fn step_done(&mut self, step: &Step) {
        match step {
            Step::PresetSet(refs) => {
                self.base_models = refs.clone().map(Some);
                self.saved_provider = provider_of(&refs[0]).to_owned();
                self.retier_pending = true;
            }
            Step::TiersSet(tiers) => {
                self.base_tiers = *tiers;
                self.retier_pending = true;
            }
            Step::Concurrency(n) => self.base_concurrency = *n,
            Step::Experimental(f, on) => self.base_experimental[f.at()] = *on,
            Step::Retier => self.retier_pending = false,
        }
        self.refusal = None;
    }

    pub fn step_failed(&mut self, _step: &Step, error: &CpdError) {
        self.refusal = Some(error.to_string());
    }

    pub fn reread_done(&mut self, catalog: Catalog) {
        self.catalog = catalog;
        self.reread_error = None;
    }

    pub fn reread_failed(&mut self, error: &CpdError) {
        self.reread_error = Some(error.to_string());
    }

    /// The last refusal as `<code>: <message>`, else the reason Save is blocked, else nothing.
    pub fn footer(&self) -> Option<String> {
        self.refusal.clone().or_else(|| self.blocked())
    }
}
