//! The Context digest (contract §4.6, hs-d194 D1, D3): its files and its validation.
//!
//! A digest is a derived file, never a Context record. Files live under
//! `run/context-digests/<context>/` (paths through [`CpdHome`]); the latest is
//! the one with the highest `<upto_seq>`. A digest is the distiller's topic
//! sections, then cpd's record index (hs-d244 D-a), which the model never writes.
//! [`validate`] is pure: the topic sections' bytes and what the records in force
//! [`Expect`] in, the failures out.
//! [`first_message`] is the one rendering of "the Context" for a first message (D6).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::render::{Indexed, markdown};
use super::{ContextRow, Record, plan_repos};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, MachineName, PlanId, RecordId, RecordKind};

/// The size limit of a digest, in bytes (32 KB; contract §4.6).
pub const LIMIT: usize = 32_768;

/// The most raw records a first message lists after the digest (contract §4.6 "Consumers", D6).
pub const NEWER_MAX: usize = 20;

/// A digest file on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest {
    /// The highest `seq` of the Context's records it covers.
    pub upto_seq: u64,
    pub file: PathBuf,
    pub text: String,
}

/// The line where cpd's index starts; the last such line of a digest file is the start of cpd's index.
pub const INDEX_HEADING: &str = "## Record index";

/// Why a digest is not valid: every failure at once (contract §4.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    /// The size of the whole digest file: topic sections and cpd's index.
    pub bytes: usize,
    pub limit: usize,
    /// The topic sections are not UTF-8.
    pub not_utf8: bool,
    /// The whole file is more than `limit` bytes.
    pub too_big: bool,
    /// Decisions and open questions in force that the topic sections do not cite.
    pub missing: Vec<RecordId>,
    /// Ids the topic sections cite that are not in force: superseded, `plan` or `brief` records, or unknown ids.
    pub stray: Vec<RecordId>,
}

/// What a digest of the records in force must and may cite, and cpd's index of them (hs-d244 D-a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expect {
    /// Every decision and every open question in force: the topic sections must cite each.
    required: BTreeSet<RecordId>,
    /// Every record in force: the topic sections may cite these and nothing else.
    allowed: BTreeSet<RecordId>,
    /// The lines of cpd's index, one per record in force, oldest first.
    index: String,
}

impl Expect {
    /// For `in_force`, the records in force in `seq` order ([`super::inforce::in_force`]). A question is open
    /// when no record in force answers it.
    pub fn of(in_force: &[&Indexed]) -> Expect {
        let answered: BTreeSet<&RecordId> = in_force.iter().filter_map(|(_, r)| r.question()).collect();
        let mut expect = Expect { required: BTreeSet::new(), allowed: BTreeSet::new(), index: String::new() };
        for (_, record) in in_force {
            let kind = record.kind();
            if kind == RecordKind::Decision || (kind == RecordKind::Question && !answered.contains(&record.id)) {
                expect.required.insert(record.id.clone());
            }
            expect.allowed.insert(record.id.clone());
            let date = record.at.get(..10).unwrap_or(&record.at);
            expect.index.push_str(&format!("- [{}] {} ({date})\n", record.id, kind.as_str()));
        }
        expect
    }
}

/// The digest file: the topic sections without trailing whitespace, a blank line, [`INDEX_HEADING`], a blank line and `index`.
fn compose(topic: &[u8], index: &str) -> Vec<u8> {
    let end = topic.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(0, |i| i + 1);
    let mut file = topic[..end].to_vec();
    file.extend_from_slice(format!("\n\n{INDEX_HEADING}\n\n{index}").as_bytes());
    file
}

/// The topic sections of a digest file: everything before the last [`INDEX_HEADING`] line, or all of it
/// when there is none.
pub fn topic_sections(text: &str) -> &str {
    let (mut offset, mut cut) = (0, text.len());
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == INDEX_HEADING {
            cut = offset;
        }
        offset += line.len();
    }
    &text[..cut]
}

/// Checks a digest's topic sections: UTF-8; the whole file (with cpd's index appended) at most `limit` bytes;
/// every decision and open question in force cited, and nothing cited that is not in force. `missing` and
/// `stray` are sorted and without repeats.
pub fn validate(topic: &[u8], expect: &Expect, limit: usize) -> Result<(), Invalid> {
    let text = String::from_utf8_lossy(topic);
    let cited = citations(&text);
    let bytes = compose(topic, &expect.index).len();
    let invalid = Invalid {
        bytes,
        limit,
        not_utf8: std::str::from_utf8(topic).is_err(),
        too_big: bytes > limit,
        missing: expect.required.iter().filter(|id| !cited.contains(*id)).cloned().collect(),
        stray: cited.iter().filter(|id| !expect.allowed.contains(*id)).cloned().collect(),
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

/// The kept invalid digest of `upto_seq`: `<upto_seq>.md.invalid` (hs-d244 D-b).
pub fn invalid_file(home: &CpdHome, context: &ContextId, upto_seq: u64) -> PathBuf {
    home.context_digest(context, upto_seq).with_extension("md.invalid")
}

/// Appends cpd's `index` to the topic sections in `<upto_seq>.md.tmp`, renames it to `<upto_seq>.md` and
/// removes every other `*.md`, `*.md.tmp` and `*.md.invalid` of the Context, so the directory holds one
/// digest. Call it only after [`validate`] passed.
pub fn install(home: &CpdHome, context: &ContextId, upto_seq: u64, expect: &Expect) -> Result<PathBuf, CpdError> {
    let tmp = home.context_digest_tmp(context, upto_seq);
    let file = home.context_digest(context, upto_seq);
    let topic = fs::read(&tmp).map_err(|e| io_error("read", &tmp, &e))?;
    fs::write(&tmp, compose(&topic, &expect.index)).map_err(|e| io_error("write", &tmp, &e))?;
    fs::rename(&tmp, &file).map_err(|e| io_error("rename", &tmp, &e))?;
    remove_in_dir(home, context, |name| upto_seq_of(name).is_some() || is_leftover(name), &file)?;
    Ok(file)
}

/// Renames `<upto_seq>.md.tmp` of a digest that failed validation to [`invalid_file`], replacing the
/// Context's previous one (D-b). The latest valid digest stays.
pub fn keep_invalid(home: &CpdHome, context: &ContextId, upto_seq: u64) -> Result<PathBuf, CpdError> {
    let tmp = home.context_digest_tmp(context, upto_seq);
    let kept = invalid_file(home, context, upto_seq);
    fs::rename(&tmp, &kept).map_err(|e| io_error("rename", &tmp, &e))?;
    remove_in_dir(home, context, |name| invalid_seq_of(name).is_some(), &kept)?;
    Ok(kept)
}

/// Removes the files of the Context's digest directory whose names `remove` accepts, except `keep`.
fn remove_in_dir(home: &CpdHome, context: &ContextId, remove: impl Fn(&str) -> bool, keep: &Path) -> Result<(), CpdError> {
    let dir = home.context_digest_dir(context);
    for entry in fs::read_dir(&dir).map_err(|e| io_error("read", &dir, &e))? {
        let path = entry.map_err(|e| io_error("read", &dir, &e))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path != keep && remove(name) {
            fs::remove_file(&path).map_err(|e| io_error("remove", &path, &e))?;
        }
    }
    Ok(())
}

/// A `<upto_seq>.md.tmp` or a `<upto_seq>.md.invalid`.
fn is_leftover(name: &str) -> bool {
    name.strip_suffix(".tmp").is_some_and(|n| upto_seq_of(n).is_some()) || invalid_seq_of(name).is_some()
}

/// The `<upto_seq>` of a file named `<upto_seq>.md.invalid`.
fn invalid_seq_of(name: &str) -> Option<u64> {
    upto_seq_of(name.strip_suffix(".invalid")?)
}

/// Removes the `<upto_seq>.md.tmp` of a digest that failed; absent is fine.
pub fn discard(home: &CpdHome, context: &ContextId, upto_seq: u64) -> Result<(), CpdError> {
    let tmp = home.context_digest_tmp(context, upto_seq);
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error("remove", &tmp, &e)),
        _ => Ok(()),
    }
}

/// "The Context" of a Lead's or Manager's first message (contract §4.6 "Consumers", D6), read in `conn`'s
/// transaction. With a digest: its text, then the records written after its `upto_seq`, oldest first, at
/// most [`NEWER_MAX`], rendered as `context show --since <upto_seq>` renders them, then, when more exist,
/// `and <K> more: cpd context show --context <c> --since <seq>`, `<seq>` the `seq` before the first record
/// not listed. With no digest yet: `context show` as markdown, every record.
///
/// `own_brief_of` is the Plan whose brief the message holds in full: that brief is not listed again.
/// The digest is read before the records, so every record it covers is in what is read.
pub(crate) fn first_message(
    conn: &Connection,
    home: &CpdHome,
    machine: &MachineName,
    context: &ContextId,
    own_brief_of: Option<&PlanId>,
) -> Result<String, CpdError> {
    let row = ContextRow::require(conn, context)?;
    let digest = latest(home, context)?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
    let mut records = stmt.query_map([context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    if let Some(plan) = own_brief_of {
        records.retain(|(_, r)| !(r.kind() == RecordKind::Brief && r.plan().is_some_and(|p| p.id == *plan)));
    }
    let repos = plan_repos(conn, machine, &records)?;
    let Some(digest) = digest else {
        return Ok(markdown(&row.title, row.jira.as_deref(), &records, 0, &repos));
    };
    let upto = i64::try_from(digest.upto_seq).unwrap_or(i64::MAX);
    let first = records.partition_point(|(seq, _)| *seq <= upto);
    let end = records.len().min(first + NEWER_MAX);
    let mut parts = vec![digest.text.trim_end().to_owned()];
    if end > first {
        // Judged from the records up to the last one listed, so no listed record is "superseded by" an unlisted one.
        parts.push(markdown(&row.title, row.jira.as_deref(), &records[..end], upto, &repos).trim_end().to_owned());
    }
    if end < records.len() {
        let more = records.len() - end;
        let before = records[end].0 - 1;
        parts.push(format!("and {more} more: cpd context show --context {context} --since {before}"));
    }
    Ok(parts.join("\n\n"))
}

/// A Manager's first message (commands.md §3.10): its header, then "the Context" as [`first_message`] renders it.
/// The one rendering `context manager` and `session first-message` share.
pub(crate) fn manager_message(conn: &Connection, home: &CpdHome, machine: &MachineName, context: &ContextId, title: &str) -> Result<String, CpdError> {
    let text = first_message(conn, home, machine, context, None)?;
    Ok(format!("You are the Manager of Context {context} \"{title}\". Follow the cpd-manager skill.\n\n{text}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{By, Draft};

    fn id(s: &str) -> RecordId {
        RecordId::parse(s).unwrap()
    }

    fn ids(list: &[&str]) -> Vec<RecordId> {
        list.iter().map(|s| id(s)).collect()
    }

    fn record(seq: i64, id: &str, draft: Draft) -> Indexed {
        let at = format!("2026-10-{:02}T10:00:00.000Z", seq);
        (seq, Record { id: RecordId::parse(id).unwrap(), at, by: By { session: None, plan: None }, text: format!("Text of {id}."), draft })
    }

    fn decision(seq: i64, id: &str) -> Indexed {
        record(seq, id, Draft::Decision { supersedes: None })
    }

    /// What a digest of `records` (all in force) expects.
    fn expect(records: &[Indexed]) -> Expect {
        Expect::of(&records.iter().collect::<Vec<_>>())
    }

    /// One of each kind in force: a decision, a note, an open question, an answered question and its answer.
    fn mixed() -> Expect {
        expect(&[
            decision(1, "hs-d1"),
            record(2, "hs-n1", Draft::Note),
            record(3, "hs-q1", Draft::Question),
            record(4, "hs-q2", Draft::Question),
            record(5, "hs-a1", Draft::Answer { question: id("hs-q2") }),
        ])
    }

    fn invalid(text: &str, expect: &Expect) -> Invalid {
        validate(text.as_bytes(), expect, LIMIT).unwrap_err()
    }

    #[test]
    fn decisions_and_open_questions_cited_in_the_topic_sections_make_a_valid_digest() {
        let e = mixed();
        assert_eq!(validate(b"One engine [hs-d1]. Still open [hs-q1].", &e, LIMIT), Ok(()));
    }

    #[test]
    fn notes_answered_questions_and_answers_need_no_citation_but_may_be_cited() {
        let e = mixed();
        assert_eq!(validate(b"[hs-d1] [hs-q1]", &e, LIMIT), Ok(()));
        assert_eq!(validate(b"[hs-d1, hs-q1, hs-n1] and [hs-q2] answered by [hs-a1]", &e, LIMIT), Ok(()));
    }

    #[test]
    fn a_decision_or_open_question_the_topic_sections_do_not_cite_is_missing() {
        let e = expect(&[decision(1, "hs-d7"), decision(2, "hs-d9"), decision(3, "hs-d8"), record(4, "hs-q1", Draft::Question)]);
        let i = invalid("Only [hs-d7].", &e);
        assert_eq!(i.missing, ids(&["hs-d8", "hs-d9", "hs-q1"]));
        assert!(i.stray.is_empty());
    }

    #[test]
    fn a_decision_cited_only_in_the_index_is_missing() {
        // Only the topic sections count: what follows `## Record index` is cpd's.
        let file = "## Topic\nNo citations here.\n\n## Record index\n\n- [hs-d1] decision (2026-10-01)\n";
        let e = expect(&[decision(1, "hs-d1")]);
        assert_eq!(invalid(topic_sections(file), &e).missing, ids(&["hs-d1"]));
    }

    #[test]
    fn a_cited_id_that_is_not_in_force_is_stray() {
        let e = expect(&[decision(1, "hs-d7")]);
        let i = invalid("Keep [hs-d7]; drop [hs-d1] and [hs-p2, hs-d1]; [hs-d99].", &e);
        assert!(i.missing.is_empty());
        assert_eq!(i.stray, ids(&["hs-d1", "hs-d99", "hs-p2"]));
    }

    #[test]
    fn missing_and_stray_are_reported_together() {
        let i = invalid("[hs-d1]", &expect(&[decision(1, "hs-d2")]));
        assert_eq!((i.missing, i.stray), (ids(&["hs-d2"]), ids(&["hs-d1"])));
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
        let e = expect(&[decision(1, "hs-d1")]);
        assert_eq!(validate(b"[hs-d1] and again [hs-d1], [hs-d1, hs-d1]", &e, LIMIT), Ok(()));
    }

    #[test]
    fn the_index_lists_every_record_in_force_oldest_first() {
        let e = mixed();
        assert_eq!(
            e.index,
            "- [hs-d1] decision (2026-10-01)\n- [hs-n1] note (2026-10-02)\n- [hs-q1] question (2026-10-03)\n\
             - [hs-q2] question (2026-10-04)\n- [hs-a1] answer (2026-10-05)\n"
        );
    }

    #[test]
    fn the_size_limit_covers_the_topic_sections_and_the_index_and_is_inclusive() {
        let e = expect(&[decision(1, "hs-d1")]);
        let overhead = compose(b"", &e.index).len();
        let cite = "[hs-d1]";
        let room = LIMIT - overhead - cite.len();
        let at_limit = format!("{cite}{}", "é".repeat(room / 2)) + &"x".repeat(room % 2);
        assert_eq!(validate(at_limit.as_bytes(), &e, LIMIT), Ok(()));
        let i = invalid(&format!("{at_limit}x"), &e);
        assert_eq!((i.too_big, i.bytes, i.limit), (true, LIMIT + 1, LIMIT));
        assert_eq!(validate(b"[hs-d1] is long", &e, 14).unwrap_err().too_big, true);
    }

    #[test]
    fn a_digest_that_is_not_utf8_is_invalid() {
        let mut bytes = b"[hs-d1] ".to_vec();
        bytes.push(0xff);
        let i = validate(&bytes, &expect(&[decision(1, "hs-d1")]), LIMIT).unwrap_err();
        assert!(i.not_utf8 && !i.too_big && i.missing.is_empty());
    }

    #[test]
    fn nothing_in_force_needs_no_citation() {
        let e = expect(&[]);
        assert_eq!(validate(b"", &e, LIMIT), Ok(()));
        assert_eq!(invalid("[hs-d1]", &e).stray, ids(&["hs-d1"]));
    }

    #[test]
    fn the_topic_sections_end_at_the_last_index_heading() {
        assert_eq!(topic_sections("# A\ntext\n\n## Record index\n\n- [hs-d1] x\n"), "# A\ntext\n\n");
        assert_eq!(topic_sections("# A\n## Record index\nmodel's own\n\n## Record index\n- cpd's\n"), "# A\n## Record index\nmodel's own\n\n");
        assert_eq!(topic_sections("# A\nno index\n"), "# A\nno index\n");
        assert_eq!(topic_sections("# A\nsee ## Record index in text\n"), "# A\nsee ## Record index in text\n");
    }

    #[test]
    fn composing_a_digest_trims_the_topic_and_appends_the_index_once() {
        let file = compose(b"# A\n\n[hs-d1]\n\n\n", "- [hs-d1] decision (2026-10-01)\n");
        let text = String::from_utf8(file).unwrap();
        assert_eq!(text, "# A\n\n[hs-d1]\n\n## Record index\n\n- [hs-d1] decision (2026-10-01)\n");
        assert_eq!(topic_sections(&text).trim_end(), "# A\n\n[hs-d1]");
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
    fn install_appends_the_index_renames_the_tmp_and_leaves_one_digest() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        let d = home.context_digest_dir(&ctx());
        fs::create_dir_all(&d).unwrap();
        for name in ["5.md", "7.md.tmp", "7.md.invalid", "8.md.tmp", "prompt.md", "records.md", "distill.log"] {
            fs::write(d.join(name), name).unwrap();
        }
        fs::write(home.context_digest_tmp(&ctx(), 8), "[hs-d1]\n").unwrap();
        let e = expect(&[decision(1, "hs-d1")]);
        assert_eq!(install(&home, &ctx(), 8, &e).unwrap(), home.context_digest(&ctx(), 8));
        let mut left: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["8.md", "distill.log", "prompt.md", "records.md"]);
        let text = latest(&home, &ctx()).unwrap().unwrap().text;
        assert_eq!(text, "[hs-d1]\n\n## Record index\n\n- [hs-d1] decision (2026-10-01)\n");
    }

    #[test]
    fn keep_invalid_renames_the_tmp_replaces_the_previous_invalid_and_leaves_the_valid_digest() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        let d = home.context_digest_dir(&ctx());
        fs::create_dir_all(&d).unwrap();
        for name in ["5.md", "6.md.invalid", "9.md.tmp", "prompt.md"] {
            fs::write(d.join(name), name).unwrap();
        }
        assert_eq!(keep_invalid(&home, &ctx(), 9).unwrap(), d.join("9.md.invalid"));
        assert_eq!(invalid_file(&home, &ctx(), 9), d.join("9.md.invalid"));
        let mut left: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, ["5.md", "9.md.invalid", "prompt.md"]);
        assert_eq!(fs::read_to_string(d.join("9.md.invalid")).unwrap(), "9.md.tmp");
        assert_eq!(latest(&home, &ctx()).unwrap().unwrap().upto_seq, 5, "the invalid digest is never the latest");
    }

    #[test]
    fn install_without_a_tmp_fails_and_keeps_the_previous_digest() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(dir.path());
        fs::create_dir_all(home.context_digest_dir(&ctx())).unwrap();
        fs::write(home.context_digest(&ctx(), 5), "old").unwrap();
        assert_eq!(install(&home, &ctx(), 8, &expect(&[])).unwrap_err().code(), ErrorCode::FileUnreadable);
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
