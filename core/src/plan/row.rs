//! The phases of a Plan (contract §3.1). The `plans` row is read by `show::load_head`.

/// A Plan's phase (§3.1).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Grounding,
    Plan,
    Dispatch,
    Deploy,
    Done,
}

impl Phase {
    pub const ALL: [Phase; 5] = [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Deploy, Phase::Done];

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Grounding => "grounding",
            Phase::Plan => "plan",
            Phase::Dispatch => "dispatch",
            Phase::Deploy => "deploy",
            Phase::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        Phase::ALL.into_iter().find(|p| p.as_str() == s)
    }
}
