//! The engine crate's test kit (`#[cfg(test)]`): an [`Engine`] on a temporary home, a store to prepare and read, and
//! the fixtures the step tests share (Job, running Job, `ago`). No repo, no tmux, no process: a step test calls one step
//! (`inbox::run`, `open::run`, ...) once per tick on [`Fx::engine`] and reads the store.
//!
//! The engine's `Tmux` runs a program that does not exist, so a step that reaches for tmux fails loud instead of
//! starting one. Time: steps read `OffsetDateTime::now_utc()`, so a fixture dated with [`ago`] needs no clock.

use cpd_core::clock::timestamp;
use cpd_core::config::{Catalog, TierModel};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::host::runner::FakeRunner;
use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint};
use rusqlite::types::FromSql;
use serde_json::Value;
use time::OffsetDateTime;

use super::engine::Engine;

/// The stored title of Workflow `repo` of the Plan [`Fx::with_plan`] adds.
pub(super) const PR_TITLE: &str = "Charge the card at checkout";

/// A time `minutes` ago, as cpd writes times (contract §1.3).
pub(super) fn ago(minutes: i64) -> String {
    timestamp(OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
}

/// A time `seconds` ago, as cpd writes times.
pub(super) fn ago_seconds(seconds: i64) -> String {
    timestamp(OffsetDateTime::now_utc() - time::Duration::seconds(seconds))
}

/// The catalog pi would give (contract §9): O1-O5, S0-S5, H0-H3.
fn catalog() -> Catalog {
    Catalog {
        pi_version: "1.0.0".into(),
        o: TierModel {
            model: "p/opus".into(),
            efforts: vec![1, 2, 3, 4, 5],
        },
        s: TierModel {
            model: "p/sonnet".into(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        },
        h: TierModel {
            model: "p/haiku".into(),
            efforts: vec![0, 1, 2, 3],
        },
        g: TierModel {
            model: "p/gemini".into(),
            efforts: vec![],
        },
    }
}

/// One row of the events table: `(plan, job, attempt, data)`.
pub(super) type EventRow = (Option<String>, Option<String>, Option<String>, Value);

/// An engine on a temporary home, and the home's directory kept for as long as the test runs.
pub(super) struct Fx {
    pub engine: Engine,
    dir: tempfile::TempDir,
}

impl Fx {
    /// A home whose `config.toml` is the minimum plus `config_extra` (top-level keys, then tables), and the Context `ctx`.
    pub(super) fn new(config_extra: &str) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("config.toml"),
            format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n{config_extra}\n[tmux]\nsocket = \"testkit\"\n"),
        )
        .unwrap();
        let home = CpdHome::resolve(Some(root.into()), None).unwrap();
        let mut engine = Engine::for_test(home, no_tmux());
        engine.set_catalog(catalog());
        let mut fx = Fx { engine, dir };
        fx.fixture("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
        fx
    }

    /// `new`, then Plan `pay` in `dispatch` with its Workflow `repo` (`dispatch`) and its Lead Session `lead1` (`stopped`).
    pub(super) fn with_plan(config_extra: &str) -> Fx {
        Fx::with_plan_in("dispatch", config_extra)
    }

    /// `with_plan` with the Plan in `phase` (`grounding` is not locked).
    pub(super) fn with_plan_in(phase: &str, config_extra: &str) -> Fx {
        let locked = if phase == "grounding" { "NULL" } else { "'t'" };
        let mut fx = Fx::new(config_extra);
        let repo = fx.dir.path().join("repo");
        let lead = fx.dir.path().join("lead");
        fx.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay', '{phase}', 'lead1', 't', {locked});
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
               VALUES ('pay', 'repo', '{}', 'main', 'dispatch', 'cpd/pay', '{}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
        fx
    }

    /// The engine's tmux on the in-process tmux (the `tmux` route of a fresh `FakeRunner`), no server running yet
    /// ([`Fx::tmux_server`]). Returns the runner: a test scripts a fault on it (`once`, `on`: see the fault injections in
    /// `cpd_core::fakes::tmux::route`) and reads what the step sent (`calls`).
    pub(super) fn on_fake_tmux(&mut self) -> FakeRunner {
        let runner = FakeRunner::with_tmux();
        let tmpdir = self.dir.path().join("tmux");
        std::fs::create_dir_all(&tmpdir).unwrap();
        self.engine.tmux =
            Tmux::with_tmpdir("testkit", tmpdir).with_runner(std::sync::Arc::new(runner.clone()));
        runner
    }

    /// A tmux server with one session, `keepalive`, on the tmux of [`Fx::on_fake_tmux`].
    pub(super) fn tmux_server(&self) {
        self.engine
            .tmux
            .run(&["new-session", "-d", "-s", "keepalive", "sleep", "600"].map(str::to_owned))
            .unwrap();
    }

    /// A second engine on the same home: the engine after a restart.
    pub(super) fn restarted(&self) -> Engine {
        let mut engine = Engine::for_test(self.engine.home.clone(), no_tmux());
        engine.set_catalog(catalog());
        engine
    }

    /// Runs `sql` in one transaction.
    pub(super) fn fixture(&mut self, sql: &str) {
        self.engine
            .store
            .write("fixture", |tx| Ok(tx.execute_batch(sql)?))
            .unwrap();
    }

    /// The first column of the first row of `sql`.
    pub(super) fn query<T: FromSql>(&mut self, sql: &str) -> T {
        self.engine
            .store
            .read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// Job `key` of Plan `pay` in Workflow `repo`, `queued` with its Locked-Job fingerprint, then moved to another status by `then`
    /// (SQL run after the fingerprint is set).
    pub(super) fn add_job(&mut self, key: &str, needs: &[&str], then: &str) {
        self.add_job_in("repo", key, needs, then);
    }

    /// `add_job` in Workflow `pr` of Plan `pay`.
    pub(super) fn add_job_in(&mut self, workflow: &str, key: &str, needs: &[&str], then: &str) {
        self.add_job_on("S2", workflow, key, needs, then);
    }

    /// `add_job_in` with the Job's Tier `tier`.
    pub(super) fn add_job_on(
        &mut self,
        tier: &str,
        workflow: &str,
        key: &str,
        needs: &[&str],
        then: &str,
    ) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{workflow}', '{key}', 'Title of {key}', '{tier}', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!(
                "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"
            );
        }
        self.fixture(&sql);
        self.lock_job(key, then);
    }

    /// Job `key` `queued` with its fingerprint (the Job as the lock leaves it), then moved by `then` (SQL).
    pub(super) fn lock_job(&mut self, key: &str, then: &str) {
        let plan = PlanId::parse("pay").unwrap();
        let all = self
            .engine
            .store
            .read(|conn| jobs::load(conn, &plan))
            .unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}'; {then}",
            locked_fingerprint(job)
        ));
    }

    /// Job `key` `running` with Attempt `<key>-a1` in `status`, started at `started_at`, its driver (pid 4242) started.
    /// A `running` Attempt has its `starting → running` `attempt-status` Event at `started_at` too.
    pub(super) fn add_running(&mut self, key: &str, status: &str, started_at: &str) {
        self.add_running_since(
            key,
            status,
            started_at,
            (status == "running").then_some(started_at),
        );
    }

    /// `add_running` with the `starting → running` Event at `running_at` (none: the Attempt is still `starting`).
    pub(super) fn add_running_since(
        &mut self,
        key: &str,
        status: &str,
        started_at: &str,
        running_at: Option<&str>,
    ) {
        let worktree = self.dir.path().join(format!("pay.pay--{key}-a1"));
        let event = running_at
            .map(|at| {
                format!(
                    "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
                       VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', '{key}', '{key}-a1', 'running',
                               '{{\"from\":\"starting\",\"to\":\"running\"}}');"
                )
            })
            .unwrap_or_default();
        self.add_job(
            key,
            &[],
            &format!(
                "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
                   VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/pay--{key}-a1', '{}', '{started_at}', 4242);
                 {event}",
                worktree.display()
            ),
        );
    }

    /// Every Event of `kind`, in order: `(plan, job, attempt, data)`.
    pub(super) fn events(&mut self, kind: &str) -> Vec<EventRow> {
        self.engine
            .store
            .read(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT plan, job, attempt, data FROM events WHERE kind = ?1 ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([kind], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows
                    .into_iter()
                    .map(|(p, j, a, d)| (p, j, a, serde_json::from_str(&d).unwrap()))
                    .collect())
            })
            .unwrap()
    }

    pub(super) fn count(&mut self, kind: &str) -> usize {
        self.events(kind).len()
    }

    pub(super) fn job_status(&mut self, key: &str) -> String {
        self.query(&format!(
            "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
    }
}

/// A `Tmux` whose program does not exist: the steps these tests call never use it, and one that does fails.
fn no_tmux() -> Tmux {
    Tmux::new("testkit").with_program("/nonexistent/cpd-testkit-has-no-tmux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kit_adds_a_locked_job_and_a_running_one_with_its_attempt() {
        let mut fx = Fx::with_plan("");
        fx.add_job("api", &[], "");
        assert_eq!(fx.job_status("api"), "queued");
        assert!(fx.query::<bool>("SELECT fingerprint IS NOT NULL FROM jobs WHERE key = 'api'"));
        fx.add_job("web", &["api"], "");
        assert_eq!(
            fx.query::<i64>(
                "SELECT count(*) FROM dependencies WHERE job = 'web' AND needs = 'api'"
            ),
            1
        );

        let started = ago(5);
        fx.add_running("busy", "running", &started);
        assert_eq!(fx.job_status("busy"), "running");
        assert_eq!(
            fx.query::<i64>("SELECT driver_pid FROM attempts WHERE id = 'busy-a1'"),
            4242
        );
        assert_eq!(fx.events("attempt-status").len(), 1);
        fx.add_running("fresh", "starting", &started);
        assert_eq!(
            fx.count("attempt-status"),
            1,
            "a starting Attempt has no starting → running Event yet"
        );
    }

    #[test]
    fn the_fake_tmux_has_no_server_until_one_is_made_and_the_runner_records_what_the_engine_sent() {
        let mut fx = Fx::new("");
        let runner = fx.on_fake_tmux();
        assert!(!fx.engine.tmux.server_running().unwrap());

        fx.tmux_server();
        assert!(fx.engine.tmux.server_running().unwrap());
        fx.engine
            .tmux
            .run(&["set", "-g", "@x", "1"].map(str::to_owned))
            .unwrap();

        let sent: Vec<String> = runner.calls().iter().map(|c| c.args.join(" ")).collect();
        assert!(sent.iter().any(|a| a.contains("set -g @x 1")), "{sent:?}");
    }

    #[test]
    fn ago_is_relative_to_now_in_the_stores_time_format() {
        let (older, newer) = (ago(10), ago(1));
        assert!(older < newer, "{older} {newer}");
        assert_eq!(older.len(), "2026-10-20T11:00:00.000Z".len());
    }

    #[test]
    fn a_restarted_engine_shares_the_store_and_the_tmux_is_not_a_real_one() {
        let mut fx = Fx::new("");
        let mut second = fx.restarted();
        fx.fixture("INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES ('dialog', 'red', 's', 'l', 't');");
        let seen: i64 = second
            .store
            .read(|conn| Ok(conn.query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(seen, 1);
        assert!(
            fx.engine.tmux.server_running().is_err(),
            "the kit's tmux has no program: any use of it is an error"
        );
    }
}
