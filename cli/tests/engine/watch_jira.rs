//! Engine step E19, the Jira transition queue (contract §3.7 E19, §4.4 `jira_transitions`; commands.md §21.11, §21.12;
//! decisions pro-d126, pro-d128): the engine runs the helper `pi/lib/jira-transition.ts` once, detached, for the oldest
//! PENDING `jira_transitions` row whose PR has a ticket and is not cancelled. The row is then SETTLED (`moved`,
//! `unchanged`, `skipped`), PARKED (the helper refused it: nothing is tried again until the ticket or the status name
//! changes) or left PENDING (any other failure, tried again `watch.error_repeat_seconds` after the failed run's end). A
//! failure of the row itself is one Plan-scoped `error` Event per (Plan, PR, target, code) until the row ends; `jira_auth`
//! and `jira_not_configured` stay one machine-wide Event each (contract §3.7 E19, rules R1-R5).
//!
//! In-process, on virtual time, like `watch_react.rs` (whose harness this file copies, not widens): the engine runs on
//! the `Fx`'s `ManualClock`, `FakeRunner` and `FakeSpawn`; the helper is the `node` route of the `FakeRunner`. No process,
//! no Jira, no sleep: an engine run ends when a state the test reads holds (or at [`CAP`] naps, so that a step that never
//! runs fails the test on its assertion instead of running for ever). Every test that says "nothing runs" has a control:
//! a row that must run, so that the silence is shown after the step proved it is alive.
//!
//! The helper runs on a thread of its own and answers when its thread is scheduled, not when the engine ticks: a test
//! never counts on a tick for an answer, it runs the engine until the row or the Event it checks is there.
//!
//! Fixture: Plan `pay` of Context `ctx` (owner `acme`), PR `api` (ticket `ABC-123`) and PR `web` (ticket `ABC-200`), both
//! open, the repo `acme/pay`; the owner's `[pr.jira]` names the three statuses; the machine policy has the server
//! `atlassian`. The table, the keys and the step do not exist until the engine step is built: the tests fail at run time.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use common::healthy::PI_RELEASE;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::store::Store;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

/// `watch.error_repeat_seconds` of the tests that do not look at it: far above the virtual time of any run.
const LONG: u32 = 3600;

/// The repeat period of the tests that look at it.
const REPEAT: u32 = 600;

const API: &str = "ABC-123";
const WEB: &str = "ABC-200";

/// The helper's answers (`commands.md` §21.11).
const MOVED: &str = r#"0 {"ok":true,"ticket":"ABC-123","target":"in_review","outcome":"moved","from":"In Progress","to":"In Review"}"#;
const UNCHANGED: &str = r#"0 {"ok":true,"ticket":"ABC-123","target":"done","outcome":"unchanged","status":"Done","why":"at_target"}"#;
const REFUSED: &str = r#"1 {"ok":false,"error":{"code":"jira_refused","message":"No transition leads to In Review.","details":{"reason":"no_transition","tool":"getTransitionsForJiraIssue"}}}"#;
const AUTH: &str = r#"3 {"ok":false,"error":{"code":"jira_auth","message":"Atlassian needs a sign-in. Run: pi mcp login atlassian","details":{}}}"#;
const FAILED: &str = r#"1 {"ok":false,"error":{"code":"jira_failed","message":"transitionJiraIssue failed.","details":{"tool":"transitionJiraIssue","status_code":503}}}"#;
const FAILED_POINTS: &str = r#"1 {"ok":false,"error":{"code":"jira_failed","message":"transitionJiraIssue returned an error: Please add story points","details":{"tool":"transitionJiraIssue","status_code":400}}}"#;
const HELPER_FAILED: &str = r#"1 {"ok":false,"error":{"code":"jira_helper_failed","message":"The helper failed.","details":{}}}"#;
/// The fake prints the text after the exit status: stdout is not JSON.
const NO_JSON: &str = "70 the helper crashed";

/// The naps of the engine between two ticks: a tick is a second (`tick_seconds = 1`) and a nap a tenth of one.
const NAPS_PER_TICK: usize = 10;

/// The most naps a run lasts: 150 virtual seconds. Every test's state holds within a few ticks.
const CAP: usize = 1500;

/// How long a gated helper waits for its gate before it gives up: only a regression waits that long.
const GATE_DEADLINE: Duration = Duration::from_secs(30);

/// The helper held in flight: it says it started, then waits for the gate to open.
struct Gate {
    entered: Mutex<Sender<()>>,
    open: Mutex<Receiver<()>>,
}

/// The `node` route: the Jira helper. It answers `<exit> <json>` (the answer of the request's ticket when there is one,
/// else what `says` holds) and keeps every call. The first call waits for its gate when there is one.
struct Node {
    says: Mutex<String>,
    /// `(ticket, answer)`: what the helper answers for a ticket, whatever `says` holds.
    per_ticket: Mutex<Vec<(String, String)>>,
    seen: Mutex<Vec<Call>>,
    gate: Mutex<Option<Gate>>,
}

fn request_of(call: &Call) -> Value {
    serde_json::from_slice(call.stdin.as_ref().expect("the helper's request is on its stdin")).expect("the request is JSON")
}

impl Handler for Node {
    fn handle(&self, call: &Call) -> Reply {
        self.seen.lock().unwrap().push(call.clone());
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.entered.lock().unwrap().send(()).expect("the test holds the receiver of `entered`");
            if gate.open.lock().unwrap().recv_timeout(GATE_DEADLINE).is_err() {
                return Reply::exit(70, "", "the gate of the helper never opened");
            }
        }
        let ticket = request_of(call)["ticket"].as_str().unwrap_or_default().to_owned();
        let own = self.per_ticket.lock().unwrap().iter().find(|(t, _)| *t == ticket).map(|(_, a)| a.clone());
        let answer = own.unwrap_or_else(|| self.says.lock().unwrap().clone());
        let (status, json) = answer.split_once(' ').expect("an answer is `<exit> <json>`");
        Reply::exit(status.parse().expect("the exit status is a number"), &format!("{json}\n"), "")
    }
}

/// `(settled_at, outcome, refused_at, refusal, refused_with)` of a queue row.
type Row = (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);

/// What a test reads while the engine runs: `Clone` and `'static`, so a step of the run holds one.
#[derive(Clone)]
struct Probe {
    home: PathBuf,
    node: Arc<Node>,
    clock: ManualClock,
}

impl Probe {
    fn calls(&self) -> Vec<Call> {
        self.node.seen.lock().unwrap().clone()
    }

    /// The request of each call of the helper, in order.
    fn requests(&self) -> Vec<Value> {
        self.calls().iter().map(request_of).collect()
    }

    /// The `ticket` of each call of the helper, in order.
    fn tickets(&self) -> Vec<String> {
        self.requests().iter().map(|r| r["ticket"].as_str().unwrap().to_owned()).collect()
    }

    /// The `target` of each call of the helper, in order.
    fn targets(&self) -> Vec<String> {
        self.requests().iter().map(|r| r["target"].as_str().unwrap().to_owned()).collect()
    }

    fn says(&self, answer: &str) {
        *self.node.says.lock().unwrap() = answer.to_owned();
    }

    fn says_for(&self, ticket: &str, answer: &str) {
        self.node.per_ticket.lock().unwrap().push((ticket.to_owned(), answer.to_owned()));
    }

    fn store(&self) -> Connection {
        Connection::open_with_flags(self.home.join("cpd.db"), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    fn row(&self, pr: &str, target: &str) -> Row {
        self.store()
            .query_row(
                "SELECT settled_at, outcome, refused_at, refusal, refused_with FROM jira_transitions WHERE plan = 'pay' AND pr = ?1 AND target = ?2",
                [pr, target],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
    }

    fn id(&self, pr: &str, target: &str) -> i64 {
        self.store().query_row("SELECT id FROM jira_transitions WHERE plan = 'pay' AND pr = ?1 AND target = ?2", [pr, target], |r| r.get(0)).unwrap()
    }

    fn settled(&self, pr: &str, target: &str) -> bool {
        self.row(pr, target).0.is_some()
    }

    fn parked(&self, pr: &str, target: &str) -> bool {
        self.row(pr, target).2.is_some()
    }

    fn pending(&self, pr: &str, target: &str) -> bool {
        self.row(pr, target) == (None, None, None, None, None)
    }

    /// Every `error` Event but those of `gh` (E8's one lookup is not E19's): its Plan, its text and its data.
    fn error_events(&self) -> Vec<(Option<String>, String, Value)> {
        let conn = self.store();
        let mut stmt = conn.prepare("SELECT plan, text, data FROM events WHERE kind = 'error' ORDER BY id").unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows.into_iter()
            .map(|(plan, text, data)| (plan, text, serde_json::from_str::<Value>(&data).unwrap()))
            .filter(|(_, _, d)| d["code"].as_str().is_some_and(|c| c != "gh_failed"))
            .collect()
    }

    fn codes(&self) -> Vec<String> {
        self.error_events().iter().map(|(_, _, d)| d["code"].as_str().unwrap().to_owned()).collect()
    }

    /// The kinds of the Events the engine appended but its own `watch-started` and `watch-stopped`, `error` Events apart.
    fn events_but_errors(&self) -> Vec<String> {
        let conn = self.store();
        let mut stmt = conn.prepare("SELECT kind FROM events WHERE kind NOT IN ('watch-started', 'watch-stopped', 'error') ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.home.join("run/watch.log")).unwrap_or_default()
    }

    /// A write of the store, as another process's: a fixture the engine finds at its next tick.
    fn write(&self, sql: &str) {
        common::sql(&mut Store::open(&CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(), 5000).unwrap(), sql);
    }

    /// Queues the row `(pr, target)` of Plan `pay`, as a step of cpd would.
    fn queue(&self, pr: &str, target: &str) {
        self.write(&queue_sql(pr, target));
    }

    /// Moves the engine's clock forward by `seconds`.
    fn skip(&self, seconds: u64) {
        self.clock.advance(Duration::from_secs(seconds));
    }

    /// Replaces the owner's policy file, as the User's edit: the engine finds it at its next tick.
    fn owner_policy(&self, toml: &str) {
        fs::write(self.home.join("policy/acme/owner.toml"), toml).unwrap();
    }

    /// Replaces the machine policy file.
    fn host_policy(&self, toml: &str) {
        fs::write(self.home.join("policy/host.toml"), toml).unwrap();
    }
}

fn queue_sql(pr: &str, target: &str) -> String {
    format!("INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', '{pr}', '{target}', 't');")
}

/// The owner's policy: the status names of the three targets.
fn owner_toml(in_progress: &str, in_review: &str, done: &str) -> String {
    format!("[pr.jira]\nin_progress = \"{in_progress}\"\nin_review = \"{in_review}\"\ndone = \"{done}\"\n")
}

fn names() -> String {
    owner_toml("In Progress", "In Review", "Done")
}

const SERVER: &str = "[mcp.servers.atlassian]\nurl = \"https://mcp.atlassian.com/v1/mcp/authv2\"\nexposure = \"deferred\"\n";

struct Jira {
    fx: Fx,
    node: Arc<Node>,
}

/// A fake pi install, the adapter directory, a `gh` for E8's one lookup, the helper route answering [`MOVED`],
/// `watch.error_repeat_seconds = repeat`, the machine policy with the server `atlassian` (`server`), the owner's policy
/// naming the three statuses; Plan `pay` (its Lead stopped, the Plan done now) in Context `ctx` owned by `acme`, with the
/// open PRs `api` (ticket [`API`]) and `web` (ticket [`WEB`]). No queue row.
fn jira(repeat: u32, server: bool) -> Jira {
    let fx = Fx::with_ports();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(adapter.join("lib")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\nerror_repeat_seconds = {repeat}\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.engine_routes();
    fx.install_pi();
    fx.fake_gh(Reply::ok(r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#));
    let node = Arc::new(Node { says: Mutex::new(MOVED.to_owned()), per_ticket: Mutex::new(Vec::new()), seen: Mutex::new(Vec::new()), gate: Mutex::new(None) });
    let _ = (*fx.fakes.runner).clone().route("node", node.clone());
    fx.env.owner_policy("acme", &names());
    fx.env.repo_policy("acme", "pay", "");
    if server {
        let policy = fx.env.cpd_home().join("policy");
        fs::create_dir_all(&policy).unwrap();
        fs::write(policy.join("host.toml"), SERVER).unwrap();
    }
    let now = fx.stamp_ago(0);
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on, owner) VALUES ('ctx', 'Ctx', 't', 'm1', 'acme');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', '{now}', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'pay-lead', 't', 't', '{now}');"
    ));
    let t = Jira { fx, node };
    t.pr("api", 1, Some(API), "open");
    t.pr("web", 2, Some(WEB), "open");
    t
}

impl Jira {
    fn probe(&self) -> Probe {
        Probe { home: self.fx.env.cpd_home(), node: self.node.clone(), clock: (*self.fx.fakes.clock).clone() }
    }

    /// PR `key` of Plan `pay` (the repo `acme/pay`), pull request `number`, with `ticket`, `open` or `cancelled`.
    fn pr(&self, key: &str, number: u32, ticket: Option<&str>, status: &str) {
        let jira = ticket.map_or("NULL".to_owned(), |t| format!("'{t}'"));
        let sql = if status == "cancelled" {
            format!(
                "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at, name, jira)
                   VALUES ('pay', '{key}', '/nowhere', 'main', 'cancelled', 'cpd/pay/{key}', cpd_txn(), 't', 'pay', {jira});"
            )
        } else {
            let url = format!("https://github.com/acme/pay/pull/{number}");
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, authored)
                   VALUES ('acme/pay', {number}, '{url}', 'PR {key}', 0, 'open', 'none', 'none', 't', NULL, 't', 0);
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, name, jira)
                   VALUES ('pay', '{key}', '/nowhere', 'main', 'open', 'cpd/pay/{key}', '/nowhere/lead', {number}, '{url}', 't',
                           (SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = {number}), cpd_txn(), 't', 't', 'pay', {jira});"
            )
        };
        self.fx.env.sql(&sql);
    }

    fn queue(&self, pr: &str, target: &str) {
        self.fx.env.sql(&queue_sql(pr, target));
    }

    /// The engine runs until `step` says so (it is called before each nap, after the tick that came before it) or [`CAP`] naps.
    fn run(&self, mut step: impl FnMut(&Probe, usize) -> bool + Send + 'static) {
        let probe = self.probe();
        self.fx.watch_steps(move |nap| nap >= CAP || step(&probe, nap)).unwrap();
    }
}

/// `done` once `cond` has held for `ticks` whole ticks: the state the engine reached, and that it stays so.
fn then_ticks(ticks: usize, mut cond: impl FnMut(&Probe) -> bool + Send + 'static) -> impl FnMut(&Probe, usize) -> bool + Send + 'static {
    let mut since = None;
    move |p, nap| {
        if since.is_none() && cond(p) {
            since = Some(nap);
        }
        since.is_some_and(|at| nap >= at + ticks * NAPS_PER_TICK)
    }
}

/// Runs until the row `(pr, target)` is parked and three ticks more.
fn park(t: &Jira, pr: &str, target: &str) -> Probe {
    t.probe().says(REFUSED);
    let (pr, target) = (pr.to_owned(), target.to_owned());
    t.run(then_ticks(3, move |p| p.parked(&pr, &target)));
    t.probe()
}

#[test]
fn a_due_row_runs_the_helper_with_the_request_and_settles_moved() {
    let t = jira(LONG, true);
    t.queue("api", "in_review");
    t.run(then_ticks(3, |p| p.settled("api", "in_review")));
    let p = t.probe();

    let calls = p.calls();
    assert_eq!(calls.len(), 1, "one run, and later ticks run none: {calls:?}");
    // The product reports canonical paths: /var is a symlink into /private on a Mac.
    let root = t.fx.env.root().canonicalize().unwrap();
    let argv: Vec<&str> = calls[0].args.iter().map(String::as_str).collect();
    assert_eq!(
        argv,
        [
            root.join("adapter/lib/jira-transition.ts").to_str().unwrap(),
            "--coding-agent",
            root.join(format!("install/releases/{PI_RELEASE}/node_modules/@earendil-works/pi-coding-agent/dist/index.js")).to_str().unwrap(),
        ]
    );
    assert_eq!(
        request_of(&calls[0]),
        json!({
            "server": {"url": "https://mcp.atlassian.com/v1/mcp/authv2", "exposure": "deferred"},
            "ticket": API,
            "target": "in_review",
            "names": {"in_progress": "In Progress", "in_review": "In Review", "done": "Done"},
            "estimate_fields": []
        })
    );
    let env = |name: &str| calls[0].envs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_deref());
    assert_eq!(env("PI_CODING_AGENT_DIR"), Some(Some(root.to_str().unwrap())));
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert_eq!(env(removed), Some(None), "{removed} is removed from the helper's environment");
    }
    assert_eq!(calls[0].cwd, Some(t.fx.env.cpd_home().canonicalize().unwrap()));
    assert_eq!(calls[0].deadline, Duration::from_secs(120));

    let (settled_at, outcome, refused_at, _, _) = p.row("api", "in_review");
    assert!(settled_at.is_some_and(|at| at.ends_with('Z')));
    assert_eq!((outcome.as_deref(), refused_at), (Some("moved"), None));
    assert_eq!(p.error_events(), vec![]);
    assert_eq!(p.events_but_errors(), Vec::<String>::new(), "a success appends no Event");
}

#[test]
fn the_request_carries_the_estimate_fields_of_the_policy() {
    let t = jira(LONG, true);
    t.probe().owner_policy(&format!("{}estimate_fields = [\"customfield_10016\"]\n", names()));
    t.queue("api", "done");
    t.queue("web", "in_progress");
    t.run(then_ticks(3, |p| p.settled("api", "done") && p.settled("web", "in_progress")));
    let requests = t.probe().requests();

    assert_eq!(requests.len(), 2, "{requests:?}");
    for request in &requests {
        assert_eq!(request["estimate_fields"], json!(["customfield_10016"]), "{request}");
    }
}

#[test]
fn an_unchanged_answer_settles_the_row_unchanged() {
    let t = jira(LONG, true);
    t.probe().says(UNCHANGED);
    t.queue("api", "done");
    t.run(then_ticks(3, |p| p.settled("api", "done")));
    let p = t.probe();

    assert_eq!(p.calls().len(), 1);
    let (settled_at, outcome, refused_at, _, _) = p.row("api", "done");
    assert!(settled_at.is_some());
    assert_eq!((outcome.as_deref(), refused_at), (Some("unchanged"), None));
    assert_eq!(p.error_events(), vec![]);
}

#[test]
fn a_target_with_no_status_name_settles_skipped_without_calling_the_helper() {
    let t = jira(LONG, true);
    t.fx.env.owner_policy("acme", &owner_toml("In Progress", "", "Done"));
    // The row of `in_review` (no name) is queued first; the control, the row of `in_progress`, is behind it.
    t.queue("api", "in_review");
    t.queue("api", "in_progress");
    t.run(then_ticks(3, |p| p.settled("api", "in_review") && p.settled("api", "in_progress")));
    let p = t.probe();

    let (settled_at, outcome, ..) = p.row("api", "in_review");
    assert!(settled_at.is_some());
    assert_eq!(outcome.as_deref(), Some("skipped"));
    assert_eq!(p.targets(), ["in_progress"], "no helper call for the target with no name; the control runs");
    assert_eq!(p.row("api", "in_progress").1.as_deref(), Some("moved"));
    assert_eq!(p.error_events(), vec![]);
}

#[test]
fn a_row_of_a_pr_with_no_ticket_waits_and_runs_once_the_ticket_is_set() {
    let t = jira(LONG, true);
    t.fx.env.sql("UPDATE prs SET jira = NULL WHERE plan = 'pay' AND key = 'web';");
    t.queue("web", "in_progress");
    t.queue("api", "in_progress");
    t.run(then_ticks(5, |p| p.settled("api", "in_progress")));
    let p = t.probe();

    assert_eq!(p.tickets(), [API], "only the control, the PR with a ticket, runs");
    assert!(p.pending("web", "in_progress"), "the row of the PR with no ticket waits, with nothing else written");
    assert_eq!(p.error_events(), vec![]);

    p.write("UPDATE prs SET jira = 'ABC-9' WHERE plan = 'pay' AND key = 'web';");
    t.run(then_ticks(3, |p| p.settled("web", "in_progress")));
    assert_eq!(p.tickets(), [API, "ABC-9"], "the ticket is set: the row runs with it");
    assert_eq!(p.row("web", "in_progress").1.as_deref(), Some("moved"));
}

#[test]
fn rows_run_oldest_first_one_at_a_time() {
    let t = jira(LONG, true);
    // The queue order is the id order, not the order of the targets.
    t.queue("api", "done");
    t.queue("api", "in_progress");
    t.queue("api", "in_review");
    // The first run is held in flight while the engine ticks on.
    let (entered_tx, entered) = channel();
    let (open_tx, open) = channel();
    *t.node.gate.lock().unwrap() = Some(Gate { entered: Mutex::new(entered_tx), open: Mutex::new(open) });
    let mut opened = false;
    let during = Arc::new(Mutex::new(None));
    let record = during.clone();
    t.run(move |p, nap| {
        if nap == 1 {
            // The first tick started the helper's thread: wait until its route has been called.
            entered.recv_timeout(Duration::from_secs(10)).expect("the first tick starts the helper");
        }
        if !opened && nap >= 1 + 2 * NAPS_PER_TICK {
            *record.lock().unwrap() = Some(p.targets());
            open_tx.send(()).unwrap();
            opened = true;
        }
        opened && p.settled("api", "done") && p.settled("api", "in_progress") && p.settled("api", "in_review")
    });
    let p = t.probe();

    assert_eq!(during.lock().unwrap().take().unwrap(), ["done"], "one at a time: the oldest row, and no second run while it is in flight");
    assert_eq!(p.targets(), ["done", "in_progress", "in_review"], "in id order");
    assert_eq!(p.error_events(), vec![]);
}

#[test]
fn a_refusal_parks_the_row_with_one_error_event_naming_the_ticket_the_status_and_the_reason() {
    let t = jira(LONG, true);
    t.queue("api", "in_review");
    let p = park(&t, "api", "in_review");

    assert_eq!(p.calls().len(), 1, "one try, then parked");
    let (settled_at, outcome, refused_at, refusal, refused_with) = p.row("api", "in_review");
    assert_eq!((settled_at, outcome), (None, None), "parked, not settled");
    let refused_at = refused_at.expect("parked");
    assert!(refused_at.ends_with('Z'), "{refused_at}");
    assert_eq!((refusal.as_deref(), refused_with.as_deref()), (Some("no_transition"), Some("ABC-123|In Review")));

    let events = p.error_events();
    assert_eq!(events.len(), 1, "one Event: {events:?}");
    let (plan, text, data) = &events[0];
    assert_eq!(plan.as_deref(), Some("pay"), "scoped to the Plan");
    assert_eq!((data["code"].as_str(), data["source"].as_str()), (Some("jira_refused"), Some("watch")));
    for word in ["ABC-123", "api", "pay", "In Review", "no_transition"] {
        assert!(text.contains(word) && data["message"].as_str().unwrap().contains(word), "{word}: {text}");
    }
    let id = p.id("api", "in_review");
    assert_eq!(
        data["details"],
        json!({"id": id, "plan": "pay", "pr": "api", "ticket": API, "target": "in_review", "status": "In Review", "reason": "no_transition"})
    );
    let at: String = p.store().query_row("SELECT at FROM events WHERE kind = 'error' AND data LIKE '%jira_refused%'", [], |r| r.get(0)).unwrap();
    assert_eq!(at, refused_at, "the Event and the parking are one transaction");
    let items: i64 = p.store().query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0)).unwrap();
    assert_eq!(items, 0, "a parked row is not an Inbox item");
}

#[test]
fn a_parked_row_is_not_run_again_by_a_later_tick_or_a_restarted_engine() {
    let t = jira(REPEAT, true);
    // `web` is the control: its ticket answers moved while `api`'s is refused.
    t.probe().says_for(WEB, MOVED);
    t.queue("api", "in_review");
    park(&t, "api", "in_review");
    let p = t.probe();
    p.queue("web", "in_progress");
    let mut skipped = false;
    let mut tail = then_ticks(10, |p| p.settled("web", "in_progress"));
    t.run(move |p, nap| {
        if !skipped {
            if p.settled("web", "in_progress") {
                p.skip(u64::from(REPEAT) * 5);
                skipped = true;
            }
            return false;
        }
        tail(p, nap)
    });
    assert_eq!(p.tickets(), [API, WEB], "the parked row is not run again, past the repeat period either");
    assert_eq!(p.error_events().len(), 1, "no second Event");
    assert!(p.parked("api", "in_review"));

    // A second engine on the same home, with fresh memory: the same, with a new control row.
    p.queue("web", "done");
    t.run(then_ticks(10, |p| p.settled("web", "done")));
    assert_eq!(p.tickets(), [API, WEB, WEB], "the restarted engine runs the control and not the parked row");
    assert_eq!(p.error_events().len(), 1, "no second Event");
    assert!(p.parked("api", "in_review"));
}

#[test]
fn a_parked_row_is_reopened_and_run_when_the_ticket_or_the_status_name_changes() {
    // Each case parks `api`'s `in_review` row, changes something, and runs the engine until the control row behind it is
    // settled too. (change, does it reopen the row, the ticket and the name of the second run)
    type Change = Box<dyn Fn(&Probe)>;
    let cases: Vec<(&str, Change, Option<(&str, &str)>)> = vec![
        ("another ticket", Box::new(|p| p.write("UPDATE prs SET jira = 'ABC-124' WHERE plan = 'pay' AND key = 'api';")), Some(("ABC-124", "In Review"))),
        ("another status name", Box::new(|p| p.owner_policy(&owner_toml("In Progress", "Under Review", "Done"))), Some((API, "Under Review"))),
        ("another title", Box::new(|p| p.write("UPDATE prs SET title = 'Another title' WHERE plan = 'pay' AND key = 'api';")), None),
        ("the same name, rewritten", Box::new(|p| p.owner_policy(&format!("# edited\n{}", names()))), None),
        ("an empty name", Box::new(|p| p.owner_policy(&owner_toml("In Progress", "", "Done"))), None),
    ];
    for (what, change, reopened) in cases {
        let t = jira(LONG, true);
        t.probe().says_for(WEB, MOVED);
        t.queue("api", "in_review");
        let p = park(&t, "api", "in_review");
        assert_eq!(p.calls().len(), 1, "{what}: parked after one try");
        p.says(MOVED);
        p.queue("web", "in_progress");
        change(&p);
        t.run(then_ticks(5, move |p| p.settled("web", "in_progress") && (reopened.is_none() || p.settled("api", "in_review"))));

        match reopened {
            Some((ticket, name)) => {
                let requests = p.requests();
                assert_eq!(requests.len(), 3, "{what}: the parked row ran again, and the control: {requests:?}");
                let again = requests.iter().skip(1).find(|r| r["target"] == "in_review").unwrap_or_else(|| panic!("{what}: no second run of the row: {requests:?}"));
                assert_eq!((again["ticket"].as_str(), again["names"]["in_review"].as_str()), (Some(ticket), Some(name)), "{what}");
                let (settled_at, outcome, refused_at, refusal, refused_with) = p.row("api", "in_review");
                assert!(settled_at.is_some(), "{what}");
                assert_eq!((outcome.as_deref(), refused_at, refusal, refused_with), (Some("moved"), None, None, None), "{what}: the parking is cleared");
            }
            None => {
                assert_eq!(p.tickets(), [API, WEB], "{what}: the row stays parked, only the control runs");
                assert!(p.parked("api", "in_review"), "{what}");
            }
        }
        assert_eq!(p.error_events().len(), 1, "{what}: the parking is the only Event");
    }
}

#[test]
fn an_auth_failure_is_reported_once_and_retried_never_parked() {
    let t = jira(REPEAT, true);
    t.probe().says(AUTH);
    t.queue("api", "in_progress");
    // The first run fails (its Event is the signal that its end was seen). Five ticks: no second call and no second Event.
    // Then the sign-in is back and the repeat period passes: the same row runs again and settles.
    let (mut failed_at, mut again) = (None, false);
    let mut tail = then_ticks(3, |p| p.settled("api", "in_progress"));
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    t.run(move |p, nap| {
        let Some(at) = failed_at else {
            if !p.error_events().is_empty() {
                failed_at = Some(nap);
            }
            return false;
        };
        if !again {
            if nap >= at + 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.codes(), p.pending("api", "in_progress")));
                p.says(MOVED);
                p.skip(u64::from(REPEAT) + 1);
                again = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(seen.lock().unwrap().take(), Some((1, vec!["jira_auth".to_owned()], true)), "one run, one Event, the row PENDING, never parked");
    assert_eq!(p.tickets(), [API, API], "after the repeat period the same row is tried again, once");
    assert_eq!(p.row("api", "in_progress").1.as_deref(), Some("moved"));
    let events = p.error_events();
    assert_eq!(events.len(), 1, "the one failure is the only Event: {events:?}");
    let (plan, text, data) = &events[0];
    assert_eq!((plan.as_deref(), data["code"].as_str(), data["source"].as_str()), (None, Some("jira_auth"), Some("watch")), "the User's account: no Plan");
    assert!(text.contains("pi mcp login atlassian"), "{text}");
}

#[test]
fn a_failing_row_does_not_block_the_rows_behind_it() {
    let t = jira(LONG, true);
    t.probe().says_for(API, FAILED);
    t.queue("api", "in_progress");
    t.queue("web", "in_progress");
    t.run(then_ticks(5, |p| p.settled("web", "in_progress")));
    let p = t.probe();

    assert_eq!(p.tickets(), [API, WEB], "the failing row once, then the row behind it, and no retry within the period");
    assert!(p.pending("api", "in_progress"), "the failing row stays PENDING, never parked");
    assert_eq!(p.row("web", "in_progress").1.as_deref(), Some("moved"));
    assert_eq!(p.codes(), ["jira_failed"], "one Event for the one code");
}

#[test]
fn a_machine_without_the_atlassian_server_reports_jira_not_configured_and_keeps_the_row() {
    let t = jira(REPEAT, false);
    t.queue("api", "in_progress");
    // The error is the signal that the step is alive. Three ticks: no call and no second Event. Then the server is added
    // to the machine policy and the repeat period passes: the row runs.
    let (mut failed_at, mut added) = (None, false);
    let mut tail = then_ticks(3, |p| p.settled("api", "in_progress"));
    let seen = Arc::new(Mutex::new(None));
    let record = seen.clone();
    t.run(move |p, nap| {
        let Some(at) = failed_at else {
            if !p.error_events().is_empty() {
                failed_at = Some(nap);
            }
            return false;
        };
        if !added {
            if nap >= at + 3 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.codes(), p.pending("api", "in_progress")));
                p.host_policy(SERVER);
                p.skip(u64::from(REPEAT) + 1);
                added = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    assert_eq!(seen.lock().unwrap().take(), Some((0, vec!["jira_not_configured".to_owned()], true)), "no helper call, one Event, the row kept");
    let events = p.error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].1.contains("mcp.servers.atlassian"), "{}", events[0].1);
    assert_eq!(p.tickets(), [API], "the server is there: the row runs");
    assert_eq!(p.row("api", "in_progress").1.as_deref(), Some("moved"));
}

#[test]
fn a_pr_policy_that_does_not_resolve_is_reported_and_retried_later() {
    // (what is wrong with the owner's policy, the code of the Event)
    let cases = [("invalid", "policy_invalid"), ("missing", "owner_unconfirmed")];
    for (what, code) in cases {
        let t = jira(REPEAT, true);
        match what {
            "invalid" => t.fx.env.owner_policy("acme", "this is = = not toml\n"),
            _ => fs::remove_file(t.fx.env.cpd_home().join("policy/acme/owner.toml")).unwrap(),
        }
        t.queue("api", "in_progress");
        // The Event is the signal that the step is alive; the policy is fixed three ticks later and the repeat period passes.
        let (mut failed_at, mut fixed) = (None, false);
        let mut tail = then_ticks(3, |p| p.settled("api", "in_progress"));
        let seen = Arc::new(Mutex::new(None));
        let record = seen.clone();
        t.run(move |p, nap| {
            let Some(at) = failed_at else {
                if !p.error_events().is_empty() {
                    failed_at = Some(nap);
                }
                return false;
            };
            if !fixed {
                if nap >= at + 3 * NAPS_PER_TICK {
                    *record.lock().unwrap() = Some((p.calls().len(), p.codes(), p.pending("api", "in_progress")));
                    p.owner_policy(&names());
                    p.skip(u64::from(REPEAT) + 1);
                    fixed = true;
                }
                return false;
            }
            tail(p, nap)
        });
        let p = t.probe();

        assert_eq!(seen.lock().unwrap().take(), Some((0, vec![code.to_owned()], true)), "{what}: no helper call, one Event, the row PENDING");
        let events = p.error_events();
        assert_eq!(events.len(), 1, "{what}: {events:?}");
        assert_eq!(events[0].0.as_deref(), Some("pay"), "{what}: scoped to the Plan");
        assert_eq!(p.tickets(), [API], "{what}: the policy is fixed: the row runs after the repeat period");
        assert_eq!(p.row("api", "in_progress").1.as_deref(), Some("moved"), "{what}");
    }
}

#[test]
fn a_cancelled_prs_row_is_not_run() {
    let t = jira(LONG, true);
    t.pr("old", 3, Some("ABC-5"), "cancelled");
    t.queue("old", "in_progress");
    t.queue("api", "in_progress");
    t.run(then_ticks(5, |p| p.settled("api", "in_progress")));
    let p = t.probe();

    assert_eq!(p.tickets(), [API], "only the control, the live PR's row, runs");
    assert!(p.pending("old", "in_progress"), "the cancelled PR's row is left as it is");
    assert_eq!(p.error_events(), vec![]);
}

#[test]
fn incident_mode_holds_the_jira_step() {
    let t = jira(LONG, true);
    t.queue("api", "in_progress");
    t.fx.env.sql(&format!(
        "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, '{}', 'pay-lead', 'why', cpd_txn());
         INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn());",
        t.fx.stamp_ago(0)
    ));
    // Held for five ticks; then incident mode ends and the step runs.
    let held = Arc::new(Mutex::new(None));
    let record = held.clone();
    let mut over = false;
    let mut tail = then_ticks(3, |p| p.settled("api", "in_progress"));
    t.run(move |p, nap| {
        if !over {
            if nap >= 5 * NAPS_PER_TICK {
                *record.lock().unwrap() = Some((p.calls().len(), p.pending("api", "in_progress"), p.log()));
                p.write("DELETE FROM incident_plans; DELETE FROM incident;");
                over = true;
            }
            return false;
        }
        tail(p, nap)
    });
    let p = t.probe();

    let (calls, pending, log) = held.lock().unwrap().take().expect("five ticks passed");
    assert_eq!((calls, pending), (0, true), "while incident mode is on: no helper call, no row written");
    assert!(log.contains("E19: holding: incident mode is on"), "{log}");
    assert_eq!(p.calls().len(), 1, "off again: it runs");
    assert!(p.settled("api", "in_progress"));
}

#[test]
fn a_failed_run_is_one_error_event_scoped_to_the_plan_naming_the_ticket_the_status_and_jiras_message() {
    let t = jira(LONG, true);
    t.probe().says(FAILED_POINTS);
    t.queue("api", "done");
    let mut tail = then_ticks(3, |p| !p.error_events().is_empty());
    t.run(move |p, nap| tail(p, nap));
    let p = t.probe();

    let events = p.error_events();
    assert_eq!(events.len(), 1, "one Event: {events:?}");
    let (plan, text, data) = &events[0];
    assert_eq!(plan.as_deref(), Some("pay"), "scoped to the Plan: {text}");
    assert_eq!((data["source"].as_str(), data["code"].as_str()), (Some("watch"), Some("jira_failed")));
    assert_eq!(
        data["message"].as_str(),
        Some(r#"ABC-123 could not be moved to "Done": transitionJiraIssue returned an error: Please add story points"#)
    );
    assert_eq!(
        data["details"],
        json!({"tool": "transitionJiraIssue", "status_code": 400, "plan": "pay", "pr": "api", "ticket": API, "target": "done", "status": "Done"})
    );
    assert!(
        text.starts_with(r#"The engine failed at E19 (Jira transitions) for the done move of PR api of Plan pay: ABC-123 could not be moved to "Done""#),
        "{text}"
    );
    assert!(text.ends_with("See: cpd plan show --plan pay"), "{text}");
    assert!(p.pending("api", "done"), "the row stays PENDING");
    let items: i64 = p.store().query_row("SELECT count(*) FROM inbox_items", [], |r| r.get(0)).unwrap();
    assert_eq!(items, 0, "no Inbox item");
}

#[test]
fn two_rows_failing_with_the_same_code_are_two_events_one_each() {
    let t = jira(LONG, true);
    t.probe().says_for(API, FAILED_POINTS);
    t.probe().says_for(WEB, FAILED_POINTS);
    t.queue("api", "done");
    t.queue("web", "done");
    t.run(then_ticks(5, |p| p.tickets().len() >= 2));
    let events = t.probe().error_events();

    assert_eq!(events.len(), 2, "one Event each: {events:?}");
    for (plan, _, data) in &events {
        assert_eq!((plan.as_deref(), data["code"].as_str()), (Some("pay"), Some("jira_failed")));
    }
    let who: Vec<(&str, &str)> = events.iter().map(|(_, _, d)| (d["details"]["ticket"].as_str().unwrap_or(""), d["details"]["pr"].as_str().unwrap_or(""))).collect();
    assert_eq!(who, [(API, "api"), (WEB, "web")]);
}

#[test]
fn a_row_that_keeps_failing_is_one_event_per_code_until_it_settles() {
    let t = jira(REPEAT, true);
    t.probe().says(FAILED_POINTS);
    t.queue("api", "done");
    // Phases: 0 first Event; 1-3 skip a period and wait for one more call; 4 the code changes; 5 the row settles.
    // A call is followed by two ticks, so that the end of its run was seen before the next skip.
    let phase = Arc::new(Mutex::new(0usize));
    let after_three = Arc::new(Mutex::new(None));
    let after_code = Arc::new(Mutex::new(None));
    let (a3, ac) = (after_three.clone(), after_code.clone());
    let (mut base, mut seen_at) = (0usize, None::<usize>);
    let mut tail = then_ticks(3, |p| p.settled("api", "done"));
    t.run(move |p, nap| {
        let mut ph = phase.lock().unwrap();
        let skip = |p: &Probe| p.skip(u64::from(REPEAT) + 1);
        match *ph {
            0 => {
                if !p.error_events().is_empty() {
                    base = p.calls().len();
                    skip(p);
                    *ph = 1;
                }
            }
            1..=4 => {
                if seen_at.is_none() && p.calls().len() > base {
                    seen_at = Some(nap);
                }
                if seen_at.is_some_and(|at| nap >= at + 2 * NAPS_PER_TICK) {
                    seen_at = None;
                    base = p.calls().len();
                    match *ph {
                        3 => {
                            *a3.lock().unwrap() = Some((p.calls().len(), p.error_events()));
                            p.says(HELPER_FAILED);
                        }
                        4 => {
                            *ac.lock().unwrap() = Some(p.error_events());
                            p.says(MOVED);
                        }
                        _ => {}
                    }
                    skip(p);
                    *ph += 1;
                }
            }
            _ => return tail(p, nap),
        }
        false
    });
    let p = t.probe();

    let (calls, events) = after_three.lock().unwrap().take().expect("three more calls");
    assert!(calls >= 4, "{calls} calls");
    assert_eq!(events.len(), 1, "one Event for the one code: {events:?}");
    assert_eq!((events[0].0.as_deref(), events[0].2["code"].as_str()), (Some("pay"), Some("jira_failed")));

    let events = after_code.lock().unwrap().take().expect("a call with another code");
    let codes: Vec<&str> = events.iter().map(|(_, _, d)| d["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["jira_failed", "jira_helper_failed"], "{events:?}");
    assert!(events.iter().all(|(plan, ..)| plan.as_deref() == Some("pay")), "{events:?}");

    assert_eq!(p.error_events().len(), 2, "settling appends no Event");
    assert_eq!(p.row("api", "done").1.as_deref(), Some("moved"));
}

#[test]
fn a_helper_that_prints_no_json_line_is_a_plan_error_event_with_the_spawn_failed_code() {
    let t = jira(LONG, true);
    t.probe().says(NO_JSON);
    t.queue("api", "in_progress");
    t.run(then_ticks(3, |p| !p.error_events().is_empty()));
    let events = t.probe().error_events();

    assert_eq!(events.len(), 1, "{events:?}");
    let (plan, _, data) = &events[0];
    assert_eq!((plan.as_deref(), data["code"].as_str()), (Some("pay"), Some("spawn_failed")));
    let d = &data["details"];
    let keys: Vec<&str> = ["plan", "pr", "ticket", "target", "status"].iter().map(|k| d[*k].as_str().unwrap_or("")).collect();
    assert_eq!(keys, ["pay", "api", API, "in_progress", "In Progress"], "{d}");
    let message = data["message"].as_str().unwrap();
    assert!(message.starts_with(r#"ABC-123 could not be moved to "In Progress": "#), "{message}");
}

#[test]
fn jira_auth_is_one_machine_wide_event_however_many_rows_fail() {
    let t = jira(LONG, true);
    t.probe().says(AUTH);
    t.queue("api", "in_progress");
    t.queue("web", "in_progress");
    t.run(then_ticks(5, |p| p.tickets().len() >= 2));
    let events = t.probe().error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!((events[0].0.as_deref(), events[0].2["code"].as_str()), (None, Some("jira_auth")));
}

#[test]
fn jira_not_configured_is_one_machine_wide_event_however_many_rows_fail() {
    let t = jira(LONG, false);
    t.queue("api", "in_progress");
    t.queue("web", "in_progress");
    t.run(then_ticks(5, |p| !p.error_events().is_empty()));
    let events = t.probe().error_events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!((events[0].0.as_deref(), events[0].2["code"].as_str()), (None, Some("jira_not_configured")));
}
