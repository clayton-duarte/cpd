//! `pr post` and `pr repost` report their preconditions before `approval_required` (pro-d476): an agent under
//! `pr.post.require_approval` with no approval held still hears `already_posted` / `post_not_found` first.

use clap::Parser;

use crate::error::CpdError;
use crate::janitor_tests::Rig;
use crate::output::Output;
use crate::ports::Ports;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: crate::pr::Command,
}

fn pr(ports: &Ports, argv: &[&str]) -> Result<Output, CpdError> {
    let cli = Cli::try_parse_from(std::iter::once("pr").chain(argv.iter().copied())).unwrap();
    Ports::scope(ports, || crate::pr::run(ports, cli.command))
}

/// Workflow `api` of Plan `pay`, open, whose owner requires approval to post; the caller is an agent holding none.
fn rig() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let root = rig.home.root().to_path_buf();
    let policy = root.join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(
        policy.join("owner.toml"),
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\nrequire_approval = true\n",
    )
    .unwrap();
    std::fs::write(policy.join("pay.toml"), "").unwrap();
    // SAFETY: one test per process (nextest).
    unsafe {
        std::env::set_var("CPD_HOME", &root);
        std::env::set_var("CPD_SESSION", "agent-session");
    }
    rig
}

#[test]
fn post_already_posted_beats_approval_required() {
    let rig = rig();
    rig.sql(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, auto_merge)
           VALUES ('pay', 'api', 'C0POLICY1', '1700000000.000100', 'https://slack.test/p1', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z', 'none')",
    );
    let ports = rig.fakes.ports.clone();

    let err = pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).unwrap_err();

    assert_eq!(err.code().as_str(), "already_posted", "{err:?}");
    assert_eq!(err.details()["channel"], "C0POLICY1");
    assert_eq!(err.details()["ts"], "1700000000.000100");
}

#[test]
fn repost_precondition_beats_approval_required() {
    let rig = rig();
    let ports = rig.fakes.ports.clone();

    let err = pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).unwrap_err();

    assert_eq!(err.code().as_str(), "post_not_found", "{err:?}");
}
