//! Tiers (contract §9): the syntax, the effort levels, validity against a
//! [`Catalog`], and the ladder.
//!
//! The valid efforts of each model are never kept here: they come from pi's
//! own catalog (see `catalog.rs`) and arrive as a [`Catalog`].

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use crate::error::{CpdError, ErrorCode};

/// A model letter: `G`, `H`aiku, `S`onnet, `O`pus (the `[model]` of a policy key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Letter {
    O,
    S,
    H,
    G,
}

impl Letter {
    /// The ladder's order of models, weakest to strongest: Workers never use `O` (contract §9, hs-d506).
    pub const LADDER: [Letter; 2] = [Letter::H, Letter::S];

    pub fn as_char(self) -> char {
        match self {
            Letter::O => 'O',
            Letter::S => 'S',
            Letter::H => 'H',
            Letter::G => 'G',
        }
    }

    /// The place on the ladder: G < H < S < O.
    fn rank(self) -> u8 {
        match self {
            Letter::G => 0,
            Letter::H => 1,
            Letter::S => 2,
            Letter::O => 3,
        }
    }

    fn from_char(c: char) -> Option<Letter> {
        match c {
            'O' => Some(Letter::O),
            'S' => Some(Letter::S),
            'H' => Some(Letter::H),
            'G' => Some(Letter::G),
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

/// The Tiers a Worker may have (pro-d53, hs-d554), as `details.allowed` and the message give them.
const WORKER_RANGE: &str = "H0-H3, S0-S3";

/// A Tier: a model letter and an effort 0-4 or `X`, `^[GHSO]([0-4]|X)$`, e.g. `S3`, `GX`.
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
        (effort <= Tier::MAX_EFFORT).then_some(Tier {
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

    /// `tier_invalid` (exit 2) when `s` is not `^[GHSO]([0-4]|X)$`.
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
                format!("invalid Tier {s:?}: expected <G|H|S|O><0-4|X>, for example S3."),
            )
            .with("tier", s)
        })
    }

    /// The highest effort any agent may use (hs-d506, contract §9 "Tier rules").
    pub const MAX_EFFORT: u8 = 4;

    /// What a Worker may have: exactly H0-H3 and S0-S3 (pro-d82, hs-d554). `MAX_EFFORT`
    /// stays for Leads, the Manager and the distill ladder.
    pub fn is_worker_allowed(self) -> bool {
        match (self.letter, self.effort) {
            (Letter::H | Letter::S, Some(effort)) => effort <= 3,
            _ => false,
        }
    }

    /// `tier_not_allowed` (exit 2) unless a Worker may have this Tier (pro-d53, hs-d554).
    pub fn check_worker(self) -> Result<(), CpdError> {
        if self.is_worker_allowed() {
            return Ok(());
        }
        Err(CpdError::new(
            ErrorCode::TierNotAllowed,
            format!(
                "Tier {self} is not allowed for a Worker: Workers use only the Tiers {WORKER_RANGE}, to keep cost down (pro-d82, hs-d554)."
            ),
        )
        .with("tier", self.to_string())
        .with("allowed", WORKER_RANGE)
        .with("rule", "pro-d82"))
    }

    /// What a Lead may have: any letter but G, effort 0-4 (hs-d506, hs-d562).
    pub fn is_lead_allowed(self) -> bool {
        self.letter != Letter::G
    }

    /// `tier_not_allowed` (exit 2) unless a Lead may have this Tier (hs-d506, hs-d562).
    pub fn check_lead(self) -> Result<(), CpdError> {
        if self.is_lead_allowed() {
            return Ok(());
        }
        Err(CpdError::new(
            ErrorCode::TierNotAllowed,
            format!(
                "Tier {self} is not allowed for a Lead: Leads use only the Tiers O0-O4, S0-S4, H0-H4, never effort 5 and never G (hs-d506, hs-d562)."
            ),
        )
        .with("tier", self.to_string())
        .with("allowed", "O0-O4, S0-S4, H0-H4")
        .with("rule", "hs-d506, hs-d562"))
    }

    pub fn letter(self) -> Letter {
        self.letter
    }

    /// The effort digit; 0 for `X` (see [`Tier::is_x`]).
    pub fn effort(self) -> u8 {
        self.effort.unwrap_or(0)
    }

    /// pi's thinking level for this effort: `off`, `low`, `medium`, `high`, `xhigh`; `X` is `off`.
    pub fn level(self) -> &'static str {
        LEVELS[self.effort() as usize]
    }
}

/// The ladder's order (Tier model v2): letters G < H < S < O, then effort ascending, `X` lowest of its letter.
impl Ord for Tier {
    fn cmp(&self, other: &Tier) -> std::cmp::Ordering {
        (self.letter.rank(), self.effort).cmp(&(other.letter.rank(), other.effort))
    }
}

impl PartialOrd for Tier {
    fn partial_cmp(&self, other: &Tier) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// An agent role, ascending: auxiliar < worker < lead < manager (Tier model v2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Auxiliar,
    Worker,
    Lead,
    Manager,
}

/// The role defaults: `context_distill_tier`, `worker_tier`, `lead_tier`, `manager_tier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleTiers {
    pub auxiliar: Tier,
    pub worker: Tier,
    pub lead: Tier,
    pub manager: Tier,
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

/// One Tier letter's model and the efforts pi supports for it, ascending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TierModel {
    /// `<provider>/<model-id>`, as configured in `tiers.<letter>`.
    pub model: String,
    pub efforts: Vec<u8>,
}

/// The four Tier letters' models with their valid efforts, read from pi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub pi_version: String,
    pub o: TierModel,
    pub s: TierModel,
    pub h: TierModel,
    pub g: TierModel,
}

impl Catalog {
    pub fn model(&self, letter: Letter) -> &TierModel {
        match letter {
            Letter::O => &self.o,
            Letter::S => &self.s,
            Letter::H => &self.h,
            Letter::G => &self.g,
        }
    }

    /// Whether `tier`'s effort is valid for its model.
    pub fn is_valid(&self, tier: Tier) -> bool {
        let efforts = &self.model(tier.letter).efforts;
        match tier.effort {
            None => efforts.is_empty(),
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

    /// pi's `--model` argument for `tier`: `<provider>/<model-id>:<level>`.
    pub fn model_arg(&self, tier: Tier) -> Result<String, CpdError> {
        self.check(tier)?;
        Ok(format!(
            "{}:{}",
            self.model(tier.letter).model,
            tier.level()
        ))
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
    /// Floor: the lowest rung at or above the role's default; ceiling: the highest rung strictly below the
    /// next role's default (the Manager: the top rung). The auxiliar is its default only.
    pub fn range(&self, role: Role, defaults: &RoleTiers) -> Option<Range> {
        if role == Role::Auxiliar {
            let default = defaults.auxiliar;
            return self.is_valid(default).then_some(Range {
                floor: default,
                ceiling: default,
            });
        }
        let (own, above) = match role {
            Role::Auxiliar => unreachable!("handled above"),
            Role::Worker => (defaults.worker, Some(defaults.lead)),
            Role::Lead => (defaults.lead, Some(defaults.manager)),
            Role::Manager => (defaults.manager, None),
        };
        let ladder = self.ladder();
        let floor = ladder.iter().copied().find(|r| *r >= own)?;
        let ceiling = ladder
            .iter()
            .copied()
            .rfind(|r| above.is_none_or(|a| *r < a))?;
        (floor <= ceiling).then_some(Range { floor, ceiling })
    }

    /// The next valid rung inside `range`; leaving a letter lands on its lowest effort >= 1 (or X); `None` at the ceiling.
    pub fn next_in(&self, range: Range, tier: Tier) -> Option<Tier> {
        self.ladder()
            .into_iter()
            .filter(|r| range.contains(*r) && *r > tier)
            .find(|r| r.letter == tier.letter || r.effort != Some(0))
    }

    /// The lowest valid rung of `range` at or above `tier`.
    pub fn step_up_in(&self, range: Range, tier: Tier) -> Option<Tier> {
        self.ladder()
            .into_iter()
            .find(|r| range.contains(*r) && *r >= tier)
    }

    /// The ladder's next step (contract §9, pro-d82, pro-d83, hs-d554, hs-d791): within the same
    /// letter, the smallest valid effort a Worker may have (H0-H3, S0-S3) strictly greater than
    /// `tier`'s — not effort + 1, which may not be a valid effort of the model; only when none
    /// exists, leaving the letter, the lowest valid allowed S effort ≥ 1; otherwise none. It never
    /// enters `O`; the ladder ends at S3. A Tier a Worker may not have (effort 5, S4, `O`, `GX`)
    /// has no ladder.
    pub fn next(&self, tier: Tier) -> Result<Option<Tier>, CpdError> {
        self.check(tier)?;
        if !tier.is_worker_allowed() {
            return Ok(None);
        }
        let allowed = |t: &Tier| t.is_worker_allowed();
        // Within the letter: the smallest valid effort strictly above the Tier's (hs-d791).
        if let Some(up) = self
            .model(tier.letter)
            .efforts
            .iter()
            .copied()
            .find(|e| *e > tier.effort())
            .and_then(|effort| Tier::new(tier.letter, effort))
            .filter(|t| allowed(t) && self.is_valid(*t))
        {
            return Ok(Some(up));
        }
        let at = Letter::LADDER
            .iter()
            .position(|l| *l == tier.letter)
            .expect("an allowed letter is on the ladder");
        for letter in &Letter::LADDER[at + 1..] {
            let lowest = self
                .model(*letter)
                .efforts
                .iter()
                .copied()
                .find(|e| *e >= 1);
            if let Some(next) = lowest
                .and_then(|effort| Tier::new(*letter, effort))
                .filter(allowed)
            {
                return Ok(Some(next));
            }
        }
        Ok(None)
    }

    /// The Tier a Worker takes when the catalog refuses `tier` (hs-n465,
    /// hs-d632): `None` when a Worker may not have `tier` (hs-d554);
    /// otherwise the lowest valid, Worker-allowed Tier at or above `tier`'s
    /// effort, its own letter first and then `H → S` (the Tier itself when the
    /// catalog accepts it).
    pub fn step_up(&self, tier: Tier) -> Option<Tier> {
        if !tier.is_worker_allowed() {
            return None;
        }
        let allowed = |t: &Tier| t.is_worker_allowed() && self.is_valid(*t);
        // Same letter first: its lowest valid effort at or above the Tier's.
        if let Some(same) = self
            .model(tier.letter)
            .efforts
            .iter()
            .copied()
            .find(|e| *e >= tier.effort())
            .and_then(|effort| Tier::new(tier.letter, effort))
            .filter(allowed)
        {
            return Some(same);
        }
        // Then `H → S`, at the lowest valid effort at or above the Tier's.
        let at = Letter::LADDER
            .iter()
            .position(|l| *l == tier.letter)
            .expect("an allowed letter is on the ladder");
        for letter in &Letter::LADDER[at + 1..] {
            let lowest = self
                .model(*letter)
                .efforts
                .iter()
                .copied()
                .find(|e| *e >= 1);
            if let Some(next) = lowest
                .and_then(|effort| Tier::new(*letter, effort))
                .filter(allowed)
            {
                return Some(next);
            }
        }
        None
    }

    /// The Tier of automatic Attempt `n` (from 1): `next` applied `n - 1`
    /// times from the Job's Tier; `None` when the ladder runs out first.
    pub fn attempt_tier(&self, job_tier: Tier, n: u32) -> Result<Option<Tier>, CpdError> {
        assert!(n >= 1, "Attempt numbers start at 1");
        self.check(job_tier)?;
        let mut tier = Some(job_tier);
        for _ in 1..n {
            tier = match tier {
                Some(t) => self.next(t)?,
                None => return Ok(None),
            };
        }
        Ok(tier)
    }
}
