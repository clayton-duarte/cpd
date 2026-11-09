//! Tiers (contract §9): the syntax, the effort levels, validity against a
//! [`Catalog`], and the ladder.
//!
//! The valid efforts of each model are never kept here: they come from pi's
//! own catalog (see `catalog.rs`) and arrive as a [`Catalog`].

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use crate::error::{CpdError, ErrorCode};

/// A model letter: `J`ev-router, `G`, `H`aiku, `S`onnet, `O`pus (the `[model]` of a policy key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Letter {
    O,
    S,
    H,
    G,
    J,
}

impl Letter {
    /// The letter an explore Job keeps: H for a `scout`, S for `research`; `None` for every other kind.
    pub fn of_explore_kind(kind: &str) -> Option<Letter> {
        match kind {
            "scout" => Some(Letter::H),
            "research" => Some(Letter::S),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Letter::O => 'O',
            Letter::S => 'S',
            Letter::H => 'H',
            Letter::G => 'G',
            Letter::J => 'J',
        }
    }

    /// The place on the ladder: `J < G < H < S < O` (contract §9 "Ladder order").
    fn rank(self) -> u8 {
        match self {
            Letter::J => 0,
            Letter::G => 1,
            Letter::H => 2,
            Letter::S => 3,
            Letter::O => 4,
        }
    }

    fn from_char(c: char) -> Option<Letter> {
        match c {
            'O' => Some(Letter::O),
            'S' => Some(Letter::S),
            'H' => Some(Letter::H),
            'G' => Some(Letter::G),
            'J' => Some(Letter::J),
            _ => None,
        }
    }
}

impl fmt::Display for Letter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// pi's thinking levels by effort digit; `minimal` and `max` have no digit (contract §9).
const LEVELS: [&str; 5] = ["off", "low", "medium", "high", "xhigh"];

/// The effort digit of one of pi's thinking-level names. `Some(None)` is
/// `minimal` or `max`, which have no digit; `None` is a name pi does not have.
pub(super) fn effort_of_level(level: &str) -> Option<Option<u8>> {
    if level == "minimal" || level == "max" {
        return Some(None);
    }
    LEVELS
        .iter()
        .position(|l| *l == level)
        .map(|i| Some(i as u8))
}

/// The highest effort digit; effort 5 is valid for no letter.
const TOP_EFFORT: u8 = 4;

/// A Tier: a model letter and an effort 0-4 or `X`, `^[JGHSO]([0-4]|X)$`, e.g. `S3`, `GX`.
/// `X` (no effort) is distinct from effort 0; it is the one Tier of a model with no effort levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tier {
    letter: Letter,
    /// `None` is `X`.
    effort: Option<u8>,
}

impl Tier {
    /// `None` when `effort` is above 4.
    pub fn new(letter: Letter, effort: u8) -> Option<Tier> {
        (effort <= TOP_EFFORT).then_some(Tier {
            letter,
            effort: Some(effort),
        })
    }

    /// The `X` Tier of `letter`.
    pub fn x(letter: Letter) -> Tier {
        Tier {
            letter,
            effort: None,
        }
    }

    /// Whether this is an `X` Tier (no effort).
    pub fn is_x(self) -> bool {
        self.effort.is_none()
    }

    /// `tier_invalid` (exit 2) when `s` is not `^[JGHSO]([0-4]|X)$`.
    pub fn parse(s: &str) -> Result<Tier, CpdError> {
        let mut chars = s.chars();
        let tier = (|| {
            let letter = Letter::from_char(chars.next()?)?;
            let tier = match chars.next()? {
                'X' => Tier::x(letter),
                c => Tier::new(letter, c.to_digit(10)? as u8)?,
            };
            chars.next().is_none().then_some(tier)
        })();
        tier.ok_or_else(|| {
            CpdError::new(
                ErrorCode::TierInvalid,
                format!("invalid Tier {s:?}: expected <J|G|H|S|O><0-4|X>, for example S3."),
            )
            .with("tier", s)
        })
    }

    pub fn letter(self) -> Letter {
        self.letter
    }

    /// The level name of the effort digit; `X` reads as `off`. To tell pi a level use [`Tier::thinking`].
    pub fn level(self) -> &'static str {
        LEVELS[self.effort.unwrap_or(0) as usize]
    }

    /// The thinking level to send pi: `off`, `low`, `medium`, `high`, `xhigh`; `None` for `X` (send no level).
    pub fn thinking(self) -> Option<&'static str> {
        self.effort.map(|e| LEVELS[e as usize])
    }

    /// The place within the letter: 0 < 1 < X < 2 < 3 < 4.
    fn effort_rank(self) -> u8 {
        self.effort.map_or(2, |e| if e < 2 { e } else { e + 1 })
    }
}

/// The ladder's order: letters J < G < H < S < O, then effort ascending, `X` between 1 and 2.
impl Ord for Tier {
    fn cmp(&self, other: &Tier) -> std::cmp::Ordering {
        (self.letter.rank(), self.effort_rank()).cmp(&(other.letter.rank(), other.effort_rank()))
    }
}

impl PartialOrd for Tier {
    fn partial_cmp(&self, other: &Tier) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// An agent role, ascending: decide < auxiliar < worker < lead < manager (Tier model v2; the decide role air-d113, air-d115, air-d120).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Decide,
    Auxiliar,
    Worker,
    Lead,
    Manager,
}

impl Role {
    /// The role's name, as an error and `details.role` give it.
    pub fn name(self) -> &'static str {
        match self {
            Role::Decide => "decide",
            Role::Auxiliar => "auxiliar",
            Role::Worker => "worker",
            Role::Lead => "lead",
            Role::Manager => "manager",
        }
    }

    /// The config key of the role's default, which sets the floor of its range.
    pub fn key(self) -> &'static str {
        match self {
            Role::Decide => "decide_tier",
            Role::Auxiliar => "context_distill_tier",
            Role::Worker => "worker_tier",
            Role::Lead => "lead_tier",
            Role::Manager => "manager_tier",
        }
    }

    /// The role above, whose default bounds the ceiling; `None` for the manager.
    fn above(self) -> Option<Role> {
        match self {
            Role::Decide => Some(Role::Auxiliar),
            Role::Auxiliar => Some(Role::Worker),
            Role::Worker => Some(Role::Lead),
            Role::Lead => Some(Role::Manager),
            Role::Manager => None,
        }
    }
}

/// The role defaults: `decide_tier`, `context_distill_tier`, `worker_tier`, `lead_tier`, `manager_tier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleTiers {
    pub decide: Tier,
    pub auxiliar: Tier,
    pub worker: Tier,
    pub lead: Tier,
    pub manager: Tier,
}

impl RoleTiers {
    /// The range of `role` in `catalog`. `config_invalid` (naming the role's key) when no valid rung lies in its span.
    pub fn range(&self, catalog: &Catalog, role: Role) -> Result<Range, CpdError> {
        catalog.range(role, self).ok_or_else(|| {
            CpdError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "{} has no valid Tier in its span: the {} range holds no rung of the catalog.",
                    role.key(),
                    role.name()
                ),
            )
            .with("key", role.key())
        })
    }

    /// `tier_not_allowed` (exit 2) unless `tier` is in the range of a Job of `kind` ([`RoleTiers::job_range`]).
    pub fn check_job(&self, catalog: &Catalog, kind: &str, tier: Tier) -> Result<(), CpdError> {
        let worker = self.range(catalog, Role::Worker)?;
        let Some(letter) = Letter::of_explore_kind(kind) else {
            return worker.check(tier, Role::Worker);
        };
        let range = catalog.explore_range(letter, worker);
        if range.is_some_and(|r| r.contains(tier)) {
            return Ok(());
        }
        let allowed = range.map_or_else(
            || format!("no {letter} Tier up to {}", worker.ceiling),
            |r| r.allowed(),
        );
        Err(CpdError::new(
            ErrorCode::TierNotAllowed,
            format!(
                "Tier {tier} is not allowed for a {kind} Job: it keeps letter {letter}, effort 1 or more or X, up to the worker ceiling {} ({allowed}); {} and {} set the worker range.",
                worker.ceiling,
                Role::Worker.key(),
                Role::Lead.key()
            ),
        )
        .with("tier", tier.to_string())
        .with("kind", kind)
        .with("allowed", allowed)
        .with("ceiling", worker.ceiling.to_string())
        .with("key", Role::Worker.key()))
    }

    /// The range of a Job of `kind`: the worker range; for a `scout` or `research` Job the range inside its letter
    /// (`None` when no rung is left).
    pub fn job_range(&self, catalog: &Catalog, kind: &str) -> Result<Option<Range>, CpdError> {
        let worker = self.range(catalog, Role::Worker)?;
        Ok(match Letter::of_explore_kind(kind) {
            None => Some(worker),
            Some(letter) => catalog.explore_range(letter, worker),
        })
    }
}

/// The Tiers a role may have: `floor` to `ceiling`, both valid rungs of the ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub floor: Tier,
    pub ceiling: Tier,
}

impl Range {
    /// Whether `tier` lies on the ladder from `floor` to `ceiling`, both included.
    pub fn contains(&self, tier: Tier) -> bool {
        self.floor <= tier && tier <= self.ceiling
    }

    /// The range as `details.allowed` gives it: `<floor>-<ceiling>`.
    pub fn allowed(&self) -> String {
        format!("{}-{}", self.floor, self.ceiling)
    }

    /// `tier_not_allowed` (exit 2) unless `tier` is in this range of `role`. The message names the range and the
    /// config keys that set it; `details.key` is the key of the floor.
    pub fn check(&self, tier: Tier, role: Role) -> Result<(), CpdError> {
        if self.contains(tier) {
            return Ok(());
        }
        let allowed = self.allowed();
        let keys = match role.above() {
            Some(above) if role != Role::Auxiliar => format!(
                "{} sets its floor and {} (the next role's default) bounds its ceiling",
                role.key(),
                above.key()
            ),
            _ => format!("{} sets it", role.key()),
        };
        Err(CpdError::new(
            ErrorCode::TierNotAllowed,
            format!(
                "Tier {tier} is not allowed for a {}: its Tiers are {allowed}; {keys}.",
                role.name()
            ),
        )
        .with("tier", tier.to_string())
        .with("role", role.name())
        .with("floor", self.floor.to_string())
        .with("ceiling", self.ceiling.to_string())
        .with("allowed", allowed)
        .with("key", role.key()))
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.effort {
            None => write!(f, "{}X", self.letter),
            Some(effort) => write!(f, "{}{effort}", self.letter),
        }
    }
}

impl FromStr for Tier {
    type Err = CpdError;

    fn from_str(s: &str) -> Result<Tier, CpdError> {
        Tier::parse(s)
    }
}

impl Serialize for Tier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// `model` with `:<level>`; `model` alone when there is no level (`X`).
pub fn with_level(model: &str, level: Option<&str>) -> String {
    match level {
        Some(level) => format!("{model}:{level}"),
        None => model.to_string(),
    }
}

/// One Tier letter's model and the efforts pi supports for it, ascending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TierModel {
    /// `<provider>/<model-id>`, as configured in `tiers.<letter>`.
    pub model: String,
    pub efforts: Vec<u8>,
}

/// The five Tier letters' models with their valid efforts, read from pi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub pi_version: String,
    pub o: TierModel,
    pub s: TierModel,
    pub h: TierModel,
    pub g: TierModel,
    pub j: TierModel,
}

impl Catalog {
    pub fn model(&self, letter: Letter) -> &TierModel {
        match letter {
            Letter::O => &self.o,
            Letter::S => &self.s,
            Letter::H => &self.h,
            Letter::G => &self.g,
            Letter::J => &self.j,
        }
    }

    /// Whether `tier`'s effort is valid for its model.
    pub fn is_valid(&self, tier: Tier) -> bool {
        let efforts = &self.model(tier.letter).efforts;
        match tier.effort {
            None => true,
            Some(effort) => efforts.contains(&effort),
        }
    }

    /// `tier_invalid` (exit 2) when `tier`'s effort is not valid for its model.
    pub fn check(&self, tier: Tier) -> Result<(), CpdError> {
        if self.is_valid(tier) {
            return Ok(());
        }
        let model = self.model(tier.letter);
        let efforts: Vec<String> = model.efforts.iter().map(u8::to_string).collect();
        Err(CpdError::new(
            ErrorCode::TierInvalid,
            format!(
                "Tier {tier} is not valid: {} supports efforts {}.",
                model.model,
                efforts.join(",")
            ),
        )
        .with("tier", tier.to_string())
        .with("model", model.model.clone())
        .with("efforts", model.efforts.clone()))
    }

    /// pi's `--model` argument for `tier`: `<provider>/<model-id>:<level>`, with no `:<level>` for `X`.
    pub fn model_arg(&self, tier: Tier) -> Result<String, CpdError> {
        self.check(tier)?;
        Ok(with_level(&self.model(tier.letter).model, tier.thinking()))
    }

    /// Every rung a range is made of: the ladder plus the `X` of every letter, ascending.
    fn rungs(&self) -> Vec<Tier> {
        let mut rungs = self.ladder();
        rungs.extend([Letter::G, Letter::H, Letter::S, Letter::O].map(Tier::x));
        rungs.sort();
        rungs.dedup();
        rungs
    }

    /// Every valid rung, ascending (effort-0 rungs included; `X` is the one rung of a model without levels).
    pub fn ladder(&self) -> Vec<Tier> {
        [Letter::G, Letter::H, Letter::S, Letter::O]
            .into_iter()
            .flat_map(|letter| {
                let efforts = &self.model(letter).efforts;
                if efforts.is_empty() {
                    vec![Tier::x(letter)]
                } else {
                    let mut tiers: Vec<Tier> = efforts
                        .iter()
                        .filter_map(|e| Tier::new(letter, *e))
                        .collect();
                    tiers.sort();
                    tiers.dedup();
                    tiers
                }
            })
            .collect()
    }

    /// The range of `role` under the role defaults; `None` when no valid rung lies in its span.
    /// Floor: the lowest rung at or above the role's default; ceiling: the highest rung at or below the
    /// next role's default (the Manager: the top rung). The decide and the auxiliar are their default only
    /// (one rung, no ladder).
    pub fn range(&self, role: Role, defaults: &RoleTiers) -> Option<Range> {
        if role == Role::Decide || role == Role::Auxiliar {
            let default = match role {
                Role::Decide => defaults.decide,
                Role::Auxiliar => defaults.auxiliar,
                _ => unreachable!("handled above"),
            };
            return self.is_valid(default).then_some(Range {
                floor: default,
                ceiling: default,
            });
        }
        let (own, above) = match role {
            Role::Decide => unreachable!("handled above"),
            Role::Auxiliar => unreachable!("handled above"),
            Role::Worker => (defaults.worker, Some(defaults.lead)),
            Role::Lead => (defaults.lead, Some(defaults.manager)),
            Role::Manager => (defaults.manager, None),
        };
        let ladder = self.rungs();
        let floor = ladder.iter().copied().find(|r| *r >= own)?;
        let ceiling = ladder
            .iter()
            .copied()
            .rfind(|r| above.is_none_or(|a| *r <= a))?;
        (floor <= ceiling).then_some(Range { floor, ceiling })
    }

    /// The range of an explore Job of `letter` inside the worker range `worker`: the letter's lowest valid effort >= 1
    /// (or `X`) up to the worker ceiling (or the letter's top rung); `None` when no rung is left.
    pub fn explore_range(&self, letter: Letter, worker: Range) -> Option<Range> {
        let rungs: Vec<Tier> = self
            .rungs()
            .into_iter()
            .filter(|r| r.letter == letter && r.effort != Some(0) && *r <= worker.ceiling)
            .collect();
        Some(Range {
            floor: *rungs.first()?,
            ceiling: *rungs.last()?,
        })
    }

    /// The next valid rung inside `range`, never effort 0. Within a letter `X` is a rung; when entering a new
    /// letter with efforts, enter at its lowest effort of at least 1 instead of its `X`.
    pub fn next_in(&self, range: Range, tier: Tier) -> Option<Tier> {
        self.rungs().into_iter().find(|r| {
            range.contains(*r)
                && *r > tier
                && r.effort != Some(0)
                && (r.letter == tier.letter || !r.is_x() || self.model(r.letter).efforts.is_empty())
        })
    }

    /// The lowest valid rung of `range` at or above `tier`.
    pub fn step_up_in(&self, range: Range, tier: Tier) -> Option<Tier> {
        self.rungs()
            .into_iter()
            .find(|r| range.contains(*r) && *r >= tier)
    }
}
