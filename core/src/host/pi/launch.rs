//! A pi command line per role (contract §6.5, with commands.md §19 amendment
//! 8): the argv and environment of a Lead, a Manager and a Worker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::home::CpdHome;
use crate::host::tmux::Spawn;
use crate::ids::{AttemptId, PlanId, SessionId};

/// The tools a Worker may not use: the Lead's (cpd 1 §12 step 5, kept).
pub const WORKER_EXCLUDED_TOOLS: &str =
    "ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox";

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
    Fresh { model: String, name: String, first_message: PathBuf },
    /// A fork of `source_file`; a fork by `session fork` without `--message-file` has no first message.
    Fork { model: String, name: String, source_file: PathBuf, first_message: Option<PathBuf> },
    /// The Session's own file again, with no first message.
    Resume { file: PathBuf },
}

/// `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` for a Lead's pi. `from_env` reads
/// cpd's own environment (amendment 8), which is what Lead panes are given;
/// `session run` replaces the port with the Plan's (contract §6.5).
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
    /// The Lead's Plan; `None` for a Context's Manager, which gets no
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

/// Silences pi's own update banner in every pane cpd starts (contract §6.5).
const SKIP_VERSION_CHECK: (&str, &str) = ("PI_SKIP_VERSION_CHECK", "1");

fn base_env(home: &CpdHome, session: &SessionId, plan: &PlanId) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("CPD_HOME".to_owned(), home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), session.to_string()),
        ("CPD_PLAN".to_owned(), plan.to_string()),
        (SKIP_VERSION_CHECK.0.to_owned(), SKIP_VERSION_CHECK.1.to_owned()),
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
        argv.extend(["--".to_owned(), format!("@{}", message.display())]);
    }
    Launch { argv, env: lead_env(launch) }
}

fn lead_env(launch: &LeadLaunch) -> BTreeMap<String, String> {
    let Some(plan) = launch.plan else {
        // A Context's Manager (contract §6.5, "Manager").
        return BTreeMap::from([
            ("CPD_HOME".to_owned(), launch.home.root().display().to_string()),
            ("CPD_SESSION".to_owned(), launch.session.to_string()),
            ("PLANNOTATOR_UPDATE_CHECK".to_owned(), "0".to_owned()),
            (SKIP_VERSION_CHECK.0.to_owned(), SKIP_VERSION_CHECK.1.to_owned()),
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

#[cfg(test)]
mod tests {
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
    fn every_role_skips_the_pi_version_check() {
        let plannotator = Plannotator { port: None, remote: None };
        let resume = LeadStart::Resume { file: "/s/a.jsonl".into() };
        assert_eq!(lead_launch(resume, &plannotator).env["PI_SKIP_VERSION_CHECK"], "1");
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let manager = lead(&LeadLaunch {
            home: &home,
            session: &session,
            plan: None,
            extensions: &[],
            plannotator: &plannotator,
            start: LeadStart::Resume { file: "/s/a.jsonl".into() },
        });
        assert_eq!(manager.env["PI_SKIP_VERSION_CHECK"], "1");
        let plan = PlanId::parse("cpd2").unwrap();
        assert_eq!(base_env(&home, &session, &plan)["PI_SKIP_VERSION_CHECK"], "1");
    }

    #[test]
    fn a_manager_has_no_plan_in_its_environment() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let extensions = strs(&["/opt/cpd/pi"]);
        let plannotator = Plannotator { port: Some("19432".into()), remote: Some("1".into()) };
        let start = LeadStart::Fresh { model: "m/x:high".into(), name: "ctx-manager".into(), first_message: "/cpd-home/run/ctx/manager-message.md".into() };
        let launch = lead(&LeadLaunch { home: &home, session: &session, plan: None, extensions: &extensions, plannotator: &plannotator, start });
        assert_eq!(
            launch.argv,
            strs(&["pi", "--model", "m/x:high", "-e", "/opt/cpd/pi", "--session-id", ID, "-n", "ctx-manager", "--", "@/cpd-home/run/ctx/manager-message.md"])
        );
        assert_eq!(
            launch.env,
            env(&[("CPD_HOME", "/cpd-home"), ("CPD_SESSION", ID), ("PLANNOTATOR_UPDATE_CHECK", "0"), ("PI_SKIP_VERSION_CHECK", "1")])
        );
    }

    fn lead_env(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut e = env(&[
            ("CPD_HOME", "/cpd-home"),
            ("CPD_SESSION", ID),
            ("CPD_PLAN", "cpd2"),
            ("PLANNOTATOR_DATA_DIR", "/cpd-home/run/cpd2/plannotator"),
            ("PLANNOTATOR_UPDATE_CHECK", "0"),
            ("PI_SKIP_VERSION_CHECK", "1"),
        ]);
        e.extend(env(extra));
        e
    }

    #[test]
    fn fresh_lead() {
        let start = LeadStart::Fresh {
            model: "github-copilot/claude-opus-5.5:high".into(),
            name: "cpd rust lead".into(),
            first_message: "/cpd-home/run/cpd2/lead-message.md".into(),
        };
        let launch = lead_launch(start, &Plannotator::default());
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--model", "github-copilot/claude-opus-5.5:high",
                "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--session-id", ID, "-n", "cpd rust lead", "--", "@/cpd-home/run/cpd2/lead-message.md",
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
            first_message: first_message.map(PathBuf::from),
        };
        let launch = lead_launch(fork(Some("/cpd-home/run/cpd2/messages/x.md")), &Plannotator::default());
        assert_eq!(
            launch.argv,
            strs(&[
                "pi", "--model", "m/x:high",
                "-e", "/opt/cpd/pi", "-e", "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--fork", "/s/2026_6f1c.jsonl", "--session-id", ID, "-n", "pay-4521", "--", "@/cpd-home/run/cpd2/messages/x.md",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
        let silent = lead_launch(fork(None), &Plannotator::default());
        assert_eq!(silent.argv[silent.argv.len() - 2..], strs(&["-n", "pay-4521"]));
        assert!(!silent.argv.contains(&"--".to_owned()));
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
            env(&[("CPD_HOME", "/cpd-home"), ("CPD_SESSION", ID), ("CPD_PLAN", "cpd2"), ("CPD_ATTEMPT", "rs-host-a2"), ("PI_SKIP_VERSION_CHECK", "1")])
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

}
