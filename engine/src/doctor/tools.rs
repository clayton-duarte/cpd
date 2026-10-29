//! The external tools (commands.md §14.1, "Minimum versions"): git, tmux, pi
//! and gh at or above the minimum, `gh auth status`, and pi's Node.

use std::ffi::OsStr;
use std::process::{Command, Stdio};

use super::check::{Check, Severity};
use cpd_core::config::catalog::node;
use cpd_core::host::deadline::Failed;
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;

/// A tool with its minimum, and the first version it is not allowed to be.
struct Tool {
    name: &'static str,
    args: &'static [&'static str],
    /// How many dotted numbers the version has: 3 for `x.y.z`, 2 for `x.y`.
    parts: usize,
    min: [u64; 3],
    below: Option<[u64; 3]>,
    expected: &'static str,
}

const TOOLS: [Tool; 4] = [
    Tool { name: "git", args: &["--version"], parts: 3, min: [2, 43, 0], below: None, expected: ">= 2.43.0" },
    Tool { name: "tmux", args: &["-V"], parts: 2, min: [3, 6, 0], below: None, expected: ">= 3.6" },
    Tool { name: "pi", args: &["--version"], parts: 3, min: [1, 2, 0], below: Some([2, 0, 0]), expected: ">= 1.2.0 < 2.0.0" },
    Tool { name: "gh", args: &["--version"], parts: 3, min: [2, 45, 0], below: None, expected: ">= 2.45.0" },
];

/// The checks `git`, `tmux`, `pi`, `gh`, in that order.
pub fn tools() -> Vec<Check> {
    tools_with(&*Ports::current().runner)
}

/// [`tools`] on `runner`.
pub fn tools_with(runner: &dyn Runner) -> Vec<Check> {
    TOOLS.iter().map(|tool| check(runner, tool)).collect()
}

/// The first dotted number run of `parts` or more numbers in `text`, cut to
/// `parts` numbers (`v1.2.3` and `3.4a` parse; a letter suffix is ignored).
pub(super) fn parse_version(text: &str, parts: usize) -> Option<(String, [u64; 3])> {
    text.split_whitespace().find_map(|token| {
        let run: String = token.trim_start_matches('v').chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let numbers = run.trim_end_matches('.').split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok()?;
        if numbers.len() < parts {
            return None;
        }
        let mut version = [0; 3];
        version[..parts].copy_from_slice(&numbers[..parts]);
        let text = numbers[..parts].iter().map(u64::to_string).collect::<Vec<_>>().join(".");
        Some((text, version))
    })
}

/// stdout then stderr of `program args`, or why it did not run and succeed. A probe
/// past doctor's deadline ([`super::DEADLINE`]) is killed with its group and fails `tool_timeout` (commands.md §14.1).
pub(super) fn run_tool(runner: &dyn Runner, program: &OsStr, args: &[&str]) -> Result<String, String> {
    let name = program.to_string_lossy();
    let tool = std::path::Path::new(program).file_name().map_or(name.clone(), |n| n.to_string_lossy());
    // tmux is probed as every cpd call of it is: a UTF-8 client (hs-d463).
    let mut cmd = if tool == "tmux" { cpd_core::host::tmux::client(program) } else { Command::new(program) };
    cmd.args(args).stdin(Stdio::null());
    let out = runner.output(&mut cmd, &tool, super::deadline()).map_err(|e| match e {
        Failed::Io(e) if e.kind() == std::io::ErrorKind::NotFound => format!("`{name}` was not found."),
        Failed::Io(e) => format!("cannot run `{name}`: {e}."),
        Failed::Timeout(e) => format!("{}: {}", e.code().as_str(), e.message()),
    })?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        let first = text.lines().next().unwrap_or("no output");
        return Err(format!("`{name} {}` exited {}: {first}", args.join(" "), out.status.code().map_or("by a signal".into(), |c| c.to_string())));
    }
    Ok(text)
}

fn check(runner: &dyn Runner, tool: &Tool) -> Check {
    let fail = |message: String| Check::fail(tool.name, Severity::Error, message).expected(tool.expected);
    let text = match run_tool(runner, OsStr::new(tool.name), tool.args) {
        Ok(text) => text,
        Err(message) => return fail(message),
    };
    // tmux's development builds are `next-3.5` and the like: they pass.
    if tool.name == "tmux"
        && let Some(next) = text.split_whitespace().find(|t| t.starts_with("next-"))
    {
        return Check::pass(tool.name, Severity::Error).found(next).expected(tool.expected);
    }
    let Some((found, version)) = parse_version(&text, tool.parts) else {
        let first = text.lines().next().unwrap_or("no output");
        return fail(format!("no version in the output of `{} {}`: {first}", tool.name, tool.args.join(" ")));
    };
    let ok = version >= tool.min && tool.below.is_none_or(|below| version < below);
    let check = Check::new(tool.name, Severity::Error, ok).found(found).expected(tool.expected);
    if ok { check } else { check.message(format!("{} is outside the versions cpd 2 was verified with.", tool.name)) }
}

/// `gh auth status` exits 0 (warn: only the `pr` commands need it).
pub fn gh_auth() -> Check {
    gh_auth_with(&*Ports::current().runner)
}

/// [`gh_auth`] on `runner`.
pub fn gh_auth_with(runner: &dyn Runner) -> Check {
    match run_tool(runner, OsStr::new("gh"), &["auth", "status"]) {
        Ok(_) => Check::pass("gh_auth", Severity::Warn),
        Err(message) => Check::fail("gh_auth", Severity::Warn, message),
    }
}

/// pi's Node (contract §9) runs `--version`.
pub fn pi_node() -> Check {
    pi_node_with(&*Ports::current().runner)
}

/// [`pi_node`] on `runner`.
pub fn pi_node_with(runner: &dyn Runner) -> Check {
    let node = node();
    match run_tool(runner, node.as_os_str(), &["--version"]) {
        Ok(text) => Check::pass("pi_node", Severity::Error).found(text.trim()).message(format!("ran {}", node.display())),
        Err(message) => Check::fail("pi_node", Severity::Error, message),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    fn v(text: &str, parts: usize) -> Option<String> {
        parse_version(text, parts).map(|(t, _)| t)
    }

    #[test]
    fn the_first_dotted_run_of_enough_numbers() {
        assert_eq!(v("git version 2.43.0", 3).as_deref(), Some("2.43.0"));
        assert_eq!(v("git version 2.39.5 (Apple Git-154)", 3).as_deref(), Some("2.39.5"));
        assert_eq!(v("gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)", 3).as_deref(), Some("2.45.0"));
        assert_eq!(v("1.2.0\n", 3).as_deref(), Some("1.2.0"));
        assert_eq!(v("v22.23.2", 3).as_deref(), Some("22.23.2"));
        assert_eq!(v("tmux 3.4", 2).as_deref(), Some("3.4"));
        assert_eq!(v("tmux 3.4a", 2).as_deref(), Some("3.4"));
        assert_eq!(v("tmux 3.4.1", 2).as_deref(), Some("3.4"));
    }

    #[test]
    fn text_without_enough_numbers_has_no_version() {
        assert_eq!(v("", 3), None);
        assert_eq!(v("git version 2.43", 3), None);
        assert_eq!(v("tmux master", 2), None);
        assert_eq!(v("tmux 3", 2), None);
    }
}
