//! Tiers (contract §9): the syntax, the effort levels, validity against a
//! [`Catalog`], and the ladder.
//!
//! The valid efforts of each model are never kept here: they come from pi's
//! own catalog (see `catalog.rs`) and arrive as a [`Catalog`].

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use crate::error::{CpdError, ErrorCode};

/// A model letter: `O`pus, `S`onnet, `H`aiku (the `[model]` of a policy key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Letter {
    O,
    S,
    H,
    /// Auxiliary agents only, written `GX` (hs-d562).
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

/// pi's thinking levels by effort digit; `minimal` is never used (contract §9).
const LEVELS: [&str; 6] = ["off", "low", "medium", "high", "xhigh", "max"];

/// The effort digit of one of pi's thinking-level names. `Some(None)` is
/// `minimal`, which has no digit; `None` is a name pi does not have.
pub(super) fn effort_of_level(level: &str) -> Option<Option<u8>> {
    if level == "minimal" {
        return Some(None);
    }
    LEVELS.iter().position(|l| *l == level).map(|i| Some(i as u8))
}

/// The Tiers a Worker may have (pro-d53, hs-d554), as `details.allowed` and the message give them.
const WORKER_RANGE: &str = "H0-H4, S0-S3";

/// A Tier: a model letter and an effort 0–5, `^(O|S|H)([0-5])$`, e.g. `S3`, or exactly `GX` (hs-d562).
/// A G Tier keeps effort 0 inside and is written `GX`: no value displays as `G0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tier {
    letter: Letter,
    effort: u8,
}

impl Tier {
    /// `None` when `effort` is above 5.
    pub fn new(letter: Letter, effort: u8) -> Option<Tier> {
        let effort = if letter == Letter::G { 0 } else { effort };
        (effort <= 5).then_some(Tier { letter, effort })
    }

    /// `tier_invalid` (exit 2) when `s` is not `^(O|S|H)([0-5])$` or `GX`.
    pub fn parse(s: &str) -> Result<Tier, CpdError> {
        if s == "GX" {
            return Ok(Tier { letter: Letter::G, effort: 0 });
        }
        let mut peek = s.chars();
        if peek.next() == Some('G') && peek.next().is_some_and(|c| c.is_ascii_digit()) && peek.next().is_none() {
            return Err(CpdError::new(
                ErrorCode::TierInvalid,
                format!("invalid Tier {s:?}: G has no effort, write GX (hs-d562)."),
            )
            .with("tier", s));
        }
        let mut chars = s.chars();
        let tier = (|| {
            let letter = Letter::from_char(chars.next()?)?;
            let effort = chars.next()?.to_digit(10)? as u8;
            if letter == Letter::G {
                return None;
            }
            if chars.next().is_some() {
                return None;
            }
            Tier::new(letter, effort)
        })();
        tier.ok_or_else(|| {
            CpdError::new(
                ErrorCode::TierInvalid,
                format!("invalid Tier {s:?}: expected <O|S|H><0-5> or GX, for example S3."),
            )
            .with("tier", s)
        })
    }

    /// The highest effort any agent may use (hs-d506, contract §9 "Tier rules").
    pub const MAX_EFFORT: u8 = 4;

    /// What a Worker may have: exactly H0-H4 and S0-S3 (pro-d53, hs-d554). `MAX_EFFORT`
    /// stays for Leads, the Manager and the distill ladder.
    pub fn is_worker_allowed(self) -> bool {
        match self.letter {
            Letter::H => self.effort <= 4,
            Letter::S => self.effort <= 3,
            Letter::O | Letter::G => false,
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
                "Tier {self} is not allowed for a Worker: Workers use only the Tiers {WORKER_RANGE}, to keep cost down (pro-d53, hs-d554)."
            ),
        )
        .with("tier", self.to_string())
        .with("allowed", WORKER_RANGE)
        .with("rule", "pro-d53"))
    }

    pub fn letter(self) -> Letter {
        self.letter
    }

    pub fn effort(self) -> u8 {
        self.effort
    }

    /// pi's thinking level for this effort: `off`, `low`, `medium`, `high`, `xhigh`, `max`.
    pub fn level(self) -> &'static str {
        LEVELS[self.effort as usize]
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.letter {
            Letter::G => f.write_str("GX"),
            letter => write!(f, "{letter}{}", self.effort),
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

/// The three Tier letters' models with their valid efforts, read from pi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub pi_version: String,
    pub o: TierModel,
    pub s: TierModel,
    pub h: TierModel,
    /// Never cached; `efforts` is empty: G has no effort levels, `GX` is its one Tier (hs-d562).
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
        tier.letter == Letter::G || self.model(tier.letter).efforts.contains(&tier.effort)
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
            format!("Tier {tier} is not valid: {} supports efforts {}.", model.model, efforts.join(",")),
        )
        .with("tier", tier.to_string())
        .with("model", model.model.clone())
        .with("efforts", model.efforts.clone()))
    }

    /// pi's `--model` argument for `tier`: `<provider>/<model-id>:<level>`.
    pub fn model_arg(&self, tier: Tier) -> Result<String, CpdError> {
        self.check(tier)?;
        Ok(format!("{}:{}", self.model(tier.letter).model, tier.level()))
    }

    /// The ladder's next step (contract §9, pro-d53, hs-d554): the same model at effort
    /// + 1 if valid and a Worker may have it (H0-H4, S0-S3); otherwise the next model in
    /// `H → S` at the lowest valid effort ≥ the current one that is allowed;
    /// otherwise none. It never enters `O`; the ladder ends at S3, or at H4 on a catalog that has it. A Tier
    /// a Worker may not have (effort 5, S4, `O`, `GX`) has no ladder.
    pub fn next(&self, tier: Tier) -> Result<Option<Tier>, CpdError> {
        self.check(tier)?;
        if !tier.is_worker_allowed() {
            return Ok(None);
        }
        let allowed = |t: &Tier| t.is_worker_allowed();
        if let Some(up) = Tier::new(tier.letter, tier.effort + 1).filter(|t| allowed(t) && self.is_valid(*t)) {
            return Ok(Some(up));
        }
        let at = Letter::LADDER.iter().position(|l| *l == tier.letter).expect("an allowed letter is on the ladder");
        for letter in &Letter::LADDER[at + 1..] {
            let lowest = self.model(*letter).efforts.iter().copied().find(|e| *e >= tier.effort);
            if let Some(next) = lowest.and_then(|effort| Tier::new(*letter, effort)).filter(allowed) {
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
            .find(|e| *e >= tier.effort)
            .and_then(|effort| Tier::new(tier.letter, effort))
            .filter(allowed)
        {
            return Some(same);
        }
        // Then `H → S`, at the lowest valid effort at or above the Tier's.
        let at = Letter::LADDER.iter().position(|l| *l == tier.letter).expect("an allowed letter is on the ladder");
        for letter in &Letter::LADDER[at + 1..] {
            let lowest = self.model(*letter).efforts.iter().copied().find(|e| *e >= tier.effort);
            if let Some(next) = lowest.and_then(|effort| Tier::new(*letter, effort)).filter(allowed) {
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
