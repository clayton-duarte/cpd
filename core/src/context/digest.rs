//! The Context digest (contract §4.6, hs-d194 D1, D3): its files and its validation.
//!
//! A digest is a derived file, never a Context record. Files live under
//! `run/context-digests/<context>/` (paths through [`CpdHome`]); the latest is
//! the one with the highest `<upto_seq>`. [`validate`] is pure: the digest's
//! bytes and the ids of the records in force in, the failures out.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};

/// The size limit of a digest, in bytes (32 KB; contract §4.6).
pub const LIMIT: usize = 32_768;

/// A digest file on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    /// The highest `seq` of the Context's records it covers.
    pub upto_seq: u64,
    pub file: PathBuf,
    pub text: String,
}

/// Why a digest is not valid: every failure at once (contract §4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    pub bytes: usize,
    pub limit: usize,
    /// The digest is not UTF-8.
    pub not_utf8: bool,
    /// More than `limit` bytes.
    pub too_big: bool,
    /// Records in force that the digest does not cite.
    pub missing: Vec<RecordId>,
    /// Ids the digest cites that are not in force: superseded, `plan` or `brief` records, or unknown ids.
    pub stray: Vec<RecordId>,
}

/// Checks a digest: UTF-8, at most `limit` bytes, and a citation of every
/// record in `in_force` and of nothing else. `missing` and `stray` are sorted
/// and without repeats.
pub fn validate(digest: &[u8], in_force: &[RecordId], limit: usize) -> Result<(), Invalid> {
    let text = String::from_utf8_lossy(digest);
    let cited = citations(&text);
    let wanted: BTreeSet<&RecordId> = in_force.iter().collect();
    let missing: Vec<RecordId> = wanted.iter().filter(|id| !cited.contains(**id)).map(|id| (*id).clone()).collect();
    let stray: Vec<RecordId> = cited.iter().filter(|id| !wanted.contains(id)).cloned().collect();
    let invalid = Invalid {
        bytes: digest.len(),
        limit,
        not_utf8: std::str::from_utf8(digest).is_err(),
        too_big: digest.len() > limit,
        missing,
        stray,
    };
    if invalid.not_utf8 || invalid.too_big || !invalid.missing.is_empty() || !invalid.stray.is_empty() {
        Err(invalid)
    } else {
        Ok(())
    }
}

/// Every record id the text cites. A citation is a square-bracketed group
/// whose content is one or more record ids separated by commas and optional
/// spaces (`[hs-d43]`, `[hs-d43, hs-n7]`); any other bracketed text cites nothing.
pub fn citations(text: &str) -> BTreeSet<RecordId> {
    let mut cited = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        // The group ends at the next `]`; a `[` before it starts a new group.
        let Some(end) = after.find([']', '[']) else { break };
        if after.as_bytes()[end] == b'[' {
            rest = &after[end..];
            continue;
        }
        if let Some(ids) = after[..end].split(',').map(|part| RecordId::parse(part.trim_matches(' ')).ok()).collect::<Option<Vec<_>>>() {
            cited.extend(ids);
        }
        rest = &after[end + 1..];
    }
    cited
}

fn io_error(what: &str, path: &Path, e: &io::Error) -> CpdError {
    CpdError::new(ErrorCode::FileUnreadable, format!("Cannot {what} {}: {e}.", path.display()))
        .with("path", path.display().to_string())
}

/// The `<upto_seq>` of a file named `<upto_seq>.md`; none for any other name.
fn upto_seq_of(name: &str) -> Option<u64> {
    let digits = name.strip_suffix(".md")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
}

fn digest_files(home: &CpdHome, context: &ContextId) -> Result<Vec<(u64, PathBuf)>, CpdError> {
    let dir = home.context_digest_dir(context);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error("read", &dir, &e)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error("read", &dir, &e))?;
        if let Some(upto) = entry.file_name().to_str().and_then(upto_seq_of) {
            files.push((upto, entry.path()));
        }
    }
    Ok(files)
}

/// The Context's latest digest: the file with the highest `<upto_seq>`, or none.
/// A file that cannot be read as UTF-8 is `file_unreadable`.
pub fn latest(home: &CpdHome, context: &ContextId) -> Result<Option<Digest>, CpdError> {
    let Some((upto_seq, file)) = digest_files(home, context)?.into_iter().max_by_key(|(upto, _)| *upto) else {
        return Ok(None);
    };
    let text = fs::read_to_string(&file).map_err(|e| io_error("read", &file, &e))?;
    Ok(Some(Digest { upto_seq, file, text }))
}

/// Renames `<upto_seq>.md.tmp` to `<upto_seq>.md` and removes every other
/// `*.md` and every `*.md.tmp` of the Context, so the directory holds one
/// digest. Call it only after [`validate`] passed.
pub fn install(home: &CpdHome, context: &ContextId, upto_seq: u64) -> Result<PathBuf, CpdError> {
    let tmp = home.context_digest_tmp(context, upto_seq);
    let file = home.context_digest(context, upto_seq);
    fs::rename(&tmp, &file).map_err(|e| io_error("rename", &tmp, &e))?;
    let dir = home.context_digest_dir(context);
    for entry in fs::read_dir(&dir).map_err(|e| io_error("read", &dir, &e))? {
        let path = entry.map_err(|e| io_error("read", &dir, &e))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path != file && (upto_seq_of(name).is_some() || is_tmp(name)) {
            fs::remove_file(&path).map_err(|e| io_error("remove", &path, &e))?;
        }
    }
    Ok(file)
}

fn is_tmp(name: &str) -> bool {
    name.strip_suffix(".tmp").is_some_and(|n| upto_seq_of(n).is_some())
}

/// Removes the `<upto_seq>.md.tmp` of a digest that failed; absent is fine.
pub fn discard(home: &CpdHome, context: &ContextId, upto_seq: u64) -> Result<(), CpdError> {
    let tmp = home.context_digest_tmp(context, upto_seq);
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error("remove", &tmp, &e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> RecordId {
        RecordId::parse(s).unwrap()
    }

    fn ids(list: &[&str]) -> Vec<RecordId> {
        list.iter().map(|s| id(s)).collect()
    }

    fn invalid(text: &str, in_force: &[&str]) -> Invalid {
        validate(text.as_bytes(), &ids(in_force), LIMIT).unwrap_err()
    }

    #[test]
    fn a_digest_citing_every_record_in_force_and_nothing_else_is_valid() {
        let text = "One engine per machine [hs-d7]. Both settled [hs-d8, hs-n3].\nAsked [hs-q1] and answered [hs-a1,hs-n3].";
        assert_eq!(validate(text.as_bytes(), &ids(&["hs-d7", "hs-d8", "hs-n3", "hs-q1", "hs-a1"]), LIMIT), Ok(()));
    }

    #[test]
    fn a_record_in_force_that_is_not_cited_is_missing() {
        let e = invalid("Only [hs-d7].", &["hs-d7", "hs-d9", "hs-d8"]);
        assert_eq!(e.missing, ids(&["hs-d8", "hs-d9"]));
        assert!(e.stray.is_empty());
    }

    #[test]
    fn a_cited_id_that_is_not_in_force_is_stray() {
        let e = invalid("Keep [hs-d7]; drop [hs-d1] and [hs-p2, hs-d1]; [hs-d99].", &["hs-d7"]);
        assert!(e.missing.is_empty());
        assert_eq!(e.stray, ids(&["hs-d1", "hs-d99", "hs-p2"]));
    }

    #[test]
    fn missing_and_stray_are_reported_together() {
        let e = invalid("[hs-d1]", &["hs-d2"]);
        assert_eq!((e.missing, e.stray), (ids(&["hs-d2"]), ids(&["hs-d1"])));
    }

    #[test]
    fn only_a_group_of_record_ids_is_a_citation() {
        let text = "See [the docs](http://x/hs-d1), [hs-d2 and more], [], [ , ], [hs-d3,], [hs-d4 hs-d5], [HS-D6], [hs-d7, oops].";
        assert!(citations(text).is_empty(), "{:?}", citations(text));
        let text = "[nested [hs-d1] group] and [[hs-d2]] and [ hs-d3 ,  hs-d4 ] and [hs-d5";
        assert_eq!(citations(text), ids(&["hs-d1", "hs-d2", "hs-d3", "hs-d4"]).into_iter().collect());
    }

    #[test]
    fn a_repeated_citation_counts_once() {
        let text = "[hs-d1] and again [hs-d1], [hs-d1, hs-d1]";
        assert_eq!(validate(text.as_bytes(), &ids(&["hs-d1"]), LIMIT), Ok(()));
    }

    #[test]
    fn the_size_limit_is_in_bytes_and_inclusive() {
        let cite = "[hs-d1]";
        let at_limit = format!("{cite}{}", "é".repeat((LIMIT - cite.len()) / 2)) + &"x".repeat((LIMIT - cite.len()) % 2);
        assert_eq!(at_limit.len(), LIMIT);
        assert_eq!(validate(at_limit.as_bytes(), &ids(&["hs-d1"]), LIMIT), Ok(()));
        let e = invalid(&format!("{at_limit}x"), &["hs-d1"]);
        assert_eq!((e.too_big, e.bytes, e.limit), (true, LIMIT + 1, LIMIT));
        assert_eq!(validate(b"[hs-d1] is long", &ids(&["hs-d1"]), 14).unwrap_err().too_big, true);
    }

    #[test]
    fn a_digest_that_is_not_utf8_is_invalid() {
        let mut bytes = b"[hs-d1] ".to_vec();
        bytes.push(0xff);
        let e = validate(&bytes, &ids(&["hs-d1"]), LIMIT).unwrap_err();
        assert!(e.not_utf8 && !e.too_big && e.missing.is_empty());
    }

    #[test]
    fn nothing_in_force_needs_no_citation() {
        assert_eq!(validate(b"", &[], LIMIT), Ok(()));
        assert_eq!(invalid("[hs-d1]", &[]).stray, ids(&["hs-d1"]));
    }

    fn home(dir: &Path) -> CpdHome {
        CpdHome::resolve(Some(dir.into()), None).unwrap()
    }

    fn ctx() -> ContextId {
        ContextId::parse("building-cpd").unwrap()
    }

    #[test]
    fn the_latest_digest_is_the_highest_upto_seq() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        assert_eq!(latest(&home, &ctx()).unwrap(), None, "no directory");
        fs::create_dir_all(home.context_digest_dir(&ctx())).unwrap();
        assert_eq!(latest(&home, &ctx()).unwrap(), None, "no digest");
        for (name, text) in [("9.md", "nine"), ("10.md", "ten"), ("99.md.tmp", "x"), ("prompt.md", "x"), ("records.md", "x"), ("distill.log", "x")] {
            fs::write(home.context_digest_dir(&ctx()).join(name), text).unwrap();
        }
        let digest = latest(&home, &ctx()).unwrap().unwrap();
        assert_eq!((digest.upto_seq, digest.text.as_str()), (10, "ten"), "10 is above 9, which a text sort would not say");
        assert_eq!(digest.file, home.context_digest(&ctx(), 10));
        assert_eq!(latest(&home, &ContextId::parse("other").unwrap()).unwrap(), None);
    }

    #[test]
    fn install_renames_the_tmp_and_leaves_one_digest() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        let d = home.context_digest_dir(&ctx());
        fs::create_dir_all(&d).unwrap();
        for name in ["5.md", "7.md.tmp", "8.md.tmp", "prompt.md", "records.md", "distill.log"] {
            fs::write(d.join(name), name).unwrap();
        }
        fs::write(home.context_digest_tmp(&ctx(), 8), "new").unwrap();
        assert_eq!(install(&home, &ctx(), 8).unwrap(), home.context_digest(&ctx(), 8));
        let mut left: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["8.md", "distill.log", "prompt.md", "records.md"]);
        assert_eq!(latest(&home, &ctx()).unwrap().unwrap().text, "new");
    }

    #[test]
    fn install_without_a_tmp_fails_and_keeps_the_previous_digest() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        fs::create_dir_all(home.context_digest_dir(&ctx())).unwrap();
        fs::write(home.context_digest(&ctx(), 5), "old").unwrap();
        assert_eq!(install(&home, &ctx(), 8).unwrap_err().code(), ErrorCode::FileUnreadable);
        assert_eq!(latest(&home, &ctx()).unwrap().unwrap().upto_seq, 5);
    }

    #[test]
    fn discard_removes_the_tmp_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        fs::create_dir_all(home.context_digest_dir(&ctx())).unwrap();
        fs::write(home.context_digest_tmp(&ctx(), 8), "bad").unwrap();
        discard(&home, &ctx(), 8).unwrap();
        discard(&home, &ctx(), 8).unwrap();
        assert!(!home.context_digest_tmp(&ctx(), 8).exists());
    }
}
