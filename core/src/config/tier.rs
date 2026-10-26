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

/// The Tiers a Worker may have (hs-d554), as `details.allowed` and the message give them.
const WORKER_RANGE: &str = "H3, S0-S3";

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

    /// What a Worker may have: exactly `H3` and `S0`-`S3` (hs-d554). `MAX_EFFORT`
    /// stays for Leads, the Manager and the distill ladder.
    pub fn is_worker_allowed(self) -> bool {
        match self.letter {
            Letter::H => self.effort == 3,
            Letter::S => self.effort <= 3,
            Letter::O | Letter::G => false,
        }
    }

    /// `tier_not_allowed` (exit 2) unless a Worker may have this Tier (hs-d554).
    pub fn check_worker(self) -> Result<(), CpdError> {
        if self.is_worker_allowed() {
            return Ok(());
        }
        Err(CpdError::new(
            ErrorCode::TierNotAllowed,
            format!(
                "Tier {self} is not allowed for a Worker: Workers use only the Tiers H3, S0-S3, to keep cost down (hs-d554)."
            ),
        )
        .with("tier", self.to_string())
        .with("allowed", WORKER_RANGE)
        .with("rule", "hs-d554"))
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

    /// The ladder's next step (contract §9, hs-d554): the same model at effort
    /// + 1 if valid and a Worker may have it (H3, S0-S3); otherwise the next model in
    /// `H → S` at the lowest valid effort ≥ the current one that is allowed;
    /// otherwise none. It never enters `O`; the ladder ends at S3. A Tier
    /// a Worker may not have (H0-H2, S4, `O`) has no ladder.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, efforts: &[u8]) -> TierModel {
        TierModel { model: format!("p/{name}"), efforts: efforts.to_vec() }
    }

    /// O supports 0–5, S 1–5, H 0–3.
    fn catalog() -> Catalog {
        Catalog {
            pi_version: "1.2.0".into(),
            o: model("opus", &[0, 1, 2, 3, 4, 5]),
            s: model("sonnet", &[1, 2, 3, 4, 5]),
            h: model("haiku", &[0, 1, 2, 3]),
            g: model("gemini", &[]),
        }
    }

    fn t(s: &str) -> Tier {
        Tier::parse(s).unwrap()
    }

    fn next(from: &str) -> Option<String> {
        catalog().next(t(from)).unwrap().map(|t| t.to_string())
    }

    #[test]
    fn syntax() {
        for good in ["O0", "S3", "H5", "GX"] {
            assert_eq!(t(good).to_string(), good);
        }
        for bad in ["", "O", "O6", "o3", "X1", "S33", "S-1", " S3", "S3 ", "minimal", "G0", "gx"] {
            let err = Tier::parse(bad).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{bad:?}");
        }
    }

    #[test]
    fn levels_never_use_minimal() {
        let levels: Vec<_> = (0..=5).map(|e| Tier::new(Letter::O, e).unwrap().level()).collect();
        assert_eq!(levels, ["off", "low", "medium", "high", "xhigh", "max"]);
        assert_eq!(effort_of_level("minimal"), Some(None));
        assert_eq!(effort_of_level("xhigh"), Some(Some(4)));
        assert_eq!(effort_of_level("bogus"), None);
    }

    #[test]
    fn the_contract_examples() {
        assert_eq!(next("S2").as_deref(), Some("S3"));
        assert_eq!(next("H3").as_deref(), Some("S3"));
        // hs-d554: the ladder ends at S3.
        assert_eq!(next("S3"), None);
        assert_eq!(next("S4"), None);
    }

    #[test]
    fn the_next_model_starts_at_its_lowest_valid_effort_at_or_above() {
        let mut c = catalog();
        c.s = model("sonnet", &[2, 3, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S3")));
        c.s = model("sonnet", &[2, 4, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), None, "S4 is not allowed (hs-d554)");
        c.s = model("sonnet", &[0, 1, 2]);
        assert_eq!(c.next(t("H3")).unwrap(), None);
        c.s = model("sonnet", &[0, 1, 2, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), None, "S5 is not allowed (hs-d554)");
    }

    #[test]
    fn an_invalid_tier_is_refused_not_stepped() {
        let err = catalog().next(t("H4")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TierInvalid);
        assert_eq!(err.details()["model"], "p/haiku");
        assert_eq!(catalog().model_arg(t("S0")).unwrap_err().code(), ErrorCode::TierInvalid);
    }

    #[test]
    fn model_arg() {
        assert_eq!(catalog().model_arg(t("S3")).unwrap(), "p/sonnet:high");
        assert_eq!(catalog().model_arg(t("O0")).unwrap(), "p/opus:off");
    }

    #[test]
    fn attempt_tiers_follow_the_ladder_and_stop_when_it_runs_out() {
        let c = catalog();
        let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap().map(|t| t.to_string());
        assert_eq!(at("S1", 1).as_deref(), Some("S1"));
        assert_eq!(at("S1", 2).as_deref(), Some("S2"));
        assert_eq!(at("S1", 3).as_deref(), Some("S3"));
        assert_eq!(at("S1", 4), None);
        assert_eq!(at("H3", 2).as_deref(), Some("S3"));
        assert_eq!(at("H3", 3), None);
        assert_eq!(at("H3", 9), None);
    }
}

#[cfg(test)]
mod tier_g {
    //! Tier G, written `GX` (contract §9 "Tier rules", hs-d562): the syntax, and G as no Worker's Tier.
    use super::*;

    #[test]
    fn g_is_a_tier_written_gx() {
        let g = Tier::parse("GX").unwrap();
        assert_eq!(g.to_string(), "GX");
        assert_eq!(g.letter().as_char(), 'G');
        assert_eq!(g.level(), "off");
    }

    #[test]
    fn g_has_no_number() {
        for n in 0..=5 {
            let name = format!("G{n}");
            let err = Tier::parse(&name).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{name}");
            let m = err.message();
            assert!(m.contains("GX") && m.contains("hs-d562"), "{name}: {m}");
        }
        for name in ["gx", "G", "GXX", "OX", "SX", "HX"] {
            assert_eq!(Tier::parse(name).unwrap_err().code(), ErrorCode::TierInvalid, "{name}");
        }
        for name in ["H5", "S3", "O1"] {
            assert_eq!(Tier::parse(name).unwrap().to_string(), name);
        }
    }

    #[test]
    fn gx_is_never_a_workers_tier() {
        let g = Tier::parse("GX").unwrap();
        assert!(!g.is_worker_allowed());
        let err = g.check_worker().unwrap_err();
        assert_eq!(err.code(), ErrorCode::TierNotAllowed);
        assert_eq!(err.details()["allowed"], "H3, S0-S3");
        assert_eq!(err.details()["rule"], "hs-d554");
    }
}

#[cfg(test)]
mod worker_tiers {
    //! The Worker Tier range and ladder (contract §9, hs-d554).
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            pi_version: "1.3.0".into(),
            o: TierModel { model: "p/opus".into(), efforts: vec![1, 2, 3, 4, 5] },
            s: TierModel { model: "p/sonnet".into(), efforts: vec![0, 1, 2, 3, 4, 5] },
            g: TierModel { model: "p/gemini".into(), efforts: vec![] },
            h: TierModel { model: "p/haiku".into(), efforts: vec![0, 1, 2, 3] },
        }
    }

    fn t(s: &str) -> Tier {
        Tier::parse(s).unwrap()
    }

    #[test]
    fn worker_range_allows_only_h3_and_s0_to_s3() {
        for letter in [Letter::O, Letter::S, Letter::H] {
            for effort in 0..=5 {
                let tier = Tier::new(letter, effort).unwrap();
                let expected = matches!(tier.to_string().as_str(), "H3" | "S0" | "S1" | "S2" | "S3");
                assert_eq!(tier.is_worker_allowed(), expected, "{tier}");
                assert_eq!(tier.check_worker().is_ok(), expected, "{tier}");
            }
        }
    }

    #[test]
    fn worker_range_refusal_names_the_range_the_rule_and_hs_d554() {
        for name in ["H2", "S4", "S5", "O3"] {
            let err = t(name).check_worker().unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierNotAllowed, "{name}");
            assert_eq!(err.details()["tier"], name);
            assert_eq!(err.details()["allowed"], "H3, S0-S3");
            assert_eq!(err.details()["rule"], "hs-d554");
            let m = err.message();
            assert!(m.starts_with(&format!("Tier {name} is not allowed for a Worker")), "{m}");
            for part in ["hs-d554", "cost", "H3, S0-S3"] {
                assert!(m.contains(part), "{part} in {m}");
            }
        }
    }

    #[test]
    fn worker_ladder_ends_at_s3() {
        let c = catalog();
        assert_eq!(c.next(t("S0")).unwrap(), Some(t("S1")));
        assert_eq!(c.next(t("S1")).unwrap(), Some(t("S2")));
        assert_eq!(c.next(t("S2")).unwrap(), Some(t("S3")));
        assert_eq!(c.next(t("S3")).unwrap(), None);
    }

    #[test]
    fn worker_ladder_steps_h3_to_s3_and_gives_h0_to_h2_s4_s5_o3_none() {
        let c = catalog();
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S3")));
        for name in ["H0", "H1", "H2", "S4", "S5", "O3"] {
            assert_eq!(c.next(t(name)).unwrap(), None, "{name}");
        }
    }

    #[test]
    fn worker_attempt_tiers_follow_the_ladder_and_stop_at_s3() {
        let c = catalog();
        let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap();
        assert_eq!([1, 2, 3, 4].map(|n| at("S1", n)), [Some(t("S1")), Some(t("S2")), Some(t("S3")), None]);
        assert_eq!([1, 2, 3].map(|n| at("H3", n)), [Some(t("H3")), Some(t("S3")), None]);
        assert_eq!(at("H2", 2), None);
    }
}
