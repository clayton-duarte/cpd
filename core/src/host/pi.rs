//! How cpd starts pi (contract §6.5, with commands.md §19 amendment 8): the
//! argv and environment per role, the `[-e …]` rule that skips packages pi's
//! settings already declare, and finding a Session's file after start.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{AttemptId, PlanId, SessionId};

use super::tmux::Spawn;

/// The tools a Worker may not use: the Lead's (cpd 1 §12 step 5, kept).
pub const WORKER_EXCLUDED_TOOLS: &str =
    "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox";

/// pi's own directories, read once from the environment the way pi reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiDirs {
    /// The home directory, for `~` in settings paths.
    pub home: PathBuf,
    /// `PI_CODING_AGENT_DIR`, else `~/.pi/agent`.
    pub agent_dir: PathBuf,
    /// `PI_CODING_AGENT_SESSION_DIR` when set.
    pub session_dir: Option<PathBuf>,
}

impl PiDirs {
    /// From this process's environment.
    pub fn from_env() -> Result<PiDirs, CpdError> {
        PiDirs::resolve(
            std::env::home_dir(),
            std::env::var_os("PI_CODING_AGENT_DIR"),
            std::env::var_os("PI_CODING_AGENT_SESSION_DIR"),
        )
    }

    /// Empty variables count as unset and `~` is expanded, as pi does. No home
    /// directory is `usage` (exit 2): the caller fixes its environment.
    pub fn resolve(home: Option<PathBuf>, agent_dir: Option<OsString>, session_dir: Option<OsString>) -> Result<PiDirs, CpdError> {
        let home = home.filter(|h| h.is_absolute()).ok_or_else(|| {
            CpdError::new(ErrorCode::Usage, "The home directory is unknown, so pi's settings cannot be found: set HOME.")
                .with("variable", "HOME")
        })?;
        let set = |v: Option<OsString>| v.filter(|v| !v.is_empty()).map(|v| expand_tilde(&v.to_string_lossy(), &home));
        let agent_dir = set(agent_dir).unwrap_or_else(|| home.join(".pi").join("agent"));
        let session_dir = set(session_dir);
        Ok(PiDirs { home, agent_dir, session_dir })
    }

    /// `<agent dir>/settings.json`: pi's global settings.
    pub fn global_settings(&self) -> PathBuf {
        self.agent_dir.join("settings.json")
    }
}

/// `<cwd>/.pi/settings.json`: pi's project settings.
pub fn project_settings(cwd: &Path) -> PathBuf {
    cwd.join(".pi").join("settings.json")
}

/// A pi command line: `argv[0]` is `pi`; `env` is what cpd adds to the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Launch {
    /// What tmux runs for this launch in `cwd` (`new-window -c <cwd> -e K=V … -- pi …`).
    pub fn spawn(&self, cwd: &Path) -> Spawn {
        Spawn { cwd: Some(cwd.to_path_buf()), env: self.env.clone(), argv: self.argv.clone() }
    }
}

/// How a Lead starts (contract §6.5, §6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeadStart {
    /// A new session with its first message.
    Fresh { model: String, name: String, first_message: String },
    /// A fork of `source_file`; the grounded Lead has no first message.
    Fork { model: String, name: String, source_file: PathBuf, first_message: Option<String> },
    /// The Session's own file again, with no first message.
    Resume { file: PathBuf },
}

/// `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` from cpd's own environment
/// (amendment 8): passed to every Lead when set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plannotator {
    pub port: Option<String>,
    pub remote: Option<String>,
}

impl Plannotator {
    pub fn from_env() -> Plannotator {
        let var = |k| std::env::var(k).ok();
        Plannotator { port: var("PLANNOTATOR_PORT"), remote: var("PLANNOTATOR_REMOTE") }
    }
}

/// A Lead's pi launch.
#[derive(Clone, Debug)]
pub struct LeadLaunch<'a> {
    pub home: &'a CpdHome,
    pub session: &'a SessionId,
    /// The Lead's Plan; `None` for a Context's grounded Lead, which gets no
    /// `CPD_PLAN` and no `PLANNOTATOR_*` but `PLANNOTATOR_UPDATE_CHECK`.
    pub plan: Option<&'a PlanId>,
    /// The `-e` sources to pass, from [`extensions_to_pass`].
    pub extensions: &'a [String],
    pub plannotator: &'a Plannotator,
    pub start: LeadStart,
}

/// A Worker's pi launch.
#[derive(Clone, Debug)]
pub struct WorkerLaunch<'a> {
    pub home: &'a CpdHome,
    pub session: &'a SessionId,
    pub plan: &'a PlanId,
    pub attempt: &'a AttemptId,
    /// `<provider>/<model-id>:<level>` for the Attempt's Tier.
    pub model: &'a str,
    /// The `-e` sources to pass, from [`extensions_to_pass`].
    pub extensions: &'a [String],
    /// A review Job's Worker (B2) is a fork of the Plan's Lead: its session file.
    pub fork: Option<&'a Path>,
}

fn push_extensions(argv: &mut Vec<String>, extensions: &[String]) {
    for source in extensions {
        argv.push("-e".into());
        argv.push(source.clone());
    }
}

fn base_env(home: &CpdHome, session: &SessionId, plan: &PlanId) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("CPD_HOME".to_owned(), home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), session.to_string()),
        ("CPD_PLAN".to_owned(), plan.to_string()),
    ])
}

/// A Lead (fresh, fork or resume): contract §6.5's first three rows.
pub fn lead(launch: &LeadLaunch) -> Launch {
    let mut argv = vec!["pi".to_owned()];
    let (model, name, fork, message) = match &launch.start {
        LeadStart::Fresh { model, name, first_message } => (model, name, None, Some(first_message)),
        LeadStart::Fork { model, name, source_file, first_message } => (model, name, Some(source_file), first_message.as_ref()),
        LeadStart::Resume { file } => {
            argv.extend(["--session".to_owned(), file.display().to_string()]);
            push_extensions(&mut argv, launch.extensions);
            return Launch { argv, env: lead_env(launch) };
        }
    };
    argv.extend(["--model".to_owned(), model.clone()]);
    push_extensions(&mut argv, launch.extensions);
    if let Some(source) = fork {
        argv.extend(["--fork".to_owned(), source.display().to_string()]);
    }
    argv.extend(["--session-id".to_owned(), launch.session.to_string(), "-n".to_owned(), name.clone()]);
    if let Some(message) = message {
        argv.extend(["--".to_owned(), message.clone()]);
    }
    Launch { argv, env: lead_env(launch) }
}

fn lead_env(launch: &LeadLaunch) -> BTreeMap<String, String> {
    let Some(plan) = launch.plan else {
        // A grounded Lead (contract §6.5, "Lead (grounded, no Plan)").
        return BTreeMap::from([
            ("CPD_HOME".to_owned(), launch.home.root().display().to_string()),
            ("CPD_SESSION".to_owned(), launch.session.to_string()),
            ("PLANNOTATOR_UPDATE_CHECK".to_owned(), "0".to_owned()),
        ]);
    };
    let mut env = base_env(launch.home, launch.session, plan);
    env.insert("PLANNOTATOR_DATA_DIR".into(), launch.home.plan_dir(plan).join("plannotator").display().to_string());
    env.insert("PLANNOTATOR_UPDATE_CHECK".into(), "0".into());
    if let Some(port) = &launch.plannotator.port {
        env.insert("PLANNOTATOR_PORT".into(), port.clone());
    }
    if let Some(remote) = &launch.plannotator.remote {
        env.insert("PLANNOTATOR_REMOTE".into(), remote.clone());
    }
    env
}

/// A Worker: contract §6.5's last two rows (a review Job's Worker forks the Lead).
pub fn worker(launch: &WorkerLaunch) -> Launch {
    let mut argv: Vec<String> =
        ["pi", "--approve", "--no-skills", "--exclude-tools", WORKER_EXCLUDED_TOOLS].map(str::to_owned).into();
    push_extensions(&mut argv, launch.extensions);
    let attempt = launch.attempt;
    let spec = launch.home.worker_spec(launch.plan, attempt);
    argv.extend(["--model".to_owned(), launch.model.to_owned()]);
    if let Some(source) = launch.fork {
        argv.extend(["--fork".to_owned(), source.display().to_string()]);
    }
    argv.extend([
        "--session-id".to_owned(),
        launch.session.to_string(),
        "-n".to_owned(),
        format!("{} {} a{}", launch.plan, attempt.job(), attempt.number()),
        "--".to_owned(),
        format!("Read {} and follow it exactly. Your attempt id is {attempt}.", spec.display()),
    ]);
    let mut env = base_env(launch.home, launch.session, launch.plan);
    env.insert("CPD_ATTEMPT".into(), attempt.to_string());
    Launch { argv, env }
}

/// A Lead's `-e` candidates: the adapter, then each `lead_pi_packages` entry.
pub fn lead_candidates(adapter: &Path, lead_pi_packages: &[String]) -> Vec<String> {
    let mut candidates = vec![adapter.display().to_string()];
    candidates.extend(lead_pi_packages.iter().cloned());
    candidates
}

/// A Worker's `-e` candidates: the adapter.
pub fn worker_candidates(adapter: &Path) -> Vec<String> {
    vec![adapter.display().to_string()]
}

/// A package's identity, the way pi compares packages (pi `packages.md`,
/// "Understand scope and identity"; pi's `getPackageIdentity`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageId {
    /// An npm spec's package name, without the version.
    Npm(String),
    /// A git source's `<host>/<path>`, without the ref or `.git`.
    Git(String),
    /// A local path, resolved to an absolute path.
    Local(PathBuf),
}

/// The identity of `source`; a relative local path resolves from `base_dir`.
pub fn package_identity(source: &str, base_dir: &Path, home: &Path) -> PackageId {
    let source = source.trim();
    if let Some(spec) = source.strip_prefix("npm:") {
        return PackageId::Npm(npm_name(spec.trim()));
    }
    let non_local = ["git:", "github:", "http:", "https:", "ssh:", "builtin:"];
    if non_local.iter().any(|p| source.starts_with(p))
        && let Some(git) = git_identity(source)
    {
        return PackageId::Git(git);
    }
    PackageId::Local(resolve_path(source, base_dir, home))
}

/// pi's `parseNpmSpec`: `(@scope/)name` before the version's `@`.
fn npm_name(spec: &str) -> String {
    let (scope, rest) = match spec.strip_prefix('@') {
        Some(rest) => ("@", rest),
        None => ("", spec),
    };
    match rest.split_once('@') {
        Some((name, _)) if !name.is_empty() => format!("{scope}{name}"),
        _ => spec.to_owned(),
    }
}

/// pi's `parseGitUrl` reduced to the identity `<host>/<path>`: `git:` sources
/// in any shorthand (`git:host/user/repo@ref`, `git:git@host:user/repo`), and
/// `http(s)://`, `ssh://`, `git://` URLs. None when it is not a git source.
fn git_identity(source: &str) -> Option<String> {
    let lower = source.to_ascii_lowercase();
    let url = if ["https://", "http://", "ssh://", "git://"].iter().any(|s| lower.starts_with(s)) {
        source
    } else {
        source.strip_prefix("git:")?.trim()
    };
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        (host.to_ascii_lowercase(), path)
    } else if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        (host.to_owned(), path)
    } else {
        let (host, path) = url.split_once('/')?;
        if !host.contains('.') && host != "localhost" {
            return None;
        }
        (host.to_owned(), path)
    };
    let path = path.split_once('@').map_or(path, |(repo, _)| repo);
    let path = path.trim_start_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    let unsafe_part = |s: &str| s.contains('\\') || s.contains('\0');
    if host.is_empty() || segments.len() < 2 || segments.contains(&"..") || unsafe_part(&host) || unsafe_part(path) {
        return None;
    }
    Some(format!("{host}/{path}"))
}

/// pi's `resolvePath`: `~` and `file://` expanded, then an absolute path with
/// `.` and `..` removed lexically (no symlinks followed).
fn resolve_path(input: &str, base_dir: &Path, home: &Path) -> PathBuf {
    let input = input.strip_prefix("file://").unwrap_or(input);
    let path = expand_tilde(input, home);
    normalize(&base_dir.join(path))
}

fn expand_tilde(input: &str, home: &Path) -> PathBuf {
    if input == "~" {
        home.to_path_buf()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(input)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// One package a settings file declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) settings: PathBuf,
    pub(crate) source: String,
    pub(crate) id: PackageId,
}

/// The packages a settings file declares, each with its identity (relative
/// paths resolve from the file's directory). A missing file declares none; so
/// does an unreadable or invalid one, because pi then reads it as empty too
/// (its `tryLoadFromStorage`), which is logged.
pub(crate) fn declared(settings: &Path, home: &Path, log: &mut dyn Write) -> Vec<Declared> {
    let text = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            note(log, &format!("cannot read pi settings {}: {e}; treating it as declaring no packages", settings.display()));
            return Vec::new();
        }
    };
    let value: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(e) => {
            note(log, &format!("pi settings {} are not valid JSON ({e}); treating it as declaring no packages", settings.display()));
            return Vec::new();
        }
    };
    let base_dir = settings.parent().expect("a settings file has a directory");
    let Some(Value::Array(packages)) = value.get("packages") else { return Vec::new() };
    packages
        .iter()
        .filter_map(|entry| match entry {
            Value::String(s) => Some(s.as_str()),
            Value::Object(o) => o.get("source").and_then(Value::as_str),
            _ => None,
        })
        .map(|source| Declared {
            settings: settings.to_path_buf(),
            source: source.to_owned(),
            id: package_identity(source, base_dir, home),
        })
        .collect()
}

/// Writes one line to the log. A log that cannot be written is not worth
/// failing a launch for, so a write error is ignored here.
fn note(log: &mut dyn Write, line: &str) {
    let _ = writeln!(log, "cpd: {line}");
}

/// The `[-e …]` rule (contract §6.5): of `candidates`, the ones pi's global
/// settings (`<agent dir>/settings.json`) and project settings
/// (`<cwd>/.pi/settings.json`) do not already declare, compared by package
/// identity. Candidates resolve from `cwd`, as pi resolves `-e`. Each skipped
/// candidate is logged to `log` with the settings file that declares it.
pub fn extensions_to_pass(candidates: &[String], cwd: &Path, dirs: &PiDirs, log: &mut dyn Write) -> Vec<String> {
    let mut declared_packages = declared(&dirs.global_settings(), &dirs.home, log);
    declared_packages.extend(declared(&project_settings(cwd), &dirs.home, log));
    candidates
        .iter()
        .filter(|candidate| {
            let id = package_identity(candidate, cwd, &dirs.home);
            match declared_packages.iter().find(|d| d.id == id) {
                Some(d) => {
                    note(
                        log,
                        &format!(
                            "not passing -e {candidate}: {} already declares it as {:?}",
                            d.settings.display(),
                            d.source
                        ),
                    );
                    false
                }
                None => true,
            }
        })
        .cloned()
        .collect()
}

/// The Session's pi file once pi has created it (contract §6.5):
/// `<agent dir>/sessions/*/*_<id>.jsonl`, or `$PI_CODING_AGENT_SESSION_DIR/*_<id>.jsonl`
/// when that is set (pi then stores sessions directly in it). None until it exists.
pub fn session_file(dirs: &PiDirs, session: &SessionId) -> Option<PathBuf> {
    let suffix = format!("_{session}.jsonl");
    let in_dir = |dir: &Path| -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix)))
            .collect();
        found.sort();
        found.into_iter().next()
    };
    if let Some(dir) = &dirs.session_dir {
        return in_dir(dir);
    }
    let mut groups: Vec<PathBuf> = std::fs::read_dir(dirs.agent_dir.join("sessions"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    groups.sort();
    groups.iter().find_map(|g| in_dir(g))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::num::NonZeroU32;

    use super::*;
    use crate::ids::JobKey;

    const ID: &str = "596d007f-1111-4222-8333-444455556666";

    fn home() -> CpdHome {
        CpdHome::resolve(Some("/cpd-home".into()), None).unwrap()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn lead_launch(start: LeadStart, plannotator: &Plannotator) -> Launch {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let extensions = strs(&["/opt/cpd/pi", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"]);
        lead(&LeadLaunch { home: &home, session: &session, plan: Some(&plan), extensions: &extensions, plannotator, start })
    }

    #[test]
    fn grounded_lead_has_no_plan_in_its_environment() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let extensions = strs(&["/opt/cpd/pi"]);
        let plannotator = Plannotator { port: Some("19432".into()), remote: Some("1".into()) };
        let start = LeadStart::Fork {
            model: "m/x:high".into(),
            name: "ctx".into(),
            source_file: "/s/src.jsonl".into(),
            first_message: None,
        };
        let launch = lead(&LeadLaunch { home: &home, session: &session, plan: None, extensions: &extensions, plannotator: &plannotator, start });
        assert_eq!(
            launch.argv,
            strs(&["pi", "--model", "m/x:high", "-e", "/opt/cpd/pi", "--fork", "/s/src.jsonl", "--session-id", ID, "-n", "ctx"])
        );
        assert_eq!(
            launch.env,
            env(&[("CPD_HOME", "/cpd-home"), ("CPD_SESSION", ID), ("PLANNOTATOR_UPDATE_CHECK", "0")])
        );
    }

    fn lead_env(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut e = env(&[
            ("CPD_HOME", "/cpd-home"),
            ("CPD_SESSION", ID),
            ("CPD_PLAN", "cpd2"),
            ("PLANNOTATOR_DATA_DIR", "/cpd-home/run/cpd2/plannotator"),
            ("PLANNOTATOR_UPDATE_CHECK", "0"),
        ]);
        e.extend(env(extra));
        e
    }

    #[test]
    fn fresh_lead() {
        let start = LeadStart::Fresh {
            model: "github-copilot/claude-opus-5.5:high".into(),
            name: "cpd rust lead".into(),
            first_message: "Plan the rewrite.".into(),
        };
        let launch = lead_launch(start, &Plannotator::default());
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--model", "github-copilot/claude-opus-5.5:high",
                "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--session-id", ID, "-n", "cpd rust lead", "--", "Plan the rewrite.",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
    }

    #[test]
    fn lead_gets_plannotator_port_and_remote_when_set() {
        let start = LeadStart::Resume { file: "/s/x.jsonl".into() };
        let only_port = Plannotator { port: Some("19432".into()), remote: None };
        assert_eq!(lead_launch(start.clone(), &only_port).env, lead_env(&[("PLANNOTATOR_PORT", "19432")]));
        let both = Plannotator { port: Some("19432".into()), remote: Some("1".into()) };
        assert_eq!(
            lead_launch(start, &both).env,
            lead_env(&[("PLANNOTATOR_PORT", "19432"), ("PLANNOTATOR_REMOTE", "1")])
        );
    }

    #[test]
    fn forked_lead_with_and_without_a_first_message() {
        let fork = |first_message: Option<&str>| LeadStart::Fork {
            model: "m/x:high".into(),
            name: "pay-4521".into(),
            source_file: "/s/2026_6f1c.jsonl".into(),
            first_message: first_message.map(str::to_owned),
        };
        let launch = lead_launch(fork(Some("Hello.")), &Plannotator::default());
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--model", "m/x:high",
                "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--fork", "/s/2026_6f1c.jsonl", "--session-id", ID, "-n", "pay-4521", "--", "Hello.",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
        let grounded = lead_launch(fork(None), &Plannotator::default());
        assert_eq!(grounded.argv[grounded.argv.len() - 2..], strs(&["-n", "pay-4521"]));
        assert!(!grounded.argv.contains(&"--".to_owned()));
    }

    #[test]
    fn resumed_lead() {
        let launch = lead_launch(LeadStart::Resume { file: "/s/x.jsonl".into() }, &Plannotator::default());
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--session", "/s/x.jsonl",
                "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
    }

    #[test]
    fn worker() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let attempt = AttemptId::new(JobKey::parse("rs-host").unwrap(), NonZeroU32::new(2).unwrap());
        let extensions = strs(&["/opt/cpd/pi"]);
        let launch = super::worker(&WorkerLaunch {
            home: &home,
            session: &session,
            plan: &plan,
            attempt: &attempt,
            model: "github-copilot/claude-sonnet-5.5:xhigh",
            extensions: &extensions,
            fork: None,
        });
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--approve", "--no-skills", "--exclude-tools",
                "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox",
                "-e", "/opt/cpd/pi",
                "--model", "github-copilot/claude-sonnet-5.5:xhigh",
                "--session-id", ID, "-n", "cpd2 rs-host a2", "--",
                "Read /cpd-home/run/cpd2/specs/rs-host-a2.md and follow it exactly. Your attempt id is rs-host-a2.",
            ])
        );
        assert_eq!(
            launch.env,
            env(&[("CPD_HOME", "/cpd-home"), ("CPD_SESSION", ID), ("CPD_PLAN", "cpd2"), ("CPD_ATTEMPT", "rs-host-a2")])
        );
        let spawn = launch.spawn(Path::new("/wt"));
        assert_eq!(spawn.cwd.as_deref(), Some(Path::new("/wt")));
        assert_eq!((spawn.argv, spawn.env), (launch.argv, launch.env));
    }

    #[test]
    fn a_review_workers_argv_adds_the_fork_of_the_lead() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let attempt = AttemptId::new(JobKey::parse("final-review").unwrap(), NonZeroU32::new(1).unwrap());
        let launch = super::worker(&WorkerLaunch {
            home: &home,
            session: &session,
            plan: &plan,
            attempt: &attempt,
            model: "p/m:high",
            extensions: &[],
            fork: Some(Path::new("/s/lead.jsonl")),
        });
        let argv = &launch.argv;
        let at = argv.iter().position(|a| a == "--fork").expect("--fork");
        assert_eq!(argv[at + 1], "/s/lead.jsonl");
        assert!(argv.contains(&"--approve".to_owned()) && argv.contains(&"--exclude-tools".to_owned()));
        assert_eq!(&argv[at - 2..at], ["--model", "p/m:high"]);
    }

    #[test]
    fn candidates_per_role() {
        let adapter = Path::new("/opt/share/cpd/pi");
        assert_eq!(lead_candidates(adapter, &strs(&["npm:a@1", "npm:b@2"])), strs(&["/opt/share/cpd/pi", "npm:a@1", "npm:b@2"]));
        assert_eq!(worker_candidates(adapter), strs(&["/opt/share/cpd/pi"]));
    }

    #[test]
    fn identities() {
        let base = Path::new("/base/dir");
        let home = Path::new("/home/u");
        let id = |s: &str| package_identity(s, base, home);
        let npm = |s: &str| PackageId::Npm(s.into());
        let git = |s: &str| PackageId::Git(s.into());
        let local = |s: &str| PackageId::Local(s.into());

        assert_eq!(id("npm:@plannotator/pi-extension@0.28.17"), npm("@plannotator/pi-extension"));
        assert_eq!(id("npm:@plannotator/pi-extension"), npm("@plannotator/pi-extension"));
        assert_eq!(id("npm:pi-tools@^1.2"), npm("pi-tools"));
        assert_eq!(id(" npm: pi-tools "), npm("pi-tools"));

        assert_eq!(id("git:github.com/example/pi-tools@v1"), git("github.com/example/pi-tools"));
        assert_eq!(id("git:github.com/example/pi-tools"), git("github.com/example/pi-tools"));
        assert_eq!(id("https://github.com/example/pi-tools"), git("github.com/example/pi-tools"));
        assert_eq!(id("https://github.com/example/pi-tools.git@abc123"), git("github.com/example/pi-tools"));
        assert_eq!(id("ssh://git@GitHub.com:22/example/pi-tools.git"), git("github.com/example/pi-tools"));
        assert_eq!(id("git:git@github.com:example/pi-tools@v2"), git("github.com/example/pi-tools"));

        assert_eq!(id("./pkg"), local("/base/dir/pkg"));
        assert_eq!(id("../other/./pkg/"), local("/base/other/pkg"));
        assert_eq!(id("/abs/pkg"), local("/abs/pkg"));
        assert_eq!(id("~/pkgs/p"), local("/home/u/pkgs/p"));
        assert_eq!(id("file:///abs/pkg"), local("/abs/pkg"));
        assert_eq!(id("pkg"), local("/base/dir/pkg"));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        dirs: PiDirs,
        cwd: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("repo");
            fs::create_dir_all(cwd.join(".pi")).unwrap();
            let dirs = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
            fs::create_dir_all(&dirs.agent_dir).unwrap();
            Fixture { _dir: dir, dirs, cwd }
        }

        fn global(&self, json: &str) -> PathBuf {
            fs::write(self.dirs.global_settings(), json).unwrap();
            self.dirs.global_settings()
        }

        fn project(&self, json: &str) -> PathBuf {
            fs::write(project_settings(&self.cwd), json).unwrap();
            project_settings(&self.cwd)
        }

        fn pass(&self, candidates: &[String]) -> (Vec<String>, String) {
            let mut log = Vec::new();
            let kept = extensions_to_pass(candidates, &self.cwd, &self.dirs, &mut log);
            (kept, String::from_utf8(log).unwrap())
        }
    }

    #[test]
    fn no_settings_pass_every_candidate() {
        let f = Fixture::new();
        let candidates = strs(&["/opt/pi", "npm:@plannotator/pi-extension@0.28.17"]);
        assert_eq!(f.pass(&candidates), (candidates.clone(), String::new()));
    }

    #[test]
    fn skips_npm_specs_declared_globally_whatever_the_version() {
        let f = Fixture::new();
        let settings = f.global(
            r#"{"packages": ["npm:@plannotator/pi-extension@0.30.0",
                {"source": "npm:@juicesharp/rpiv-ask-user-question", "skills": []}]}"#,
        );
        let candidates = strs(&[
            "/opt/pi",
            "npm:@plannotator/pi-extension@0.28.17",
            "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
            "npm:@other/pkg@1.0.0",
        ]);
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, strs(&["/opt/pi", "npm:@other/pkg@1.0.0"]));
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log}");
        assert!(lines[0].contains("-e npm:@plannotator/pi-extension@0.28.17"), "{log}");
        assert!(lines[0].contains(&settings.display().to_string()), "{log}");
        assert!(lines[1].contains("-e npm:@juicesharp/rpiv-ask-user-question@2.13.8"), "{log}");
    }

    #[test]
    fn skips_git_sources_declared_in_the_project_whatever_the_ref() {
        let f = Fixture::new();
        let settings = f.project(r#"{"packages": ["git:github.com/example/pi-tools@v1"]}"#);
        let candidates = strs(&["https://github.com/example/pi-tools@v2", "git:github.com/example/other"]);
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, strs(&["git:github.com/example/other"]));
        assert!(log.contains("-e https://github.com/example/pi-tools@v2"), "{log}");
        assert!(log.contains(&settings.display().to_string()), "{log}");
    }

    #[test]
    fn local_paths_resolve_from_the_declaring_settings_file() {
        let f = Fixture::new();
        // The global file's relative path resolves from the agent dir, the
        // project's from `<cwd>/.pi`; candidates resolve from cwd.
        f.global(r#"{"packages": ["../../share/cpd/pi"]}"#);
        let adapter = f.dirs.home.join("share/cpd/pi");
        f.project(r#"{"packages": [{"source": "../tools/x"}]}"#);
        let candidates = vec![
            adapter.display().to_string(),
            "./tools/x".to_owned(),
            "./tools/y".to_owned(),
            f.dirs.agent_dir.join("../../share/cpd/other").display().to_string(),
        ];
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, vec!["./tools/y".to_owned(), candidates[3].clone()]);
        assert_eq!(log.lines().count(), 2, "{log}");
        assert!(log.contains(&f.dirs.global_settings().display().to_string()), "{log}");
        assert!(log.contains(&project_settings(&f.cwd).display().to_string()), "{log}");
    }

    #[test]
    fn invalid_settings_declare_nothing_and_say_so() {
        let f = Fixture::new();
        f.global("{not json");
        f.project(r#"{"packages": "npm:x"}"#);
        let (kept, log) = f.pass(&strs(&["npm:x@1"]));
        assert_eq!(kept, strs(&["npm:x@1"]));
        assert!(log.contains("not valid JSON"), "{log}");
    }

    #[test]
    fn pi_dirs_follow_pi_s_variables() {
        let home = PathBuf::from("/home/u");
        let d = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/.pi/agent"));
        assert_eq!(d.global_settings(), Path::new("/home/u/.pi/agent/settings.json"));
        assert_eq!(d.session_dir, None);
        let d = PiDirs::resolve(Some(home.clone()), Some("~/agent".into()), Some("/s".into())).unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/agent"));
        assert_eq!(d.session_dir.as_deref(), Some(Path::new("/s")));
        let d = PiDirs::resolve(Some(home), Some("".into()), Some("".into())).unwrap();
        assert_eq!(d.agent_dir, Path::new("/home/u/.pi/agent"));
        assert_eq!(d.session_dir, None);
        assert_eq!(PiDirs::resolve(None, None, None).unwrap_err().code(), ErrorCode::Usage);
    }

    #[test]
    fn finds_the_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let session = SessionId::parse(ID).unwrap();
        let dirs = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let group = dirs.agent_dir.join("sessions/--home-u-repo--");
        fs::create_dir_all(&group).unwrap();
        fs::write(group.join("2026-10-14T10-00-00-000Z_other.jsonl"), "").unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let file = group.join(format!("2026-10-14T10-00-00-000Z_{ID}.jsonl"));
        fs::write(&file, "").unwrap();
        assert_eq!(session_file(&dirs, &session), Some(file));

        let flat = home.join("flat");
        fs::create_dir_all(&flat).unwrap();
        let dirs = PiDirs::resolve(Some(home), None, Some(flat.clone().into_os_string())).unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let file = flat.join(format!("2026_{ID}.jsonl"));
        fs::write(&file, "").unwrap();
        assert_eq!(session_file(&dirs, &session), Some(file));
    }
}
