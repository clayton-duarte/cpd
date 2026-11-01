//! `ls-remote`, `fetch` and `push`: a remote is a configured url, rewritten by `insteadOf`, that names a repo
//! of the Universe; a url that names none cannot be reached (`Could not resolve host`, or for a path "does not
//! appear to be a git repository").
//!
//! `fetch` and `push` copy the closure of the commit they move into the object set of the repo that did not
//! have it (`Universe::copy`), and a successful one updates `refs/remotes/<remote>/<branch>` here, as the
//! default refspec `+refs/heads/*:refs/remotes/<remote>/*` does (the fixtures leave `remote.<name>.fetch`
//! unset, which is read as that default).

use super::*;

/// A remote of a call, found.
struct Remote {
    /// `Some(name)` for a configured remote; `None` for a url or a path given in its place.
    name: Option<String>,
    /// The url git talks to: as configured, after `insteadOf`.
    url: String,
    repo: RepoId,
}

/// The remote `name` of the repo `repo`, or the answer of a call that cannot reach it.
fn open(c: &Ctx, repo: RepoId, name: &str) -> Result<Remote, Out> {
    let configured = remote_urls(&c.u.config_entries(repo, &c.extra))
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, u)| u);
    let (named, url) = match configured {
        Some(url) => (true, url),
        None if name.contains('/') => (false, name.to_owned()),
        None => {
            return Err(Out::fail(
                128,
                format!(
                    "fatal: '{name}' does not appear to be a git repository\nfatal: Could not read from remote repository.\n"
                ),
            ));
        }
    };
    let url = c.u.resolve_url_with(repo, &url, &c.extra);
    match c.u.repo_at(&url) {
        Some(there) => Ok(Remote {
            name: named.then(|| name.to_owned()),
            url,
            repo: there,
        }),
        None if url.starts_with('/') || url.starts_with('.') => Err(Out::fail(
            128,
            format!(
                "fatal: '{url}' does not appear to be a git repository\nfatal: Could not read from remote repository.\n"
            ),
        )),
        None => Err(Out::fail(
            128,
            format!("fatal: unable to access '{url}': Could not resolve host\n"),
        )),
    }
}

/// `main` as `refs/heads/main`; a full name as it is.
fn full_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

/// The first seven hex digits: how git abbreviates an id in a small repo.
fn abbrev(oid: &Oid) -> &str {
    &oid.as_str()[..7]
}

pub(super) fn ls_remote(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("ls-remote", args, &["--heads", "--exit-code"], &[])?;
    let flags = flags(&toks);
    let pos = positionals(&toks);
    let Some((remote, patterns)) = pos.split_first() else {
        return Err("ls-remote (arguments)".to_owned());
    };
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let there = match open(c, repo, remote) {
        Ok(remote) => remote.repo,
        Err(out) => return Ok(out),
    };
    let mut refs: Vec<(String, Oid)> = Vec::new();
    if !flags.contains(&"--heads") {
        let head = match c.u.head(&c.u.git_dir(there)) {
            Head::Branch(b) => c.u.ref_oid(there, &format!("refs/heads/{b}")),
            Head::Detached(oid) => Some(oid),
        };
        refs.extend(head.map(|o| ("HEAD".to_owned(), o)));
    }
    refs.extend(c.u.refs(there, "refs/heads/"));
    if !flags.contains(&"--heads") {
        refs.extend(c.u.refs(there, "refs/tags/"));
    }
    let shown: Vec<&(String, Oid)> = refs
        .iter()
        .filter(|(name, _)| {
            patterns.is_empty()
                || patterns
                    .iter()
                    .any(|p| name == p || name.ends_with(&format!("/{p}")))
        })
        .collect();
    if shown.is_empty() && flags.contains(&"--exit-code") {
        return Ok(Out::silent(2));
    }
    Ok(Out::ok(
        shown
            .iter()
            .map(|(n, o)| format!("{o}\t{n}\n"))
            .collect::<String>(),
    ))
}

/// `fetch [<remote> [<branch>...]]`: the closure of each branch tip, and `refs/remotes/<remote>/<branch>`.
/// Local branches, `HEAD` and the working tree are left alone.
pub(super) fn fetch(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan("fetch", args, &["-q", "--quiet"], &[])?;
    let quiet = !flags(&toks).is_empty();
    let pos = positionals(&toks);
    let (name, branches) = pos
        .split_first()
        .map_or(("origin", &[][..]), |(n, b)| (*n, b));
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let remote = match open(c, repo, name) {
        Ok(remote) => remote,
        Err(out) => return Ok(out),
    };
    let tips: Vec<(String, Oid)> = if branches.is_empty() {
        c.u.refs(remote.repo, "refs/heads/")
    } else {
        let mut tips = Vec::new();
        for branch in branches {
            let reference = full_ref(branch);
            match c.u.ref_oid(remote.repo, &reference) {
                Some(oid) => tips.push((reference, oid)),
                None => {
                    return Ok(Out::fail(
                        128,
                        format!("fatal: couldn't find remote ref {branch}\n"),
                    ));
                }
            }
        }
        tips
    };
    let given = !branches.is_empty();
    let mut from = format!(
        "From {}\n",
        remote.url.trim_end_matches('/').trim_end_matches(".git")
    );
    for (reference, oid) in tips {
        c.u.copy(remote.repo, repo, &oid);
        track(c.u, repo, &remote, &reference, Some(&oid));
        from.push_str(&format!(
            " * branch            {:<10} -> FETCH_HEAD\n",
            reference.strip_prefix("refs/heads/").unwrap_or(&reference)
        ));
    }
    // Naming branches writes `FETCH_HEAD` and says so; fetching what the refspec says is silent here.
    Ok(if given && !quiet {
        Out::fail(0, from)
    } else {
        Out::ok("")
    })
}

/// Points `refs/remotes/<remote>/<branch>` of `repo` at `oid` (`None`: deletes it) for a branch `reference` of
/// the remote; nothing for another kind of ref or for a url that is no configured remote.
fn track(u: &mut Universe, repo: RepoId, remote: &Remote, reference: &str, oid: Option<&Oid>) {
    let (Some(name), Some(branch)) = (&remote.name, reference.strip_prefix("refs/heads/")) else {
        return;
    };
    let tracking = format!("refs/remotes/{name}/{branch}");
    match oid {
        Some(oid) if u.ref_oid(repo, &tracking).as_ref() != Some(oid) => {
            u.set_ref(repo, &tracking, oid, "fetch")
        }
        Some(_) => {}
        None => u.delete_ref(repo, &tracking),
    }
}

/// A `--force-with-lease=<ref>:<expected>`: `expected` empty means the ref must not exist.
struct Lease {
    reference: String,
    expected: String,
}

/// What a push did to one ref.
struct Update {
    flag: char,
    /// What the line shows as the source: the ref's full name for `--porcelain`, as given otherwise.
    from: String,
    to: String,
    summary: String,
    reason: Option<&'static str>,
}

impl Update {
    fn rejected(from: String, to: String, reason: &'static str) -> Update {
        Update {
            flag: '!',
            from,
            to,
            summary: "[rejected]".to_owned(),
            reason: Some(reason),
        }
    }
}

/// `push [--porcelain] [--delete] [--force-with-lease=<ref>:<expected>] <remote> <refspec>...`, a refspec
/// being `<src>[:<dst>]` (a branch name or a commit as `<src>`, a branch or a full ref as `<dst>`), or with
/// `--delete` a branch. The remote must be a bare repo.
pub(super) fn push(c: &mut Ctx, args: &[String]) -> Run {
    let toks = scan(
        "push",
        args,
        &["--porcelain", "--delete", "-q", "-u"],
        &["--force-with-lease="],
    )?;
    let flags = flags(&toks);
    let (porcelain, delete) = (flags.contains(&"--porcelain"), flags.contains(&"--delete"));
    let (quiet, upstream) = (flags.contains(&"-q"), flags.contains(&"-u"));
    let mut leases = Vec::new();
    for flag in flags
        .iter()
        .filter(|f| f.starts_with("--force-with-lease="))
    {
        let lease = flag["--force-with-lease=".len()..]
            .split_once(':')
            .ok_or_else(|| format!("push {flag}"))?;
        leases.push(Lease {
            reference: lease.0.to_owned(),
            expected: lease.1.to_owned(),
        });
    }
    let pos = positionals(&toks);
    let Some((name, specs)) = pos.split_first() else {
        return Err("push (arguments)".to_owned());
    };
    if specs.is_empty() {
        return Err("push (without a refspec)".to_owned());
    }
    if let Some(spec) = specs.iter().find(|s| s.starts_with('+')) {
        return Err(format!("push {spec}"));
    }
    let Some(repo) = c.repo() else {
        return Ok(Out::fail(128, NOT_A_REPO));
    };
    let remote = match open(c, repo, name) {
        Ok(remote) => remote,
        Err(out) => return Ok(out),
    };
    if !c.u.is_bare(remote.repo) {
        return Err("push (to a non-bare remote)".to_owned());
    }
    let failed = format!("error: failed to push some refs to '{}'\n", remote.url);
    let mut updates = Vec::new();
    for spec in specs {
        if delete {
            let to = full_ref(spec);
            if c.u.ref_oid(remote.repo, &to).is_none() {
                return Ok(Out::fail(
                    1,
                    format!(
                        "error: unable to delete '{spec}': remote ref does not exist\n{failed}"
                    ),
                ));
            }
            c.u.delete_ref(remote.repo, &to);
            track(c.u, repo, &remote, &to, None);
            updates.push(Update {
                flag: '-',
                from: String::new(),
                to,
                summary: "[deleted]".to_owned(),
                reason: None,
            });
            continue;
        }
        let (src, dst) = spec
            .split_once(':')
            .map_or((*spec, None), |(s, d)| (s, Some(d)));
        let Some(new) = c.parse(src).ok().filter(|_| !src.is_empty()) else {
            return Ok(Out::fail(
                1,
                format!("error: src refspec {src} does not match any\n{failed}"),
            ));
        };
        let to = match (dst, src) {
            (Some(dst), _) => full_ref(dst),
            (None, "HEAD") => match c.u.head(&c.cwd) {
                Head::Branch(b) => format!("refs/heads/{b}"),
                Head::Detached(_) => return Err("push HEAD (detached)".to_owned()),
            },
            (None, branch) => full_ref(branch),
        };
        let from = if porcelain {
            c.u.resolve_ref(repo, &full_ref(src))
                .map_or(src.to_owned(), |_| full_ref(src))
        } else {
            src.to_owned()
        };
        let old = c.u.ref_oid(remote.repo, &to);
        let lease = leases.iter().find(|l| l.reference == to);
        let update = if old.as_ref() == Some(&new) {
            Update {
                flag: '=',
                from,
                to: to.clone(),
                summary: "[up to date]".to_owned(),
                reason: None,
            }
        } else if lease.is_some_and(|l| old.as_ref().map_or("", Oid::as_str) != l.expected) {
            Update::rejected(from, to.clone(), "stale info")
        } else {
            match &old {
                None => Update {
                    flag: '*',
                    from,
                    to: to.clone(),
                    summary: "[new branch]".to_owned(),
                    reason: None,
                },
                Some(old) => {
                    let held = c.u.has(repo, old);
                    let fast_forward = held && c.u.is_ancestor(repo, old, &new).unwrap_or(false);
                    if fast_forward {
                        Update {
                            flag: ' ',
                            from,
                            to: to.clone(),
                            summary: format!("{}..{}", abbrev(old), abbrev(&new)),
                            reason: None,
                        }
                    } else if lease.is_some() {
                        let summary = format!("{}...{}", abbrev(old), abbrev(&new));
                        Update {
                            flag: '+',
                            from,
                            to: to.clone(),
                            summary,
                            reason: Some("forced update"),
                        }
                    } else {
                        Update::rejected(
                            from,
                            to.clone(),
                            if held {
                                "non-fast-forward"
                            } else {
                                "fetch first"
                            },
                        )
                    }
                }
            }
        };
        if matches!(update.flag, '*' | ' ' | '+' | '=') {
            c.u.copy(repo, remote.repo, &new);
            if update.flag != '=' {
                c.u.set_ref(remote.repo, &to, &new, "push");
            }
            if let (Some(name), Some(branch)) = (&remote.name, to.strip_prefix("refs/heads/")) {
                c.u.set_ref(
                    repo,
                    &format!("refs/remotes/{name}/{branch}"),
                    &new,
                    "update by push",
                );
                let local = match (src, c.u.head(&c.cwd)) {
                    ("HEAD", Head::Branch(b)) => Some(b),
                    (s, _) if c.u.ref_oid(repo, &format!("refs/heads/{s}")).is_some() => {
                        Some(s.to_owned())
                    }
                    _ => None,
                };
                if let (true, Some(local)) = (upstream, local) {
                    c.u.config_set(repo, &format!("branch.{local}.remote"), name);
                    c.u.config_set(repo, &format!("branch.{local}.merge"), &to);
                }
            }
        }
        updates.push(update);
    }
    let rejected = updates.iter().any(|u| u.flag == '!');
    let shown = |u: &Update| match (porcelain, u.reason) {
        (true, Some(reason)) => format!(
            "{}\t{}:{}\t{} ({reason})\n",
            u.flag, u.from, u.to, u.summary
        ),
        (true, None) => format!("{}\t{}:{}\t{}\n", u.flag, u.from, u.to, u.summary),
        (false, _) => shown_plain(u),
    };
    let to_url = format!("To {}\n", remote.url);
    let code = i32::from(rejected);
    let tail = if rejected { failed } else { String::new() };
    if porcelain {
        // The route's own, not recorded from git: after git's `error:` line, the rejected refs are also told on
        // stderr in the plain form (with the reason), which is what the product keeps of a rejected push
        // (`Push::Rejected`).
        let rejections: String = updates
            .iter()
            .filter(|u| u.flag == '!')
            .map(shown_plain)
            .collect();
        let lines: String = updates.iter().map(shown).collect();
        return Ok(Out {
            code,
            stdout: format!("{to_url}{lines}Done\n"),
            stderr: format!("{tail}{rejections}"),
        });
    }
    if quiet && !rejected {
        return Ok(Out::silent(0));
    }
    if updates.iter().all(|u| u.flag == '=') {
        return Ok(Out::fail(0, "Everything up-to-date\n"));
    }
    let lines: String = updates.iter().map(shown).collect();
    Ok(Out {
        code,
        stdout: String::new(),
        stderr: format!("{to_url}{lines}{tail}"),
    })
}

/// The line of a plain push: ` <flag> <summary> <from> -> <to> (<reason>)`, the summary padded to 17.
fn shown_plain(u: &Update) -> String {
    let to = shorten(&u.to);
    let what = if u.flag == '-' {
        to.to_owned()
    } else {
        format!("{} -> {to}", shorten(&u.from))
    };
    let reason = u.reason.map_or(String::new(), |r| format!(" ({r})"));
    format!(" {} {:<17} {what}{reason}\n", u.flag, u.summary)
}
