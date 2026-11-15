//! Paired shaping completion through Lead-owned commands, with all host ports faked.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

use clap::Parser;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;

const MANAGER: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: cpd_core::plan::Command,
}
#[derive(Parser)]
struct JobCli {
    #[command(subcommand)]
    command: cpd_core::job::Command,
}

struct Fixture {
    home: CpdHome,
    ports: Fakes,
    saved: Vec<(&'static str, Option<OsString>)>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        std::fs::create_dir_all(home.root().join("policy/acme")).unwrap();
        std::fs::write(
            home.root().join("policy/acme/owner.toml"),
            "[model]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\nG = \"p/gemini\"\n",
        )
        .unwrap();
        std::fs::write(
            home.config(),
            "version = 1\nmachine = \"m1\"\nlead_runner = \"durable\"\n",
        )
        .unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let pi = bin.join("pi");
        std::fs::write(&pi, "").unwrap();
        std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();
        let install = dir.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("current-version"), "1.0.0\n").unwrap();
        for package in ["pi-coding-agent", "pi-ai"] {
            let dist = install
                .join("releases/1.0.0/node_modules/@earendil-works")
                .join(package)
                .join("dist");
            std::fs::create_dir_all(&dist).unwrap();
            std::fs::write(dist.join("index.js"), "").unwrap();
        }
        let saved = [
            "CPD_HOME",
            "HOME",
            "PATH",
            "CPD_SESSION",
            "PI_SESSION_ID",
            "CPD_PLAN",
            "CPD_ATTEMPT",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        unsafe {
            std::env::set_var("CPD_HOME", home.root());
            std::env::set_var("HOME", dir.path());
            std::env::set_var("PATH", &bin);
            std::env::set_var("CPD_SESSION", MANAGER);
            for key in ["PI_SESSION_ID", "CPD_PLAN", "CPD_ATTEMPT"] {
                std::env::remove_var(key);
            }
        }
        let f = Self {
            home,
            ports: Fakes::new(),
            saved,
            _dir: dir,
        };
        f.sql(&format!("INSERT INTO contexts (id,title,owner,created_at,created_on) VALUES ('ctx','Context','acme','t','m1'); INSERT INTO sessions (id,role,context,name,cwd,tier,state,state_at,created_at) VALUES ('{MANAGER}','manager','ctx','ctx','/','O3','stopped','t','t'); INSERT INTO tier_catalog (pi_version,letter,model,efforts,read_at) VALUES ('1.0.0','O','p/opus','[0,1,2,3,4]','t'),('1.0.0','S','p/sonnet','[0,1,2,3,4]','t'),('1.0.0','H','p/haiku','[0,1,2,3,4]','t'),('1.0.0','G','p/gemini','[]','t');"));
        f
    }
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }
    fn sql(&self, sql: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }
    fn count(&self, sql: &str) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }
    fn plan(&self, args: &[&str]) -> Result<Output, CpdError> {
        let command =
            PlanCli::parse_from(std::iter::once("cpd").chain(args.iter().copied())).command;
        Ports::scope(&self.ports.ports, || cpd_core::plan::run(command))
    }
    fn job(&self, args: &[&str]) -> Result<Output, CpdError> {
        let command =
            JobCli::parse_from(std::iter::once("cpd").chain(args.iter().copied())).command;
        Ports::scope(&self.ports.ports, || cpd_core::job::run(command))
    }
    fn queued(&self, key: &str) {
        self.sql(&format!("INSERT INTO jobs (plan,workflow,key,title,tier,goal,context,touches,acceptance,out_of_scope,kind,status,appended,fingerprint,created_txn,created_at) VALUES ('fresh',NULL,'{key}','Scout','H3','g','c','[\"a\"]','[\"b\"]','[]','scout','queued',1,'{}',cpd_txn(),'t');", "a".repeat(64)));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn paired_shaping_lead_finishes_only_after_all_jobs_are_final_and_can_reopen() {
    let f = Fixture::new();
    f.plan(&[
        "new",
        "fresh",
        "--context",
        "ctx",
        "--title",
        "Fresh",
        "--shaping",
    ])
    .unwrap();
    assert_eq!(f.count("SELECT count(*) FROM plans WHERE id='fresh' AND phase='shaping' AND lead_session IS NOT NULL AND locked_at IS NULL AND done_at IS NULL"), 1);
    assert_eq!(
        f.count("SELECT count(*) FROM events WHERE plan='fresh' AND kind='plan-done'"),
        0
    );
    f.queued("first");
    f.queued("second");
    let args = [
        "cancel",
        "first",
        "--plan",
        "fresh",
        "--reason",
        "not needed",
    ];
    assert_eq!(f.job(&args).unwrap_err().code(), ErrorCode::PlanHasLead);
    assert_eq!(
        f.count("SELECT count(*) FROM jobs WHERE plan='fresh' AND status='queued'"),
        2
    );
    let lead: String = f
        .store()
        .read(|c| {
            Ok(
                c.query_row("SELECT lead_session FROM plans WHERE id='fresh'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    unsafe { std::env::set_var("CPD_SESSION", &lead) };
    f.job(&args).unwrap();
    assert_eq!(
        f.count(
            "SELECT count(*) FROM plans WHERE id='fresh' AND phase='shaping' AND done_at IS NULL"
        ),
        1
    );
    assert_eq!(
        f.count("SELECT count(*) FROM events WHERE plan='fresh' AND kind='plan-done'"),
        0
    );
    let last = [
        "cancel",
        "second",
        "--plan",
        "fresh",
        "--reason",
        "not needed",
    ];
    f.job(&last).unwrap();
    assert_eq!(f.count("SELECT count(*) FROM plans WHERE id='fresh' AND phase='done' AND lead_session IS NOT NULL AND locked_at IS NULL AND done_at IS NOT NULL"), 1);
    assert_eq!(f.count("SELECT count(*) FROM events WHERE plan='fresh' AND kind='plan-done' AND json_extract(data,'$.turn')=0 AND json_array_length(json_extract(data,'$.workflows'))=0"), 1);
    f.job(&last).unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM events WHERE plan='fresh' AND kind='plan-done'"),
        1
    );
    unsafe { std::env::set_var("CPD_SESSION", MANAGER) };
    assert!(
        f.plan(&["rename", "--plan", "fresh", "--title", "Too late"])
            .is_err()
    );

    // A Lead's subsequent explore Job uses the existing done -> plan reopen path.
    unsafe { std::env::set_var("CPD_SESSION", &lead) };
    let spec = f._dir.path().join("scout.json");
    std::fs::write(
        &spec,
        r#"{"goal":"Find it","context":"Here","acceptance":["Found"],"out_of_scope":[]}"#,
    )
    .unwrap();
    f.job(&[
        "add",
        "again",
        "--plan",
        "fresh",
        "--title",
        "Again",
        "--kind",
        "scout",
        "--tier",
        "H3",
        "--spec",
        spec.to_str().unwrap(),
    ])
    .unwrap();
    assert_eq!(f.count("SELECT count(*) FROM plans WHERE id='fresh' AND phase='plan' AND locked_at IS NULL AND done_at IS NULL"), 1);
    assert_eq!(f.count("SELECT count(*) FROM events WHERE plan='fresh' AND kind='plan-phase' AND json_extract(data,'$.from')='done' AND json_extract(data,'$.to')='plan'"), 1);
}
