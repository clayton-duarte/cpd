//! A pi command line per role (contract §6.5, with commands.md §19 amendment
//! 8): the argv and environment of a Lead, a Manager and a Worker.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::home::CpdHome;
use crate::host::git_guard::path_with_bin;
use crate::host::tmux::Spawn;
use crate::ids::{AttemptId, PlanId, SessionId};

/// The tools a Worker may not use: the Lead's (cpd 1 §12 step 5, kept).
pub const WORKER_EXCLUDED_TOOLS: &str = "ask_user_question,cpd_plan_review";

/// A pi command line: `argv[0]` is `pi`; `env` is what cpd adds to the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Launch {
    /// What tmux runs for this launch in `cwd` (`new-window -c <cwd> -e K=V … -- pi …`).
    pub fn spawn(&self, cwd: &Path) -> Spawn {
        Spawn {
            cwd: Some(cwd.to_path_buf()),
            env: self.env.clone(),
            argv: self.argv.clone(),
        }
    }
}

/// How a Lead starts (contract §6.5, §6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeadStart {
    /// A new session with its first message; a by-user Session (hs-d542) has none: the adapter delivers it with the
    /// User's first prompt.
    Fresh {
        model: String,
        name: String,
        first_message: Option<PathBuf>,
    },
    /// A fork of `source_file`; a fork by `session fork` without `--message-file` has no first message.
    Fork {
        model: String,
        name: String,
        source_file: PathBuf,
        first_message: Option<PathBuf>,
    },
    /// The Session's own file again, with no first message, on `model` (the row's current Tier, hs-d506): pi uses
    /// `--model` instead of the model its file recorded.
    Resume { model: String, file: PathBuf },
}

/// A Lead's pi launch.
#[derive(Clone, Debug)]
pub struct LeadLaunch<'a> {
    pub home: &'a CpdHome,
    pub session: &'a SessionId,
    /// The Lead's Plan; `None` for a Context's Manager, which gets no
    /// `CPD_PLAN`.
    pub plan: Option<&'a PlanId>,
    /// The `-e` sources to pass, from [`extensions_to_pass`].
    pub extensions: &'a [String],
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
}

/// Turns pi's `codemode` tool on, keeping its default tools (hs-d736, contract section `6.6 How cpd starts pi`).
const CODEMODE_TOOLS: [&str; 2] = ["--tools", "+codemode"];

fn push_mcp_codemode_extensions(argv: &mut Vec<String>, extensions: &[String]) {
    // pro-d7: pi's own MCP extension is off; the adapter mounts it with no file config.
    argv.push("--no-mcp".into());
    argv.extend(CODEMODE_TOOLS.map(String::from));
    for source in extensions {
        argv.push("-e".into());
        argv.push(source.clone());
    }
}

/// Silences pi's own update banner in every pane cpd starts (contract §6.5).
const SKIP_VERSION_CHECK: (&str, &str) = ("PI_SKIP_VERSION_CHECK", "1");

/// `PATH` of a Session: `$CPD_HOME/bin`, where the `git` wrapper is, first, then the `PATH` of this process (contract §6.5, hs-d310).
fn session_path(home: &CpdHome) -> (String, String) {
    (
        "PATH".to_owned(),
        path_with_bin(home, std::env::var_os("PATH")),
    )
}

fn base_env(home: &CpdHome, session: &SessionId, plan: &PlanId) -> BTreeMap<String, String> {
    BTreeMap::from([
        session_path(home),
        ("CPD_HOME".to_owned(), home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), session.to_string()),
        ("CPD_PLAN".to_owned(), plan.to_string()),
        (
            SKIP_VERSION_CHECK.0.to_owned(),
            SKIP_VERSION_CHECK.1.to_owned(),
        ),
    ])
}

/// A Lead (fresh, fork or resume): contract §6.5's first three rows.
pub fn lead(launch: &LeadLaunch) -> Launch {
    let mut argv = vec!["pi".to_owned()];
    let (model, name, fork, message) = match &launch.start {
        LeadStart::Fresh {
            model,
            name,
            first_message,
        } => (model, name, None, first_message.as_ref()),
        LeadStart::Fork {
            model,
            name,
            source_file,
            first_message,
        } => (model, name, Some(source_file), first_message.as_ref()),
        LeadStart::Resume { model, file } => {
            argv.extend([
                "--model".to_owned(),
                model.clone(),
                "--session".to_owned(),
                file.display().to_string(),
            ]);
            push_mcp_codemode_extensions(&mut argv, launch.extensions);
            return Launch {
                argv,
                env: lead_env(launch),
            };
        }
    };
    argv.extend(["--model".to_owned(), model.clone()]);
    push_mcp_codemode_extensions(&mut argv, launch.extensions);
    if let Some(source) = fork {
        argv.extend(["--fork".to_owned(), source.display().to_string()]);
    }
    argv.extend([
        "--session-id".to_owned(),
        launch.session.to_string(),
        "-n".to_owned(),
        name.clone(),
    ]);
    if let Some(message) = message {
        argv.extend(["--".to_owned(), format!("@{}", message.display())]);
    }
    Launch {
        argv,
        env: lead_env(launch),
    }
}

fn lead_env(launch: &LeadLaunch) -> BTreeMap<String, String> {
    let Some(plan) = launch.plan else {
        // A Context's Manager (contract §6.5, "Manager").
        return BTreeMap::from([
            quit_marker_env(launch),
            session_path(launch.home),
            (
                "CPD_HOME".to_owned(),
                launch.home.root().display().to_string(),
            ),
            ("CPD_SESSION".to_owned(), launch.session.to_string()),
            (
                SKIP_VERSION_CHECK.0.to_owned(),
                SKIP_VERSION_CHECK.1.to_owned(),
            ),
        ]);
    };
    let mut env = base_env(launch.home, launch.session, plan);
    let (key, value) = quit_marker_env(launch);
    env.insert(key, value);
    env
}

/// `CPD_QUIT_MARKER`: where the adapter writes the User's `/quit` for `session run` (pro-d361).
fn quit_marker_env(launch: &LeadLaunch) -> (String, String) {
    (
        "CPD_QUIT_MARKER".to_owned(),
        launch
            .home
            .quit_marker(launch.session)
            .display()
            .to_string(),
    )
}

/// A Worker: contract §6.5's last two rows.
pub fn worker(launch: &WorkerLaunch) -> Launch {
    // hs-d528: tmux's server starts the pane, so the idle prefix goes in the argv.
    let mut argv: Vec<String> = crate::host::priority::idle_prefix()
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    argv.extend(
        [
            "pi",
            "--approve",
            "--no-skills",
            "--exclude-tools",
            WORKER_EXCLUDED_TOOLS,
        ]
        .map(str::to_owned),
    );
    push_mcp_codemode_extensions(&mut argv, launch.extensions);
    let attempt = launch.attempt;
    let spec = launch.home.worker_spec(launch.plan, attempt);
    argv.extend(["--model".to_owned(), launch.model.to_owned()]);
    argv.extend([
        "--session-id".to_owned(),
        launch.session.to_string(),
        "-n".to_owned(),
        format!("{} {} a{}", launch.plan, attempt.job(), attempt.number()),
        "--".to_owned(),
        format!(
            "Read {} and follow it exactly. Your attempt id is {attempt}.",
            spec.display()
        ),
    ]);
    Launch {
        argv,
        env: worker_env(launch.home, launch.session, launch.plan, attempt),
    }
}

/// A Worker's environment, a pi Worker's and a `durable` Worker's alike (contract §3.4 "The Worker runner and its seam").
pub fn worker_env(
    home: &CpdHome,
    session: &SessionId,
    plan: &PlanId,
    attempt: &AttemptId,
) -> BTreeMap<String, String> {
    let mut env = base_env(home, session, plan);
    env.insert("CPD_ATTEMPT".into(), attempt.to_string());
    env
}

/// The distiller (contract §6.5 "Distiller", §4.6): a one-shot pi that reads
/// the `files` (names in its working directory, each given to pi as `@file`) and prints its JSON
/// event stream (`--mode json`, hs-d244 D-d: the only mode that reports tokens
/// and cost) to stdout, whose final assistant message is the digest. No
/// session, no extension, no skill, no tool, and no `CPD_*` variable: it is no
/// Session. It loads context files, so pi reads the global AGENTS.md, the
/// axioms (hs-d611).
pub fn distiller(model: &str, files: &[&str]) -> Launch {
    let argv = [
        "pi",
        "--mode",
        "json",
        "--no-session",
        "--no-extensions",
        "--no-skills",
        "--no-tools",
        "--model",
        model,
        "--",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(files.iter().map(|f| format!("@{f}")))
    .collect();
    Launch {
        argv,
        env: BTreeMap::from([(
            SKIP_VERSION_CHECK.0.to_owned(),
            SKIP_VERSION_CHECK.1.to_owned(),
        )]),
    }
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
    const MARKER: &str = "/cpd-home/run/quit/596d007f-1111-4222-8333-444455556666";

    fn home() -> CpdHome {
        CpdHome::resolve(Some("/cpd-home".into()), None).unwrap()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The env of a Session: the `pairs` and the `PATH` with `/cpd-home/bin` first (hs-d310).
    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut e: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        e.insert(
            "PATH".to_owned(),
            path_with_bin(&home(), std::env::var_os("PATH")),
        );
        e
    }

    fn lead_launch(start: LeadStart) -> Launch {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let extensions = strs(&[
            "/opt/cpd/pi",
            "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
        ]);
        lead(&LeadLaunch {
            home: &home,
            session: &session,
            plan: Some(&plan),
            extensions: &extensions,
            start,
        })
    }

    /// hs-d506: a resume names the model of the row's Tier, before `--session`: pi restores its file's model only when
    /// `--model` is not given.
    #[test]
    fn a_resume_runs_on_the_given_model() {
        let start = LeadStart::Resume {
            model: "p/m:medium".into(),
            file: "/s/a.jsonl".into(),
        };
        let argv = lead_launch(start).argv;
        assert_eq!(
            argv[..5],
            ["pi", "--model", "p/m:medium", "--session", "/s/a.jsonl"]
        );
    }

    #[test]
    fn every_role_skips_the_pi_version_check() {
        let resume = LeadStart::Resume {
            model: "p/m:high".into(),
            file: "/s/a.jsonl".into(),
        };
        assert_eq!(lead_launch(resume).env["PI_SKIP_VERSION_CHECK"], "1");
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let manager = lead(&LeadLaunch {
            home: &home,
            session: &session,
            plan: None,
            extensions: &[],
            start: LeadStart::Resume {
                model: "p/m:high".into(),
                file: "/s/a.jsonl".into(),
            },
        });
        assert_eq!(manager.env["PI_SKIP_VERSION_CHECK"], "1");
        let plan = PlanId::parse("cpd2").unwrap();
        assert_eq!(
            base_env(&home, &session, &plan)["PI_SKIP_VERSION_CHECK"],
            "1"
        );
    }

    #[test]
    fn a_worker_a_lead_and_the_manager_start_path_with_the_wrapper_directory() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let attempt = AttemptId::new(
            JobKey::parse("rs-host").unwrap(),
            NonZeroU32::new(1).unwrap(),
        );
        let worker = super::worker(&WorkerLaunch {
            home: &home,
            session: &session,
            plan: &plan,
            attempt: &attempt,
            model: "p/m:high",
            extensions: &[],
        });
        let lead = lead_launch(LeadStart::Resume {
            model: "p/m:high".into(),
            file: "/s/a.jsonl".into(),
        });
        let manager = super::lead(&LeadLaunch {
            home: &home,
            session: &session,
            plan: None,
            extensions: &[],
            start: LeadStart::Resume {
                model: "p/m:high".into(),
                file: "/s/a.jsonl".into(),
            },
        });
        for (role, launch) in [("worker", worker), ("lead", lead), ("manager", manager)] {
            let path = &launch.env["PATH"];
            assert!(
                path.starts_with("/cpd-home/bin:") || path == "/cpd-home/bin",
                "{role}: {path}"
            );
        }
        // The Distiller is no Session: it gets no PATH of its own.
        assert!(!super::distiller("p/m:low", &[]).env.contains_key("PATH"));
    }

    #[test]
    fn a_manager_has_no_plan_in_its_environment() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let extensions = strs(&["/opt/cpd/pi"]);
        let start = LeadStart::Fresh {
            model: "m/x:high".into(),
            name: "ctx-manager".into(),
            first_message: Some("/cpd-home/run/ctx/manager-message.md".into()),
        };
        let launch = lead(&LeadLaunch {
            home: &home,
            session: &session,
            plan: None,
            extensions: &extensions,
            start,
        });
        assert_eq!(
            launch.argv,
            strs(&[
                "pi",
                "--model",
                "m/x:high",
                "--no-mcp",
                "--tools",
                "+codemode",
                "-e",
                "/opt/cpd/pi",
                "--session-id",
                ID,
                "-n",
                "ctx-manager",
                "--",
                "@/cpd-home/run/ctx/manager-message.md"
            ])
        );
        assert_eq!(
            launch.env,
            env(&[
                ("CPD_HOME", "/cpd-home"),
                ("CPD_SESSION", ID),
                ("CPD_QUIT_MARKER", MARKER),
                ("PI_SKIP_VERSION_CHECK", "1"),
            ])
        );
    }

    fn lead_env(extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut e = env(&[
            ("CPD_HOME", "/cpd-home"),
            ("CPD_SESSION", ID),
            ("CPD_PLAN", "cpd2"),
            ("CPD_QUIT_MARKER", MARKER),
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
            first_message: Some("/cpd-home/run/cpd2/lead-message.md".into()),
        };
        let launch = lead_launch(start);
        assert_eq!(
            launch.argv,
            strs(&[
                "pi",
                "--model",
                "github-copilot/claude-opus-5.5:high",
                "--no-mcp",
                "--tools",
                "+codemode",
                "-e",
                "/opt/cpd/pi",
                "-e",
                "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--session-id",
                ID,
                "-n",
                "cpd rust lead",
                "--",
                "@/cpd-home/run/cpd2/lead-message.md",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
    }

    #[test]
    fn forked_lead_with_and_without_a_first_message() {
        let fork = |first_message: Option<&str>| LeadStart::Fork {
            model: "m/x:high".into(),
            name: "pay-4521".into(),
            source_file: "/s/2026_6f1c.jsonl".into(),
            first_message: first_message.map(PathBuf::from),
        };
        let launch = lead_launch(fork(Some("/cpd-home/run/cpd2/messages/x.md")));
        assert_eq!(
            launch.argv,
            strs(&[
                "pi",
                "--model",
                "m/x:high",
                "--no-mcp",
                "--tools",
                "+codemode",
                "-e",
                "/opt/cpd/pi",
                "-e",
                "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
                "--fork",
                "/s/2026_6f1c.jsonl",
                "--session-id",
                ID,
                "-n",
                "pay-4521",
                "--",
                "@/cpd-home/run/cpd2/messages/x.md",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
        let silent = lead_launch(fork(None));
        assert_eq!(
            silent.argv[silent.argv.len() - 2..],
            strs(&["-n", "pay-4521"])
        );
        assert!(!silent.argv.contains(&"--".to_owned()));
    }

    #[test]
    fn resumed_lead() {
        let launch = lead_launch(LeadStart::Resume {
            model: "p/m:high".into(),
            file: "/s/x.jsonl".into(),
        });
        assert_eq!(
            launch.argv,
            strs(&[
                "pi",
                "--model",
                "p/m:high",
                "--session",
                "/s/x.jsonl",
                "--no-mcp",
                "--tools",
                "+codemode",
                "-e",
                "/opt/cpd/pi",
                "-e",
                "npm:@juicesharp/rpiv-ask-user-question@2.13.8",
            ])
        );
        assert_eq!(launch.env, lead_env(&[]));
    }

    #[test]
    fn distiller() {
        let launch = super::distiller("p/sonnet:medium", &["prompt.md", "records.md"]);
        assert_eq!(
            launch.argv,
            strs(&[
                "pi",
                "--mode",
                "json",
                "--no-session",
                "--no-extensions",
                "--no-skills",
                "--no-tools",
                "--model",
                "p/sonnet:medium",
                "--",
                "@prompt.md",
                "@records.md",
            ])
        );
        assert_eq!(
            launch.env,
            BTreeMap::from([("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned())])
        );
    }

    #[test]
    fn worker() {
        let home = home();
        let session = SessionId::parse(ID).unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        let attempt = AttemptId::new(
            JobKey::parse("rs-host").unwrap(),
            NonZeroU32::new(2).unwrap(),
        );
        let extensions = strs(&["/opt/cpd/pi"]);
        let launch = super::worker(&WorkerLaunch {
            home: &home,
            session: &session,
            plan: &plan,
            attempt: &attempt,
            model: "github-copilot/claude-sonnet-5.5:xhigh",
            extensions: &extensions,
        });
        assert_eq!(
            launch.argv,
            [crate::host::priority::idle_prefix(), &[
                "pi", "--approve", "--no-skills", "--exclude-tools",
                "ask_user_question,cpd_plan_review",
                "--no-mcp", "--tools", "+codemode", "-e", "/opt/cpd/pi",
                "--model", "github-copilot/claude-sonnet-5.5:xhigh",
                "--session-id", ID, "-n", "cpd2 rs-host a2", "--",
                "Read /cpd-home/run/cpd2/specs/rs-host-a2.md and follow it exactly. Your attempt id is rs-host-a2.",
            ]].concat().into_iter().map(str::to_owned).collect::<Vec<_>>()
        );
        assert_eq!(
            launch.env,
            env(&[
                ("CPD_HOME", "/cpd-home"),
                ("CPD_SESSION", ID),
                ("CPD_PLAN", "cpd2"),
                ("CPD_ATTEMPT", "rs-host-a2"),
                ("PI_SKIP_VERSION_CHECK", "1")
            ])
        );
        let spawn = launch.spawn(Path::new("/wt"));
        assert_eq!(spawn.cwd.as_deref(), Some(Path::new("/wt")));
        assert_eq!((spawn.argv, spawn.env), (launch.argv, launch.env));
    }

    #[test]
    fn candidates_per_role() {
        let adapter = Path::new("/opt/share/cpd/pi");
        assert_eq!(
            lead_candidates(adapter, &strs(&["npm:a@1", "npm:b@2"])),
            strs(&["/opt/share/cpd/pi", "npm:a@1", "npm:b@2"])
        );
        assert_eq!(worker_candidates(adapter), strs(&["/opt/share/cpd/pi"]));
    }

    // The Distiller's pi launch (contract §6.5 "Distiller", hs-d611): no `--no-context-files`, so pi loads the global
    // AGENTS.md, and every other flag and the environment stay as they were.
    const DISTILLER_MODEL: &str = "p/sonnet:medium";
    const DISTILLER_FILES: &[&str] = &["prompt.md", "batch.md", "known.md", "docs.md"];

    #[test]
    fn distiller_launch_does_not_pass_no_context_files() {
        let argv = super::distiller(DISTILLER_MODEL, DISTILLER_FILES).argv;
        assert!(
            !argv.iter().any(|arg| arg == "--no-context-files"),
            "the Distiller must load context files (hs-d611): {argv:?}"
        );
        for flag in [
            "--no-session",
            "--no-extensions",
            "--no-skills",
            "--no-tools",
        ] {
            assert!(argv.iter().any(|arg| arg == flag), "{flag} in {argv:?}");
        }
        assert!(
            argv.windows(2).any(|pair| pair == ["--mode", "json"]),
            "--mode json in {argv:?}"
        );
        let model = argv
            .windows(2)
            .find(|pair| pair[0] == "--model")
            .map(|pair| pair[1].as_str());
        assert_eq!(model, Some(DISTILLER_MODEL), "model in {argv:?}");

        let separator = argv
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or_else(|| panic!("no `--` in {argv:?}"));
        let expected: Vec<String> = DISTILLER_FILES
            .iter()
            .map(|file| format!("@{file}"))
            .collect();
        assert_eq!(
            &argv[separator + 1..],
            expected.as_slice(),
            "file arguments after `--` in {argv:?}"
        );
    }

    #[test]
    fn distiller_launch_env_is_only_skip_version_check() {
        let env = super::distiller(DISTILLER_MODEL, DISTILLER_FILES).env;
        assert_eq!(
            env,
            BTreeMap::from([("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned())])
        );
    }
}

#[cfg(test)]
#[path = "launch_no_mcp_tests.rs"]
mod no_mcp_tests;

#[cfg(test)]
#[path = "launch_codemode_tests.rs"]
mod codemode_tests;
