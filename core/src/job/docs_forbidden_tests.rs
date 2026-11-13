//! Red tests of `workflow.local_docs = false` (bug b72; contract: the amended docs/contract.md): an owner policy that
//! sets it refuses `job add --kind docs`, `workflow add --shape docs`, the `docs` stub of the `tdd` template, and a
//! `job submit` whose diff ADDS a `docs/**/*.md` file, all with `docs_forbidden`. The default (true) is unchanged.
//!
//! Every command runs in process on a temp `$CPD_HOME` with the fakes of `crate::ports`: a fake git, no process.
//! `CPD_HOME` is read from the process: nextest gives each test a process of its own.

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use serde_json::Value;
use tempfile::TempDir;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::git::{GitRoute, Head, Repo, Universe};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::ports::{Fakes, Ports};

const OWNER_NO_DOCS: &str = "[workflow]\nlocal_docs = false\n";

struct Rig {
    tmp: TempDir,
    fakes: Fakes,
    env: Env,
    repo: Repo,
    lead_worktree: PathBuf,
}

impl Rig {
    /// A temp `$CPD_HOME` whose owner `acme` has the policy `owner`; Plan `pay` (phase `phase`) of Context `ctx`; a
    /// fake git repo `acme/r` on `main` with the Lead Branch `cpd/pay/api` checked out in a Lead worktree.
    fn new(owner: &str, phase: &str) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
        let dir = home.config().parent().unwrap().join("policy").join("acme");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("owner.toml"), owner).unwrap();
        std::fs::write(
            dir.join("r.toml"),
            "[test-commands]\nbuild = \"make\"\nrun = \"make test\"\n",
        )
        .unwrap();
        // SAFETY: nextest gives each test a process of its own.
        unsafe { std::env::set_var("CPD_HOME", home.root()) };

        let fakes = Fakes::new();
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));
        let _ = (*fakes.runner).clone().with_git(&route);
        let repo = Repo::init(&universe, &root.join("r"));
        repo.write("f.txt", "base\n");
        repo.write("docs/old.md", "old\n");
        repo.commit("base");
        repo.keyed("r");
        repo.branch("cpd/pay/api");
        let lead_worktree = root.join("r.lead");
        // The fake git does not check files out: the worktree holds the base files as committed.
        std::fs::create_dir_all(lead_worktree.join("docs")).unwrap();
        std::fs::write(lead_worktree.join("f.txt"), "base\n").unwrap();
        std::fs::write(lead_worktree.join("docs/old.md"), "old\n").unwrap();
        universe.lock().unwrap().add_worktree(
            repo.id(),
            &lead_worktree,
            Head::Branch("cpd/pay/api".to_owned()),
        );

        let mut env = Env::open_home_with(&fakes.ports, home).unwrap();
        env.catalog = Some(catalog());
        let mut rig = Rig {
            tmp,
            fakes,
            env,
            repo,
            lead_worktree,
        };
        rig.sql(&format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
               VALUES ('pay', 'ctx', 'Pay', '{phase}', 'lead1', {locked}, 't');",
            locked = if phase == "plan" { "NULL" } else { "'t'" },
        ));
        rig
    }

    fn sql(&mut self, sql: &str) {
        self.env
            .store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// Workflow `api` of the Plan in the Lead worktree, and one queued `work` Job `a` touching everything.
    fn workflow_with_job(&mut self) {
        let fp = "f".repeat(64);
        let sql = format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
               VALUES ('pay', 'api', '{repo}', 'main', 'dispatch', 'cpd/pay/api', '{lead}', 't', cpd_txn(), 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'queued', 0, '{fp}', cpd_txn(), 't');",
            repo = self.repo.dir().display(),
            lead = self.lead_worktree.display(),
        );
        self.sql(&sql);
        let plan = crate::ids::PlanId::parse("pay").unwrap();
        let jobs = self
            .env
            .store
            .read(|conn| crate::plan::jobs::load(conn, &plan))
            .unwrap();
        for j in jobs {
            let fingerprint = crate::plan::locked_fingerprint(&j);
            self.sql(&format!(
                "UPDATE jobs SET fingerprint = '{fingerprint}' WHERE plan = 'pay' AND key = '{}';",
                j.key
            ));
        }
    }

    /// A draft Job `key` (kind `work`) of Workflow `workflow`.
    fn add_draft(&mut self, key: &str, workflow: &str) {
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', '{workflow}', '{key}', 'D', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');"
        ));
    }

    /// A second Workflow `key` of the Plan on the same repo.
    fn add_workflow_row(&mut self, key: &str, branch: &str) {
        let sql = format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
               VALUES ('pay', '{key}', '{repo}', 'main', 'dispatch', '{branch}', '{lead}', 't', cpd_txn(), 't');",
            repo = self.repo.dir().display(),
            lead = self.lead_worktree.display(),
        );
        self.sql(&sql);
    }

    fn scoped<R>(&self, f: impl FnOnce() -> R) -> R {
        Ports::scope(&self.fakes.ports, f)
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.env
            .store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// `cpd job add docs1 --kind docs --tier S2 --spec <file> --plan pay --workflow api` (a draft when `draft`).
    fn job_add_docs(&mut self, draft: bool) -> Result<crate::output::Output, CpdError> {
        #[derive(Parser)]
        struct Wrap {
            #[command(flatten)]
            add: crate::job::AddArgs,
        }
        let spec = self.tmp.path().join("spec.json");
        std::fs::write(
            &spec,
            r#"{"goal": "g", "context": "c", "touches": ["docs/x.md"], "acceptance": ["b"], "out_of_scope": []}"#,
        )
        .unwrap();
        let mut argv = vec![
            "wrap",
            "docs1",
            "--title",
            "T",
            "--tier",
            "S2",
            "--spec",
            spec.to_str().unwrap(),
            "--kind",
            "docs",
            "--workflow",
            "api",
            "--plan",
            "pay",
        ];
        if draft {
            argv.extend(["--draft", "--waiting-on", "lead"]);
        }
        let wrap = Wrap::try_parse_from(argv).unwrap();
        let ports = self.fakes.ports.clone();
        Ports::scope(&ports, || crate::job::add::add_in(&mut self.env, wrap.add))
    }

    /// `cpd workflow add api --repo <repo> --title T --shape <shape> --plan pay`.
    fn workflow_add(&self, shape: &str) -> Result<crate::output::Output, CpdError> {
        #[derive(Parser)]
        struct Wrap {
            #[command(subcommand)]
            command: crate::workflow::WorkflowCommand,
        }
        let wrap = Wrap::try_parse_from([
            "wrap",
            "add",
            "api",
            "--repo",
            self.repo.dir().to_str().unwrap(),
            "--title",
            "T",
            "--shape",
            shape,
            "--plan",
            "pay",
        ])
        .unwrap();
        self.scoped(|| crate::workflow::run_workflow(wrap.command))
    }

    /// `git -C <dir> <args>` on the fake git; it must exit 0.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut cmd = Process::new("git");
        cmd.arg("-C").arg(dir).args(args);
        let out = match self
            .fakes
            .ports
            .runner
            .output(&mut cmd, "git", Duration::from_secs(60))
        {
            Ok(o) => o,
            Err(Failed::Io(e)) => panic!("git could not start: {e}"),
            Err(Failed::Timeout(e)) => panic!("git timed out: {e:?}"),
        };
        assert!(
            out.status.success(),
            "git {args:?} said: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Starts Job `a` as the starter does, then commits `changes` in the Attempt's worktree (the Lead worktree):
    /// `(path, Some(text))` writes the file, `(path, None)` deletes it.
    fn start_and_commit(&mut self, changes: &[(&str, Option<&str>)]) {
        use crate::config::Config;
        use crate::driver::{StartCtx, start_with};
        self.fakes
            .spawn
            .set_self_exe(std::env::current_exe().unwrap());
        let config = Config::parse(
            "version = 1\nmachine = \"test\"\n",
            Path::new("config.toml"),
        )
        .unwrap()
        .0;
        let catalog = catalog();
        let ctx = StartCtx {
            home: &self.env.home,
            config: &config,
            catalog: &catalog,
            command: "test",
        };
        let plan = crate::ids::PlanId::parse("pay").unwrap();
        let started = start_with(
            &self.fakes.ports,
            &mut self.env.store,
            &ctx,
            &plan,
            &crate::ids::JobKey::parse("a").unwrap(),
        )
        .unwrap();
        self.sql(&format!(
            "UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = '{}';",
            started.attempt
        ));
        let wt = self.lead_worktree.clone();
        let base = self.git(&wt, &["rev-parse", "HEAD"]);
        self.sql(&format!(
            "UPDATE attempts SET base_sha = '{base}' WHERE plan = 'pay' AND id = '{}';",
            started.attempt
        ));
        self.commit(changes);
    }

    /// Commits `changes` in the Attempt's worktree.
    fn commit(&mut self, changes: &[(&str, Option<&str>)]) {
        let wt = self.lead_worktree.clone();
        for (path, text) in changes {
            match text {
                Some(text) => {
                    let file = wt.join(path);
                    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                    std::fs::write(file, text).unwrap();
                    self.git(&wt, &["add", path]);
                }
                None => {
                    self.git(&wt, &["rm", "-q", path]);
                }
            }
        }
        self.git(&wt, &["commit", "-q", "-m", "work"]);
    }

    /// `cpd attempt submit a-a1 --plan pay` from the Attempt's worktree.
    fn submit(&mut self) -> Result<crate::output::Output, CpdError> {
        #[derive(Parser)]
        struct Wrap {
            #[command(subcommand)]
            command: crate::attempt::Command,
        }
        let wrap = Wrap::try_parse_from(["wrap", "submit", "a-a1", "--plan", "pay"]).unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&self.lead_worktree).unwrap();
        let out = self.scoped(|| crate::attempt::run(wrap.command));
        std::env::set_current_dir(previous).unwrap();
        out
    }
}

fn catalog() -> crate::config::Catalog {
    use crate::config::{Catalog, TierModel};
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![1, 2, 3],
    };
    Catalog {
        j: TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    }
}

fn refused(out: Result<crate::output::Output, CpdError>, what: &str) -> CpdError {
    match out {
        Err(e) => {
            assert_eq!(e.code(), ErrorCode::DocsForbidden, "{what}: {e}");
            e
        }
        Ok(_) => panic!("{what}: expected the refusal docs_forbidden, got a success"),
    }
}

#[test]
fn job_add_docs_kind_refused_when_local_docs_false() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    rig.workflow_with_job();
    let err = refused(rig.job_add_docs(false), "job add --kind docs");
    let message = err.to_string();
    assert!(
        message.contains("workflow.local_docs"),
        "the message names the policy key: {message}"
    );
    assert!(
        message.contains("cpd context decide"),
        "the message points at the Context: {message}"
    );
    assert_eq!(
        rig.count("SELECT count(*) FROM jobs WHERE key = 'docs1'"),
        0,
        "a refused add writes nothing"
    );
    let draft = refused(rig.job_add_docs(true), "job add --kind docs --draft");
    assert_eq!(draft.code(), ErrorCode::DocsForbidden);
}

#[test]
fn job_add_docs_kind_allowed_by_default() {
    let mut rig = Rig::new("", "dispatch");
    rig.workflow_with_job();
    let out = rig.job_add_docs(false);
    assert!(
        out.is_ok(),
        "the default local_docs = true allows it: {out:?}"
    );
    assert_eq!(
        rig.count("SELECT count(*) FROM jobs WHERE key = 'docs1'"),
        1
    );
    // The policy, not the kind, decides: the same add is refused once the owner sets the key false.
    let owner = rig
        .env
        .home
        .config()
        .parent()
        .unwrap()
        .join("policy/acme/owner.toml");
    std::fs::write(owner, OWNER_NO_DOCS).unwrap();
    refused(
        rig.job_add_docs(true),
        "the same add after local_docs = false",
    );
}

#[test]
fn workflow_add_docs_shape_refused_when_local_docs_false() {
    let rig = Rig::new(OWNER_NO_DOCS, "plan");
    let err = refused(rig.workflow_add("docs"), "workflow add --shape docs");
    assert!(
        err.to_string().contains("workflow.local_docs"),
        "the message names the policy key: {err}"
    );
}

#[test]
fn tdd_template_has_no_docs_stub_when_local_docs_false() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "plan");
    let out = rig.workflow_add("tdd");
    assert!(out.is_ok(), "the tdd shape is still allowed: {out:?}");
    let keys: Vec<String> = rig
        .env
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT key FROM jobs WHERE plan = 'pay' AND workflow = 'api' AND kind != 'open' ORDER BY key",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(
        keys,
        vec!["api-impl".to_owned(), "api-tests".to_owned()],
        "the stubs are tests and impl"
    );
    let needs: i64 =
        rig.count("SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'api-tests'");
    assert_eq!(needs, 0, "the tests stub needs nothing");
    let _: Value = Value::Null;
}

#[test]
fn submit_refuses_added_markdown_under_docs_dir() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    rig.workflow_with_job();
    rig.start_and_commit(&[("docs/new/design.md", Some("# design\n"))]);
    let err = refused(rig.submit(), "submit with an added docs/**/*.md");
    let message = err.to_string();
    assert!(
        message.contains("workflow.local_docs"),
        "the message names the policy key: {message}"
    );
    assert!(
        message.contains("cpd context decide"),
        "the message points at the Context: {message}"
    );
}

#[test]
fn submit_allows_modified_markdown_under_docs_dir_and_added_markdown_elsewhere() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    rig.workflow_with_job();
    // The control: with an added docs/**/*.md in the diff, the submit is refused ...
    rig.start_and_commit(&[
        ("docs/old.md", Some("changed\n")),
        ("README.md", Some("# readme\n")),
        ("src/notes.md", Some("notes\n")),
        ("docs/new.md", Some("new\n")),
    ]);
    refused(rig.submit(), "submit with an added docs/new.md");
    // ... and once the added file is gone, the modified docs file and the .md files elsewhere pass.
    rig.commit(&[("docs/new.md", None)]);
    let out = rig.submit();
    assert!(
        out.is_ok(),
        "a modified docs file and an added .md elsewhere pass: {out:?}"
    );
}

#[test]
fn policy_show_lists_local_docs() {
    #[derive(Parser)]
    struct Wrap {
        #[command(subcommand)]
        command: crate::policy::Command,
    }
    let rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    let wrap = Wrap::try_parse_from(["wrap", "show", "--owner", "acme"]).unwrap();
    let out = rig
        .scoped(|| crate::policy::run(wrap.command))
        .expect("policy show succeeds");
    let crate::output::Output::Success(map) = out else {
        panic!("policy show prints a success envelope")
    };
    let json = Value::Object(map);
    let key = &json["policy"]["workflow"]["keys"]["workflow.local_docs"];
    assert_eq!(key["value"], Value::Bool(false), "{json}");
    assert!(
        key["source"].as_str().unwrap().contains("owner.toml"),
        "{key}"
    );
}

#[test]
fn submit_allows_deleting_docs_markdown_already_deleted_on_lead() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    rig.workflow_with_job();
    rig.start_and_commit(&[("docs/old.md", None)]);
    // Another Job deleted the same file on the Lead Branch meanwhile.
    let lead = rig.repo.dir().to_path_buf();
    rig.git(&lead, &["rm", "-q", "docs/old.md"]);
    let out = rig.submit();
    assert!(out.is_ok(), "a deletion is never refused: {out:?}");
}

/// `cpd job edit <key> ...` on the draft `d` of Workflow `api`.
fn edit_draft(rig: &mut Rig, extra: &[&str]) -> Result<crate::output::Output, CpdError> {
    #[derive(Parser)]
    struct Wrap {
        #[command(flatten)]
        edit: crate::job::EditArgs,
    }
    let mut argv = vec!["wrap", "d", "--plan", "pay"];
    argv.extend_from_slice(extra);
    let wrap = Wrap::try_parse_from(argv).unwrap();
    let ports = rig.fakes.ports.clone();
    Ports::scope(&ports, || {
        crate::job::edit::edit_in(&mut rig.env, wrap.edit)
    })
}

#[test]
fn job_edit_to_docs_kind_refused_when_local_docs_false() {
    let mut rig = Rig::new(OWNER_NO_DOCS, "dispatch");
    rig.workflow_with_job();
    rig.add_draft("d", "api");
    refused(
        edit_draft(&mut rig, &["--kind", "docs"]),
        "job edit --kind docs",
    );
    assert_eq!(
        rig.count("SELECT count(*) FROM jobs WHERE key = 'd' AND kind = 'docs'"),
        0
    );
}

#[test]
fn job_edit_move_docs_draft_into_forbidden_workflow_refused() {
    let mut rig = Rig::new("", "dispatch");
    rig.workflow_with_job();
    rig.add_workflow_row("other", "cpd/pay/other");
    rig.add_draft("d", "other");
    rig.sql("UPDATE jobs SET kind = 'docs' WHERE key = 'd';");
    let owner = rig
        .env
        .home
        .config()
        .parent()
        .unwrap()
        .join("policy/acme/owner.toml");
    std::fs::write(owner, OWNER_NO_DOCS).unwrap();
    refused(
        edit_draft(&mut rig, &["--workflow", "api"]),
        "job edit --workflow of a docs draft",
    );
}
