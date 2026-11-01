//! Helpers of the `cpd secret` tests (commands.md §24): a fake `op` that only logs its argv, the Sessions
//! and Attempts the rules need, and a way to run `cpd secret ...` as each of them. Not a test file itself.

use crate::common::{self, CPD, Run, TestEnv};

use std::fs;
use std::io::Write;
use std::process::Stdio;

use serde_json::Value;

/// Who runs the command: the Session, and the Attempt for a Worker.
#[derive(Clone, Copy, Debug)]
pub enum Who {
    /// no `CPD_SESSION`
    Nobody,
    /// `CPD_SESSION` empty
    Empty,
    /// a made-up `CPD_SESSION`
    Unknown,
    /// the Lead of Plan `pl`
    Lead,
    /// the Manager of Context `ctx`
    Manager,
    /// the Worker of the running Attempt `a-a1` of Plan `pl`
    Worker,
}

pub struct Fx {
    pub env: TestEnv,
}

impl Fx {
    /// Context `ctx`; Plan `pl` (Lead `lead1`, Manager `mgr1`) with the Jobs `a` (Attempt `a-a1`, running, Worker
    /// `w-a`) and `e` (Attempt `e-a1`, ended); Plan `pl2` (Lead `lead2`) with Job `b` (Attempt `b-a1`, running).
    /// A fake `op` in `bin/` logs one line per call.
    pub fn new() -> Fx {
        let env = TestEnv::new();
        let fx = Fx { env };
        fs::write(
            fx.env.cpd_home().join("config.toml"),
            "version = 1\nmachine = \"m1\"\n",
        )
        .unwrap();
        let fp = "0".repeat(64);
        let job = |plan: &str, key: &str, pr: &str| {
            format!(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('{plan}', '{pr}', '{key}', 'J', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');
                 UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = '{key}';
                 UPDATE jobs SET status = 'running' WHERE key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                   VALUES ('{plan}', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/{plan}--{key}-a1', '/w', 'w-{key}', 't');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('w-{key}', 'worker', 'ctx', '{plan}', '{key}-a1', '{key}-a1', '/w', 'S2', 'working', 't', 't');"
            )
        };
        fx.env.sql(&format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', NULL, 't', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't'), ('pl2', 'ctx', 'P2', 'grounding', 'lead2', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't'),
                      ('lead2', 'lead', 'ctx', 'pl2', 'L2', '/', 'O3', 'idle', 't', 't');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr1', 'manager', 'ctx', 'M', '/', 'O3', 'idle', 't', 't');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, name, created_txn, created_at)
               VALUES ('pl', 'p1', '/r', 'main', 'waiting', 'cpd/pl/p1', NULL, cpd_txn(), 't'),
                      ('pl2', 'p2', '/r', 'main', 'waiting', 'cpd/pl2/p2', NULL, cpd_txn(), 't');
             {} {} {}",
            job("pl", "a", "p1"),
            job("pl", "e", "p1"),
            job("pl2", "b", "p2")
        ));
        fx.env.sql(
            "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'e-a1';",
        );
        let (log, dir) = (fx.op_log_path(), fx.op_dir());
        fs::create_dir_all(&dir).unwrap();
        // The fake `op` logs its argv (one line) and, for `item create` and `item edit`, keeps its stdin in
        // `op/stdin.<sub>`. It answers by subcommand (the word after `item`) from `op/<sub>.out`, `.err` and `.code`.
        common::write_executable(
            &fx.env.bin().join("op"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nsub=''\nprev=''\nfor a in \"$@\"; do\n  if [ \"$prev\" = item ]; then sub=$a; break; fi\n  prev=$a\ndone\ncase \"$sub\" in create|edit) cat > '{d}'/stdin.\"$sub\";; esac\n[ -f '{d}'/\"$sub\".out ] && cat '{d}'/\"$sub\".out\n[ -f '{d}'/\"$sub\".err ] && cat '{d}'/\"$sub\".err >&2\ncode=0\n[ -f '{d}'/\"$sub\".code ] && code=$(cat '{d}'/\"$sub\".code)\nexit \"$code\"\n",
                log = log.display(),
                d = dir.display()
            ),
        );
        fx
    }

    fn op_dir(&self) -> std::path::PathBuf {
        self.env.root().join("op")
    }

    fn op_log_path(&self) -> std::path::PathBuf {
        self.env.root().join("op.log")
    }

    /// The fake `op`'s calls, one argv line each, in order.
    pub fn op_log(&self) -> Vec<String> {
        fs::read_to_string(self.op_log_path())
            .map(|t| t.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    pub fn assert_op_not_called(&self, run: &Run) {
        assert_eq!(
            self.op_log(),
            Vec::<String>::new(),
            "op was called: {run:?}"
        );
    }

    /// `cpd <args>` as `who`, with `stdin` (none: a closed stdin).
    pub fn run(&self, who: Who, args: &[&str], stdin: Option<&str>) -> Run {
        self.run_with_path(who, args, stdin, None)
    }

    /// Like [`Fx::run`], with `PATH` set to `path` when given.
    pub fn run_with_path(
        &self,
        who: Who,
        args: &[&str],
        stdin: Option<&str>,
        path: Option<&std::ffi::OsStr>,
    ) -> Run {
        let mut cmd = self.env.command(CPD);
        cmd.args(args);
        if let Some(path) = path {
            cmd.env("PATH", path);
        }
        match who {
            Who::Nobody => {}
            Who::Empty => {
                cmd.env("CPD_SESSION", "");
            }
            Who::Unknown => {
                cmd.env("CPD_SESSION", "no-such-session");
            }
            Who::Lead => {
                cmd.env("CPD_SESSION", "lead1");
            }
            Who::Manager => {
                cmd.env("CPD_SESSION", "mgr1");
            }
            Who::Worker => {
                cmd.env("CPD_SESSION", "w-a")
                    .env("CPD_ATTEMPT", "a-a1")
                    .env("CPD_PLAN", "pl");
            }
        }
        let out = match stdin {
            None => common::output(&mut cmd),
            Some(text) => {
                let mut child = common::spawn(
                    cmd.stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped()),
                )
                .expect("spawn cpd");
                // A command that refuses before it reads its stdin has exited: the write may meet a closed pipe, and the Run says why.
                match child.stdin.take().unwrap().write_all(text.as_bytes()) {
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                    other => other.unwrap(),
                }
                child.wait_with_output()
            }
        }
        .expect("run cpd");
        Run {
            exit: out.status.code().expect("cpd was killed by a signal"),
            stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }
}

/// The command line of `command` (`get`, `list`, `create`, `edit`, `delete`, `allow`, `approve`) for the
/// names given. Every value is bound with `=` or after `--`, so a name that starts with `-` reaches cpd's own check.
/// `allow` names Attempt `a-a1`; `approve` the action `get` (`list` for a list-less `item`: use [`approve_args`]).
pub fn args(command: &str, vault: &str, item: &str, field: &str) -> Vec<String> {
    let v = format!("--vault={vault}");
    let f = format!("--field={field}");
    let mut a: Vec<String> = vec!["secret".into(), command.into()];
    match command {
        "get" | "create" | "edit" => a.extend([v, f]),
        "list" => a.push(v),
        "delete" => a.push(v),
        "allow" => a.extend(["--attempt=a-a1".into(), v, f]),
        "approve" => return approve_args("get", vault, Some(item), Some(field)),
        other => panic!("not a secret command: {other}"),
    }
    if command != "list" {
        a.extend(["--".into(), item.into()]);
    }
    a
}

pub fn approve_args(
    action: &str,
    vault: &str,
    item: Option<&str>,
    field: Option<&str>,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "secret".into(),
        "approve".into(),
        format!("--action={action}"),
        format!("--vault={vault}"),
    ];
    if let Some(i) = item {
        a.push(format!("--item={i}"));
    }
    if let Some(f) = field {
        a.push(format!("--field={f}"));
    }
    a
}

pub fn strs(a: &[String]) -> Vec<&str> {
    a.iter().map(String::as_str).collect()
}

/// The refusal `code` with `exit`. Not `common::refused`: a `details.next` of a secret refusal is prose
/// (`ask the Lead with cpd ask`), not a command line. Returns the `error` object.
pub fn denied(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}
