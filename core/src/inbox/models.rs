//! The `models-changed` notice (contract §2.2 "The models-changed rule"; pro-d13, pro-d16, pro-n12): what E8 announces of a
//! provider's `changes` in `run/models-<provider>.json`, and the subject that names it.

use std::time::Duration;

use crate::models::Changes;

/// How long a change shows: 7 days from `changes.at` (a constant, not a config key, like `team::WINDOW`).
pub const WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);

/// The provider whose list is announced whole: its list is its `available` list.
const WHOLE: &str = "github-copilot";

/// The non-chat ids (case-insensitive substrings, the pattern `/embed|tts|transcribe|live|image|nano-banana|ocr/i`):
/// the cache keeps them, the announcement leaves them out.
const NON_CHAT: [&str; 7] = ["embed", "tts", "transcribe", "live", "image", "nano-banana", "ocr"];

/// The ids of a change the User is told: `(appeared, went_away)`.
#[must_use]
pub fn announced(provider: &str, changes: &Changes) -> (Vec<String>, Vec<String>) {
    let keep = |ids: &[String]| -> Vec<String> {
        ids.iter()
            .filter(|id| {
                let id = id.to_lowercase();
                provider == WHOLE || !NON_CHAT.iter().any(|p| id.contains(p))
            })
            .cloned()
            .collect()
    };
    (keep(&changes.appeared), keep(&changes.went_away))
}

/// The item's subject: `models:<provider>/<changes.at>`.
#[must_use]
pub fn subject(provider: &str, at: &str) -> String {
    format!("models:{provider}/{at}")
}

/// The provider and `changes.at` of a subject made by [`subject`].
#[must_use]
pub fn parse_subject(subject: &str) -> Option<(&str, &str)> {
    subject.strip_prefix("models:")?.split_once('/')
}

/// `<provider>: <N> new, <M> gone`, a zero part omitted.
#[must_use]
pub fn label(provider: &str, new: usize, gone: usize) -> String {
    let parts: Vec<String> =
        [(new, "new"), (gone, "gone")].into_iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect();
    format!("{provider}: {}", parts.join(", "))
}
