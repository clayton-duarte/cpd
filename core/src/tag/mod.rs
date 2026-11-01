//! `cpd tag` (docs/commands.md §26.1): tags the commit of `origin/main` that last changed the workspace version of
//! `Cargo.toml` with the annotated tag `v<version>` and pushes it. Git runs through `ports.runner`.

use std::path::Path;
use std::time::Duration;

use crate::host::runner::Call;
use crate::ports::Ports;

/// Long enough for a fetch or a push over a slow link.
const GIT_DEADLINE: Duration = Duration::from_secs(300);

/// `git -C root args`: `Some(stdout)` when it exited 0, `None` when it failed or could not run.
fn git(ports: &Ports, root: &Path, args: &[&str]) -> Option<String> {
    let mut argv = vec!["-C".to_owned(), root.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| (*a).to_owned()));
    let call = Call {
        tool: "git".to_owned(),
        program: "git".to_owned(),
        args: argv,
        envs: Vec::new(),
        cwd: None,
        deadline: GIT_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    let out = ports.runner.run(&call).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The first `version = "` line from `[workspace.package]` on, as the script's `sed` range read it.
fn workspace_version(toml: &str) -> String {
    let mut lines = toml
        .lines()
        .skip_while(|l| !l.starts_with("[workspace.package]"));
    lines
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("")
        .to_owned()
}

fn first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or("")
}

/// Runs the steps of §26.1 in the git repository `root`. `Ok` is the one `tag-version: ...` line of a success
/// (`tagged v<version> on <sha>` or `v<version> exists; nothing to do`), `Err` the one `tag-version: ...` line of
/// a failure; neither ends in a newline. The streams and the exit code are the command's.
pub fn tag(ports: &Ports, root: &Path) -> Result<String, String> {
    let die = |why: String| Err(format!("tag-version: {why}"));
    let g = |args: &[&str]| git(ports, root, args);

    if g(&["fetch", "--quiet", "origin", "main"]).is_none() {
        return die("cannot fetch origin/main".to_owned());
    }
    let Some(toml) = g(&["show", "origin/main:Cargo.toml"]) else {
        return die("cannot read Cargo.toml on origin/main".to_owned());
    };
    let version = workspace_version(&toml);
    if version.is_empty() {
        return die("no workspace version in Cargo.toml on origin/main".to_owned());
    }
    let tag = format!("v{version}");
    let reference = format!("refs/tags/{tag}");

    let Some(remote) = g(&["ls-remote", "--tags", "origin", &reference]) else {
        return die("cannot list the tags of origin".to_owned());
    };
    if !remote.trim().is_empty() {
        return Ok(format!("tag-version: {tag} exists; nothing to do"));
    }

    let Some(log) = g(&[
        "log",
        "-1",
        "--first-parent",
        "-G^version = ",
        "--format=%H",
        "origin/main",
        "--",
        "Cargo.toml",
    ]) else {
        return die(format!("cannot find the commit that set {tag}"));
    };
    let sha = log.trim();
    if sha.is_empty() {
        return die("no commit on origin/main changed the version".to_owned());
    }
    let at = workspace_version(&g(&["show", &format!("{sha}:Cargo.toml")]).unwrap_or_default());
    if at != version {
        return die(format!("commit {sha} has version '{at}', not {version}"));
    }

    if g(&["rev-parse", "-q", "--verify", &reference]).is_some() {
        let have = g(&["rev-parse", &format!("{reference}^{{commit}}")]).unwrap_or_default();
        let have = have.trim();
        if have != sha {
            return die(format!("local tag {tag} points at {have}, not {sha}"));
        }
    } else if g(&["tag", "-a", &tag, sha, "-m", &tag]).is_none() {
        return die(format!("cannot create {tag}"));
    }

    if g(&["push", "--quiet", "origin", &reference]).is_none() {
        let theirs = g(&[
            "ls-remote",
            "--tags",
            "origin",
            &format!("{reference}^{{}}"),
        ])
        .unwrap_or_default();
        if first_word(&theirs) != sha {
            return die(format!("push of {tag} failed"));
        }
    }
    Ok(format!("tag-version: tagged {tag} on {sha}"))
}

#[cfg(test)]
mod tests;
