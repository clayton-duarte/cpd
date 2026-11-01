//! Keying a repo from its `origin` URL (contract §8.3 "Validation", hs-d288).
//!
//! The URL is parsed once, at the boundary, into an owner and a name. Only
//! `https://github.com/<owner>/<name>` and `git@github.com:<owner>/<name>`, each
//! with or without `.git`, key a repo: no other host, no SSH host alias, no
//! network call.

use std::path::Path;

use super::repo_unkeyed;
use crate::error::CpdError;
use crate::git::run::origin_url;
use crate::ids::{OwnerLogin, RepoName};

/// The owner and name of a GitHub URL; `None` for any other URL or invalid owner or name.
pub fn parse_origin(url: &str) -> Option<(OwnerLogin, RepoName)> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if name.contains('/') {
        return None;
    }
    Some((OwnerLogin::parse(owner).ok()?, RepoName::parse(name).ok()?))
}

/// The policy key's owner and name of the repo at `repo`, from its `origin`
/// (`repo_unkeyed` when it has none, or one that does not key it).
pub fn repo_key(repo: &Path) -> Result<(OwnerLogin, RepoName), CpdError> {
    let url = origin_url(repo)?;
    if url.is_none() {
        // No remote: clayton-duarte's, named by the directory (contract §8.3).
        let name = repo
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| RepoName::parse(n).ok());
        return match name {
            Some(name) => Ok((
                OwnerLogin::parse(super::FALLBACK_OWNER).expect("a valid login"),
                name,
            )),
            None => Err(repo_unkeyed(None)),
        };
    }
    url.as_deref()
        .and_then(parse_origin)
        .ok_or_else(|| repo_unkeyed(url.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn key(url: &str) -> Option<(String, String)> {
        parse_origin(url).map(|(o, n)| (o.to_string(), n.to_string()))
    }

    fn some(o: &str, n: &str) -> Option<(String, String)> {
        Some((o.to_owned(), n.to_owned()))
    }

    #[test]
    fn the_four_github_forms_key_a_repo() {
        assert_eq!(key("https://github.com/acme/cpd"), some("acme", "cpd"));
        assert_eq!(key("https://github.com/acme/cpd.git"), some("acme", "cpd"));
        assert_eq!(key("git@github.com:acme/cpd"), some("acme", "cpd"));
        assert_eq!(key("git@github.com:acme/cpd.git"), some("acme", "cpd"));
    }

    #[test]
    fn owner_and_name_are_lowercased() {
        assert_eq!(
            key("https://github.com/Clayton-Duarte/My.Repo_X.git"),
            some("clayton-duarte", "my.repo_x")
        );
        assert_eq!(key("git@github.com:ACME/CPD"), some("acme", "cpd"));
    }

    #[test]
    fn a_name_with_dots_keeps_all_but_one_git_suffix() {
        assert_eq!(key("https://github.com/acme/a.b.git"), some("acme", "a.b"));
        assert_eq!(
            key("https://github.com/acme/a.git.git"),
            some("acme", "a.git")
        );
    }

    #[test]
    fn anything_else_is_refused() {
        for url in [
            "git@github-work:acme/x.git",
            "git@gitlab.com:acme/x.git",
            "https://gitlab.com/acme/x",
            "https://github.com.evil.com/acme/x",
            "http://github.com/acme/x",
            "ssh://git@github.com/acme/x.git",
            "https://user@github.com/acme/x",
            "https://github.com/acme",
            "https://github.com/acme/",
            "https://github.com//x",
            "https://github.com/acme/x/",
            "https://github.com/acme/x/y",
            "https://github.com/-bad/x",
            "https://github.com/a--b/x",
            "https://github.com/acme/a b",
            "https://github.com/acme/.git",
            "git@github.com:acme",
            "/tmp/repos/origin.git",
            "file:///tmp/acme/x.git",
            "",
        ] {
            assert_eq!(key(url), None, "{url}");
        }
    }

    #[test]
    fn repo_key_reads_origin_and_refuses_with_the_url() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("my-repo");
        std::fs::create_dir(&dir).unwrap();
        let git = |args: &[&str]| {
            let st = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(st.success());
        };
        git(&["init", "-q"]);
        // No origin: clayton-duarte's, named by the directory.
        let (o, n) = repo_key(&dir).unwrap();
        assert_eq!((o.as_str(), n.as_str()), ("clayton-duarte", "my-repo"));
        git(&["remote", "add", "origin", "git@github-work:acme/x.git"]);
        let bad = repo_key(&dir).unwrap_err();
        assert_eq!((bad.code(), bad.exit().code()), (ErrorCode::RepoUnkeyed, 1));
        assert_eq!(bad.details()["url"], "git@github-work:acme/x.git");
        git(&["remote", "set-url", "origin", "git@github.com:Acme/X.git"]);
        let (o, n) = repo_key(&dir).unwrap();
        assert_eq!((o.as_str(), n.as_str()), ("acme", "x"));
    }
}
