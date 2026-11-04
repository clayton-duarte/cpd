//! Identifiers (contract §2.1), parsed and validated once at the boundary.
//!
//! Each id is its own type: holding one proves it matched §2.1. Parsing an
//! invalid id fails with its `invalid_<thing>_id` code (exit 2). Every type
//! implements `FromStr` (so clap's `value_parser!` accepts it and the error code
//! reaches the envelope, see `cli::usage_error`), `Display`, and serde as a
//! plain string.

use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};

use crate::error::{CpdError, ErrorCode};

fn invalid(code: ErrorCode, what: &str, value: &str, expected: &str) -> CpdError {
    CpdError::new(
        code,
        format!("Invalid {what} {value:?}: expected {expected}."),
    )
    .with("value", value)
    .with("expected", expected)
}

/// `^[first][rest]{min,max}$` without a regex engine.
fn matches(
    s: &str,
    first: fn(char) -> bool,
    rest: fn(char) -> bool,
    min: usize,
    max: usize,
) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if first(c) => {}
        _ => return false,
    }
    let tail: Vec<char> = chars.collect();
    (min..=max).contains(&tail.len()) && tail.into_iter().all(rest)
}

fn lower(c: char) -> bool {
    c.is_ascii_lowercase()
}

fn lower_digit_dash(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
}

fn lower_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// A sequence number: `1`, `2`, … with no sign and no leading zero.
fn seq_number(s: &str) -> Option<NonZeroU32> {
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Implements `FromStr`, `TryFrom<String>`, serde and `as_str`-based traits for
/// a newtype over `String` whose constructor is `fn parse(&str) -> Result<Self, CpdError>`.
macro_rules! id_traits {
    ($ty:ident) => {
        impl FromStr for $ty {
            type Err = CpdError;
            fn from_str(s: &str) -> Result<Self, CpdError> {
                $ty::parse(s)
            }
        }

        impl TryFrom<String> for $ty {
            type Error = CpdError;
            fn try_from(s: String) -> Result<Self, CpdError> {
                $ty::parse(&s)
            }
        }

        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                $ty::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// A newtype over a `String` that matched a pattern.
macro_rules! simple_id {
    ($(#[$doc:meta])* $ty:ident, $code:ident, $what:literal, $expected:literal, $check:expr) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $ty(String);

        impl $ty {
            /// Validates `s` against contract §2.1.
            pub fn parse(s: &str) -> Result<Self, CpdError> {
                let check: fn(&str) -> bool = $check;
                if check(s) {
                    Ok($ty(s.to_owned()))
                } else {
                    Err(invalid(ErrorCode::$code, $what, s, $expected))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        id_traits!($ty);
    };
}

simple_id!(
    /// Context id: `^[a-z][a-z0-9-]{1,39}$`.
    ContextId, InvalidContextId, "Context id", "^[a-z][a-z0-9-]{1,39}$",
    |s| matches(s, lower, lower_digit_dash, 1, 39)
);

/// The three Permanent Contexts (commands.md §3.8): never archived, renamed or deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermanentContext {
    Cpd,
    Host,
    Codereview,
}

impl PermanentContext {
    /// Every Permanent Context, in their fixed order.
    pub const ALL: [PermanentContext; 3] = [
        PermanentContext::Cpd,
        PermanentContext::Host,
        PermanentContext::Codereview,
    ];

    pub fn id(self) -> &'static str {
        match self {
            PermanentContext::Cpd => "cpd",
            PermanentContext::Host => "host",
            PermanentContext::Codereview => "codereview",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            PermanentContext::Cpd => "cpd",
            PermanentContext::Host => "Host: local maintenance of this machine",
            PermanentContext::Codereview => "codereview",
        }
    }
}

impl ContextId {
    /// The Permanent Context this id names, if it is one of the three.
    pub fn permanent(&self) -> Option<PermanentContext> {
        PermanentContext::ALL
            .into_iter()
            .find(|p| p.id() == self.as_str())
    }
}

simple_id!(
    /// Plan id: `^[a-z][a-z0-9-]{1,23}$`.
    PlanId, InvalidPlanId, "Plan id", "^[a-z][a-z0-9-]{1,23}$",
    |s| matches(s, lower, lower_digit_dash, 1, 23)
);

simple_id!(
    /// Job key: `^[a-z][a-z0-9-]{0,31}$`.
    JobKey, InvalidJobId, "Job key", "^[a-z][a-z0-9-]{0,31}$",
    |s| matches(s, lower, lower_digit_dash, 0, 31)
);

simple_id!(
    /// Workflow key (R34): `^[a-z][a-z0-9-]{0,23}$`, unique in its Plan.
    WorkflowKey, InvalidWorkflowId, "Workflow key", "^[a-z][a-z0-9-]{0,23}$",
    |s| matches(s, lower, lower_digit_dash, 0, 23)
);

simple_id!(
    /// Session id as accepted on input: `^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$`.
    /// The ids cpd chooses itself are lowercase UUID v4 ([`SessionId::new_v4`]).
    SessionId, InvalidSessionId, "Session id", "^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$",
    |s| matches(
        s,
        |c| c.is_ascii_alphanumeric(),
        |c| c.is_ascii_alphanumeric() || c == '_' || c == '-',
        0,
        127,
    )
);

simple_id!(
    /// Machine name: `^[a-z][a-z0-9]{0,7}$`, set in `config.toml` (contract §8),
    /// so an invalid one is `config_invalid`.
    MachineName, ConfigInvalid, "machine name", "^[a-z][a-z0-9]{0,7}$",
    |s| matches(s, lower, lower_digit, 0, 7)
);

impl SessionId {
    /// A new session id for a pi session cpd starts: a lowercase UUID v4.
    pub fn new_v4() -> SessionId {
        SessionId(uuid::Uuid::new_v4().hyphenated().to_string())
    }
}

/// Attempt id: `<job>-a<n>`, `n` from 1 per Job.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AttemptId {
    job: JobKey,
    number: NonZeroU32,
}

impl AttemptId {
    pub fn new(job: JobKey, number: NonZeroU32) -> AttemptId {
        AttemptId { job, number }
    }

    pub fn parse(s: &str) -> Result<AttemptId, CpdError> {
        let err = || {
            invalid(
                ErrorCode::InvalidAttemptId,
                "Attempt id",
                s,
                "<job>-a<n>, n from 1",
            )
        };
        // n has no '-' and no 'a', so the separator is the last "-a".
        let (job, n) = s.rsplit_once("-a").ok_or_else(err)?;
        let number = seq_number(n).ok_or_else(err)?;
        let job = JobKey::parse(job).map_err(|_| err())?;
        Ok(AttemptId { job, number })
    }

    pub fn job(&self) -> &JobKey {
        &self.job
    }

    pub fn number(&self) -> NonZeroU32 {
        self.number
    }
}

impl fmt::Display for AttemptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-a{}", self.job, self.number)
    }
}

id_traits!(AttemptId);

/// Escalation id: `<attempt>-q<k>`, `k` from 1 per Attempt.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EscalationId {
    attempt: AttemptId,
    number: NonZeroU32,
}

impl EscalationId {
    pub fn new(attempt: AttemptId, number: NonZeroU32) -> EscalationId {
        EscalationId { attempt, number }
    }

    pub fn parse(s: &str) -> Result<EscalationId, CpdError> {
        let err = || {
            invalid(
                ErrorCode::InvalidEscalationId,
                "Escalation id",
                s,
                "<attempt>-q<k>, k from 1",
            )
        };
        let (attempt, k) = s.rsplit_once("-q").ok_or_else(err)?;
        let number = seq_number(k).ok_or_else(err)?;
        let attempt = AttemptId::parse(attempt).map_err(|_| err())?;
        Ok(EscalationId { attempt, number })
    }

    pub fn attempt(&self) -> &AttemptId {
        &self.attempt
    }

    pub fn number(&self) -> NonZeroU32 {
        self.number
    }
}

impl fmt::Display for EscalationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-q{}", self.attempt, self.number)
    }
}

id_traits!(EscalationId);

/// Plan review id: `r<k>`, `k` from 1 per Plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReviewId {
    number: NonZeroU32,
}

impl ReviewId {
    pub fn new(number: NonZeroU32) -> ReviewId {
        ReviewId { number }
    }

    pub fn parse(s: &str) -> Result<ReviewId, CpdError> {
        let err = || {
            invalid(
                ErrorCode::InvalidReviewId,
                "Plan review id",
                s,
                "r<k>, k from 1",
            )
        };
        let k = s.strip_prefix('r').ok_or_else(err)?;
        Ok(ReviewId {
            number: seq_number(k).ok_or_else(err)?,
        })
    }

    pub fn number(&self) -> NonZeroU32 {
        self.number
    }
}

impl fmt::Display for ReviewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.number)
    }
}

id_traits!(ReviewId);

/// The kind of a Context record, and its id letter (contract §2.1, §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    Decision,
    Note,
    Question,
    Answer,
    Plan,
    Brief,
}

impl RecordKind {
    pub const ALL: [RecordKind; 6] = [
        RecordKind::Decision,
        RecordKind::Note,
        RecordKind::Question,
        RecordKind::Answer,
        RecordKind::Plan,
        RecordKind::Brief,
    ];

    /// The letter in record ids: `d`, `n`, `q`, `a`, `p`, `b`.
    pub fn letter(self) -> char {
        match self {
            RecordKind::Decision => 'd',
            RecordKind::Note => 'n',
            RecordKind::Question => 'q',
            RecordKind::Answer => 'a',
            RecordKind::Plan => 'p',
            RecordKind::Brief => 'b',
        }
    }

    pub fn from_letter(letter: char) -> Option<RecordKind> {
        RecordKind::ALL
            .into_iter()
            .find(|kind| kind.letter() == letter)
    }

    /// The `kind` field of the record: `decision`, `note`, `question`, `answer`, `plan`, `brief`.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Decision => "decision",
            RecordKind::Note => "note",
            RecordKind::Question => "question",
            RecordKind::Answer => "answer",
            RecordKind::Plan => "plan",
            RecordKind::Brief => "brief",
        }
    }
}

/// Context record id: `<machine>-<letter><n>`, `n` from 1 per machine,
/// Context and letter.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordId {
    machine: MachineName,
    kind: RecordKind,
    number: NonZeroU32,
}

impl RecordId {
    pub fn new(machine: MachineName, kind: RecordKind, number: NonZeroU32) -> RecordId {
        RecordId {
            machine,
            kind,
            number,
        }
    }

    pub fn parse(s: &str) -> Result<RecordId, CpdError> {
        let err = || {
            invalid(
                ErrorCode::InvalidRecordId,
                "Context record id",
                s,
                "<machine>-<letter><n>, letter one of d n q a p b, n from 1",
            )
        };
        // A machine name has no '-', so the separator is the first one.
        let (machine, rest) = s.split_once('-').ok_or_else(err)?;
        let machine = MachineName::parse(machine).map_err(|_| err())?;
        let mut chars = rest.chars();
        let kind = chars
            .next()
            .and_then(RecordKind::from_letter)
            .ok_or_else(err)?;
        let number = seq_number(chars.as_str()).ok_or_else(err)?;
        Ok(RecordId {
            machine,
            kind,
            number,
        })
    }

    pub fn machine(&self) -> &MachineName {
        &self.machine
    }

    pub fn kind(&self) -> RecordKind {
        self.kind
    }

    pub fn number(&self) -> NonZeroU32 {
        self.number
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}{}", self.machine, self.kind.letter(), self.number)
    }
}

id_traits!(RecordId);

/// A GitHub owner login (contract §2.1, hs-d288): `^[a-z0-9](-?[a-z0-9])*$`,
/// 1–39 characters, ASCII-lowercased when parsed. The policy key's owner.
/// An invalid one is `usage` (commands.md §3.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OwnerLogin(String);

impl OwnerLogin {
    const EXPECTED: &'static str = "^[a-z0-9](-?[a-z0-9])*$, 1-39 characters";

    pub fn parse(s: &str) -> Result<Self, CpdError> {
        let lowered = s.to_ascii_lowercase();
        let alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
        let ok = (1..=39).contains(&lowered.len())
            && lowered.chars().all(|c| alnum(c) || c == '-')
            && !lowered.starts_with('-')
            && !lowered.ends_with('-')
            && !lowered.contains("--");
        if ok {
            Ok(OwnerLogin(lowered))
        } else {
            Err(invalid(ErrorCode::Usage, "owner login", s, Self::EXPECTED))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OwnerLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

id_traits!(OwnerLogin);

/// A GitHub repository name without its owner (contract §2.1, hs-d288):
/// `^[a-z0-9._-]{1,100}$`, ASCII-lowercased when parsed; `owner` is refused (`<owner>/owner.toml` is the owner
/// scope, not a repo's). An invalid one is `usage`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoName(String);

impl RepoName {
    const EXPECTED: &'static str = "^[a-z0-9._-]{1,100}$, not `owner`";

    pub fn parse(s: &str) -> Result<Self, CpdError> {
        let lowered = s.to_ascii_lowercase();
        let ok = (1..=100).contains(&lowered.len())
            && lowered.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            });
        if ok && lowered != "owner" {
            Ok(RepoName(lowered))
        } else {
            Err(invalid(ErrorCode::Usage, "repo name", s, Self::EXPECTED))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

id_traits!(RepoName);

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: u32) -> NonZeroU32 {
        NonZeroU32::new(v).unwrap()
    }

    fn code<T: fmt::Debug>(r: Result<T, CpdError>) -> ErrorCode {
        r.expect_err("expected an error").code()
    }

    #[test]
    fn context_ids() {
        for ok in ["building-cpd", "ab", "a1", &format!("a{}", "b".repeat(39))] {
            assert_eq!(ContextId::parse(ok).unwrap().as_str(), ok);
        }
        for bad in [
            "",
            "a",
            "1ab",
            "-ab",
            "Ab",
            "a_b",
            "a b",
            &format!("a{}", "b".repeat(40)),
        ] {
            assert_eq!(
                code(ContextId::parse(bad)),
                ErrorCode::InvalidContextId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn plan_ids() {
        for ok in ["pay-4521", "cpd2", "ab", &format!("a{}", "b".repeat(23))] {
            assert!(PlanId::parse(ok).is_ok(), "{ok:?}");
        }
        for bad in [
            "",
            "a",
            "2cpd",
            "Cpd2",
            "cpd/2",
            "cpd.2",
            &format!("a{}", "b".repeat(24)),
        ] {
            assert_eq!(
                code(PlanId::parse(bad)),
                ErrorCode::InvalidPlanId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn job_keys() {
        for ok in ["a", "api-schema", "x-a1", &format!("a{}", "b".repeat(31))] {
            assert!(JobKey::parse(ok).is_ok(), "{ok:?}");
        }
        for bad in ["", "1a", "A", "a_b", &format!("a{}", "b".repeat(32))] {
            assert_eq!(code(JobKey::parse(bad)), ErrorCode::InvalidJobId, "{bad:?}");
        }
    }

    #[test]
    fn workflow_keys() {
        for ok in [
            "a",
            "schema",
            "web-2",
            "a-",
            &format!("a{}", "b".repeat(23)),
        ] {
            assert_eq!(WorkflowKey::parse(ok).unwrap().to_string(), ok);
            assert_eq!(ok.parse::<WorkflowKey>().unwrap().as_str(), ok);
        }
        for bad in [
            "",
            "1a",
            "-a",
            "A",
            "a_b",
            "a/b",
            "a.b",
            &format!("a{}", "b".repeat(24)),
        ] {
            assert_eq!(
                code(WorkflowKey::parse(bad)),
                ErrorCode::InvalidWorkflowId,
                "{bad:?}"
            );
        }
        let err = WorkflowKey::parse("Web").unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert_eq!(err.details()["expected"], "^[a-z][a-z0-9-]{0,23}$");
        let key: WorkflowKey = serde_json::from_str("\"web\"").unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"web\"");
        let err = serde_json::from_str::<WorkflowKey>("\"Web\"").unwrap_err();
        assert!(err.to_string().contains("invalid_workflow_id"), "{err}");
    }

    #[test]
    fn machine_names() {
        for ok in ["hs", "h", "mac2", "abcdefgh"] {
            assert!(MachineName::parse(ok).is_ok(), "{ok:?}");
        }
        for bad in ["", "abcdefghi", "h-s", "2h", "Hs"] {
            assert_eq!(
                code(MachineName::parse(bad)),
                ErrorCode::ConfigInvalid,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn session_ids() {
        for ok in [
            "596d007f-6775-4134-b77a-b356cc523842",
            "A",
            "a_b-C",
            &"a".repeat(128),
        ] {
            assert!(SessionId::parse(ok).is_ok(), "{ok:?}");
        }
        for bad in ["", "-a", "_a", "a.b", "a/b", &"a".repeat(129)] {
            assert_eq!(
                code(SessionId::parse(bad)),
                ErrorCode::InvalidSessionId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn new_session_ids_are_lowercase_uuid_v4() {
        let id = SessionId::new_v4();
        let s = id.as_str();
        assert_eq!(s.len(), 36);
        assert_eq!(s, s.to_ascii_lowercase());
        assert_eq!(&s[14..15], "4", "version nibble");
        assert!(SessionId::parse(s).is_ok());
        assert_ne!(SessionId::new_v4(), id);
    }

    #[test]
    fn attempt_ids() {
        let id = AttemptId::parse("api-schema-a2").unwrap();
        assert_eq!(id.job().as_str(), "api-schema");
        assert_eq!(id.number(), n(2));
        assert_eq!(id.to_string(), "api-schema-a2");
        // A Job key may itself end in -a<digits>.
        let id = AttemptId::parse("x-a1-a12").unwrap();
        assert_eq!(id.job().as_str(), "x-a1");
        assert_eq!(id.number(), n(12));
        assert_eq!(
            AttemptId::new(JobKey::parse("w").unwrap(), n(3)).to_string(),
            "w-a3"
        );
        for bad in [
            "",
            "api-schema",
            "api-schema-a",
            "api-schema-a0",
            "api-schema-a01",
            "-a1",
            "A-a1",
            "x-a+1",
            "x-a1x",
        ] {
            assert_eq!(
                code(AttemptId::parse(bad)),
                ErrorCode::InvalidAttemptId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn escalation_ids() {
        let id = EscalationId::parse("api-schema-a2-q1").unwrap();
        assert_eq!(id.attempt().to_string(), "api-schema-a2");
        assert_eq!(id.number(), n(1));
        assert_eq!(id.to_string(), "api-schema-a2-q1");
        for bad in [
            "api-schema-a2",
            "api-schema-q1",
            "api-schema-a2-q0",
            "api-schema-a2-q",
            "x-a1-q1x",
        ] {
            assert_eq!(
                code(EscalationId::parse(bad)),
                ErrorCode::InvalidEscalationId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn review_ids() {
        assert_eq!(ReviewId::parse("r3").unwrap().number(), n(3));
        assert_eq!(ReviewId::new(n(12)).to_string(), "r12");
        for bad in ["", "r", "r0", "r01", "3", "R3", "r-1", "r3a"] {
            assert_eq!(
                code(ReviewId::parse(bad)),
                ErrorCode::InvalidReviewId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn record_ids() {
        let id = RecordId::parse("hs-d3").unwrap();
        assert_eq!(id.machine().as_str(), "hs");
        assert_eq!(id.kind(), RecordKind::Decision);
        assert_eq!(id.number(), n(3));
        assert_eq!(id.to_string(), "hs-d3");
        for kind in RecordKind::ALL {
            let s = format!("mac2-{}10", kind.letter());
            assert_eq!(RecordId::parse(&s).unwrap().kind(), kind);
            assert_eq!(
                RecordId::new(MachineName::parse("mac2").unwrap(), kind, n(10)).to_string(),
                s
            );
        }
        for bad in [
            "",
            "hs",
            "hs-",
            "hs-d",
            "hs-d0",
            "hs-x3",
            "HS-d3",
            "h-s-d3",
            "hs-d3x",
            "abcdefghi-d1",
        ] {
            assert_eq!(
                code(RecordId::parse(bad)),
                ErrorCode::InvalidRecordId,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn record_kind_names() {
        let names: Vec<_> = RecordKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(
            names,
            ["decision", "note", "question", "answer", "plan", "brief"]
        );
    }

    #[test]
    fn invalid_ids_carry_value_and_pattern() {
        let err = PlanId::parse("Bad").unwrap_err();
        assert_eq!(err.exit().code(), 2);
        assert_eq!(err.details()["value"], "Bad");
        assert_eq!(err.details()["expected"], "^[a-z][a-z0-9-]{1,23}$");
    }

    #[test]
    fn serde_round_trips_and_validates() {
        let id: AttemptId = serde_json::from_str("\"api-schema-a2\"").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"api-schema-a2\"");
        let err = serde_json::from_str::<PlanId>("\"Bad\"").unwrap_err();
        assert!(err.to_string().contains("invalid_plan_id"), "{err}");
    }

    #[test]
    fn owner_login_and_repo_name_follow_contract_2_1() {
        assert_eq!(
            OwnerLogin::parse("Clayton-Duarte").unwrap().as_str(),
            "clayton-duarte"
        );
        for bad in ["", "-a", "a-", "a--b", "a_b", "a b", &"a".repeat(40)] {
            assert_eq!(
                OwnerLogin::parse(bad).unwrap_err().code(),
                ErrorCode::Usage,
                "{bad}"
            );
        }
        assert!(OwnerLogin::parse(&"a".repeat(39)).is_ok());
        assert_eq!(
            RepoName::parse("My.Repo_x-1").unwrap().as_str(),
            "my.repo_x-1"
        );
        for bad in ["", "a/b", "a b", &"a".repeat(101)] {
            assert_eq!(
                RepoName::parse(bad).unwrap_err().code(),
                ErrorCode::Usage,
                "{bad}"
            );
        }
        assert!(RepoName::parse(&"a".repeat(100)).is_ok());
    }
}
