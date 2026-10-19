//! One whole Plan through the built `cpd`, end to end: `context new`,
//! `plan new`, `plan phase plan`, two Jobs (one needing the other), the Plan
//! review, `plan lock`, the engine in the background dispatching both
//! Attempts, fake Workers that commit and submit, both merges in dependency
//! order, `all-merged`, `pr open` with the Gate and `pr poll` with failing checks.
//!
//! Everything external is faked: `pi` (Leads and Workers), `gh`, pi's Node for
//! the Tier catalog, a bare `origin`, and a private tmux server. Nothing is
//! written into the database by hand: every row comes from a `cpd` command,
//! the engine or a driver, so the bugs between areas show up here.

mod common;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde_json::{Value, json};

const CONTEXT: &str = "ctx";
const PLAN: &str = "pay";
const PR_URL: &str = "https://github.com/acme/pay/pull/7";

/// The fake `pi`, for every role. It records its session file, as pi does at
/// start (§6.5), and reports `idle` as the adapter does at `session_start`
/// (§15.2). A Lead then stays idle. A Worker (it has `CPD_ATTEMPT`) reports
/// `working`, writes `<job>.txt` (the Job `ui` first checks that the Job
/// `api` it needs is in its worktree), commits, runs `attempt submit` and
/// stays until the driver kills its window.
const PI: &str = r#"
sid=""; prev=""
for a in "$@"; do [ "$prev" = "--session-id" ] && sid="$a"; prev="$a"; done
dir="$HOME/.pi/agent/sessions/--e2e--"
mkdir -p "$dir"
f="$dir/2026-10-14T21-00-00-000Z_$sid.jsonl"
msg='{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Done."}],"usage":{"input":10,"output":20,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.5}},"stopReason":"stop"}}'
cpd session state idle > /dev/null || exit 11
if [ -n "$CPD_ATTEMPT" ]; then
  cpd session state working > /dev/null || exit 12
  printf '%s\n' '{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}' "$msg" > "$f"
  until cpd attempt show "$CPD_ATTEMPT" | grep -q '"status":"running"'; do sleep 0.1; done
  job="${CPD_ATTEMPT%-a*}"
  if [ "$job" = ui ] && [ ! -f api.txt ]; then exit 13; fi
  echo "$job" > "$job.txt"
  git add -A && git commit -qm "$CPD_ATTEMPT" || exit 14
  cpd attempt submit "$CPD_ATTEMPT" > "../submit-$CPD_ATTEMPT.json" 2>&1
else
  printf '%s\n' '{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}' "$msg" > "$f"
fi
"#;

/// The fake `gh`: no open PR yet, the PR it creates, then the poller's search finds that PR with failing checks (the
/// engine polls from its first tick, before the PR exists: the search finds nothing then).
const GH: &str = r#"
created="$(dirname "$0")/../pr-created"
case "$1 $2" in
  "pr list") echo '[]' ;;
  "pr create") touch "$created"; echo 'https://github.com/acme/pay/pull/7' ;;
  "api graphql")
    case "$4" in
      *"p0: repository"*)
        # `pr open` reads `createdAt` of the pull request it just created.
        echo '{"data":{"p0":{"pullRequest":{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"OPEN","createdAt":"2026-10-14T21:00:00Z","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[]}}}}}'
        exit 0 ;;
    esac
    nodes=''
    if [ -e "$created" ]; then
      nodes='{"repository":{"nameWithOwner":"acme/pay"},"number":7,"url":"https://github.com/acme/pay/pull/7","title":"Pay","isDraft":false,"state":"OPEN","createdAt":"2026-10-14T21:00:00Z","mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE"}}}]}}'
    fi
    echo '{"data":{"search":{"issueCount":1,"nodes":['"$nodes"']},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}' ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

/// pi's catalog as the embedded module prints it (§9): every level for each model.
const CATALOG: &str = r#"{"O":["off","minimal","low","medium","high","xhigh","max"],"S":["off","minimal","low","medium","high","xhigh","max"],"H":["off","minimal","low","medium","high"]}"#;

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    origin: PathBuf,
    /// The Lead worktree (§2.1).
    lead: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let env = TestEnv::new();
        // `cpd` on PATH is the built binary: panes and the engine run it so (§6.5, E5).
        symlink(CPD, env.bin().join("cpd")).unwrap();
        env.fake_pi(&format!("{PI}exec sleep {SLEEP_SECONDS}"));
        env.fake_gh(GH);
        // A pi install whose agent directory is the test root, and pi's own Node
        // printing its catalog (§9).
        fs::create_dir_all(env.root().join("install")).unwrap();
        fs::write(env.root().join("install/current-version"), "1.2.0\n").unwrap();
        let scope = env.root().join("install/releases/1.2.0/node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            fs::create_dir_all(scope.join(package).join("dist")).unwrap();
            fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
        }
        let node = env.home().join(".local/share/pi-node/current/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, format!("#!/bin/sh\necho '{CATALOG}'\n")).unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();

        fs::write(
            env.cpd_home().join("config.toml"),
            format!(
                "version = 1\nmachine = \"m1\"\n\
                 [tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
                 [timeouts]\nfork_quiet_seconds = 1\n\
                 [watch]\ntick_seconds = 1\n\
                 [tmux]\nsocket = \"{}\"\n",
                env.tmux_socket()
            ),
        )
        .unwrap();

        // The repo, its hooks (§7.3) and a bare `origin`.
        let repo = env.keyed_repo("pay").canonicalize().unwrap();
        let hooks = env.root().join("hooks.log");
        fs::create_dir_all(repo.join(".config")).unwrap();
        fs::write(
            repo.join(".config/wt.toml"),
            format!(
                "pre-start = \"echo pre-start {{{{ branch }}}} >> {log}\"\n\
                 pre-commit = \"echo pre-commit {{{{ branch }}}} >> {log}\"\n\
                 pre-merge = \"test -f api.txt && test -f ui.txt && echo pre-merge {{{{ branch }}}} >> {log}\"\n",
                log = hooks.display()
            ),
        )
        .unwrap();
        env.git(&repo, &["add", "-A"]);
        env.git(&repo, &["commit", "-q", "-m", "hooks"]);
        let origin = env.root().join("origin.git");
        env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
        env.local_origin(&repo, &origin);
        env.git(&repo, &["push", "-q", "-u", "origin", "main"]);
        let lead = repo.parent().unwrap().join(format!("pay.{PLAN}.pay"));
        Fx { env, repo, origin, lead }
    }

    fn ok(&self, args: &[&str]) -> Value {
        self.env.cpd(args).ok()
    }

    /// Every Event after `cursor`, in id order.
    fn events_since(&self, cursor: i64) -> Vec<Value> {
        let out = self.ok(&["events", "list", "--since", &cursor.to_string(), "--limit", "5000"]);
        out["events"].as_array().unwrap().clone()
    }

    fn cursor(&self) -> i64 {
        self.events_since(0).last().map_or(0, |e| e["id"].as_i64().unwrap())
    }

    fn plan(&self) -> Value {
        self.ok(&["plan", "show", "--plan", PLAN])
    }

    fn session(&self, id: &str) -> Value {
        self.ok(&["session", "show", id])["session"].clone()
    }

    fn hooks_log(&self) -> String {
        fs::read_to_string(self.env.root().join("hooks.log")).unwrap_or_default()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    /// What to look at when a wait times out: the engine's log, each driver's
    /// log, the `error` Events and the windows.
    fn diagnostics(&self) -> String {
        let home = self.env.cpd_home();
        let mut out = format!("watch.log:\n{}\n", fs::read_to_string(home.join("run/watch.log")).unwrap_or_default());
        if let Ok(dir) = fs::read_dir(home.join(format!("run/{PLAN}/logs"))) {
            for entry in dir.flatten() {
                out += &format!("{}:\n{}\n", entry.path().display(), fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
        let errors: Vec<Value> = self.events_since(0).into_iter().filter(|e| e["kind"] == "error").collect();
        out += &format!("error Events: {errors:?}\n");
        let windows = self.env.tmux(&["list-panes", "-a", "-F", "#{session_name}:#{window_name} #{pane_dead} #{pane_dead_status}"]);
        out += &format!("panes:\n{}", String::from_utf8_lossy(&windows.stdout));
        out
    }

    /// Waits until `ready` gives a value, at most `secs` seconds.
    fn wait<T>(&self, what: &str, secs: u64, mut ready: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(value) = ready() {
                return value;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}\n{}", self.diagnostics());
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits for the first Event of `kind` after `cursor` matching `filter`.
    fn wait_event(&self, cursor: i64, kind: &str, secs: u64, filter: impl Fn(&Value) -> bool) -> Value {
        self.wait(&format!("an Event {kind}"), secs, || {
            self.events_since(cursor).into_iter().find(|e| e["kind"] == kind && filter(e))
        })
    }

    fn pi_calls(&self) -> Vec<Vec<String>> {
        self.env.calls("pi")
    }

    /// The pid of the engine, `cpd-work:watch` (tmux runs its argv without a shell).
    fn engine_pid(&self) -> i32 {
        let out = self.env.tmux(&["list-panes", "-a", "-F", "#{session_name}:#{window_name} #{pane_pid}"]);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let line = text.lines().find(|l| l.starts_with("cpd-work:watch ")).unwrap_or_else(|| panic!("no engine pane: {text}"));
        line.rsplit(' ').next().unwrap().parse().unwrap()
    }
}

fn kinds(events: &[Value]) -> Vec<&str> {
    events.iter().map(|e| e["kind"].as_str().unwrap()).collect()
}

fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

/// `argv[i + 1]` where `argv[i] == flag`.
fn flag<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter().position(|a| a == flag).map(|i| argv[i + 1].as_str())
}

fn spec_file(fx: &Fx, key: &str) -> String {
    let path = fx.env.root().join(format!("{key}.spec.json"));
    let spec = json!({
        "goal": format!("Write {key}.txt."),
        "context": "The end-to-end test.",
        "touches": [format!("{key}.txt")],
        "acceptance": [format!("{key}.txt exists")],
        "out_of_scope": [],
    });
    fs::write(&path, spec.to_string()).unwrap();
    path.display().to_string()
}

#[test]
fn a_whole_plan_runs_from_context_new_to_pr_poll() {
    let started = Instant::now();
    let fx = Fx::new();
    let repo = fx.repo.to_str().unwrap();

    // ---------- 1. context new
    let out = fx.ok(&["context", "new", CONTEXT, "--title", "Payments", "--owner", "acme"]);
    assert_eq!(out["context"]["id"], CONTEXT);
    let events = fx.events_since(0);
    assert_eq!(kinds(&events), ["context-created"]);
    assert_eq!(events[0]["context"], CONTEXT);
    let mut cursor = fx.cursor();

    // ---------- 2. plan new: a fresh Lead in its parked window
    let out = fx.ok(&["plan", "new", PLAN, "--context", CONTEXT, "--title", "Retry failed payouts"]);
    let lead_session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    assert_eq!(out["plan"]["phase"], "grounding");
    assert_eq!(out["plan"]["prs"], json!([]));
    assert_eq!(out["lead"]["session"], lead_session.as_str());
    assert!(out["lead"]["forked_from"].is_null(), "{out}");
    // The Lead's pi reports `idle` (stopped -> idle, a `session-state` Event) as soon as
    // it starts, concurrently with this command's return. Wait for that, so the Events
    // after `plan new` are a settled set; its order against `session-moved` is not fixed.
    let lead = fx.wait("the Lead's session file and idle state", 30, || {
        let s = fx.session(&lead_session);
        (s["state"] == "idle" && s["file"].is_string()).then_some(s)
    });
    let events = fx.events_since(cursor);
    let mut ks = kinds(&events);
    assert_eq!(ks[..3], ["plan-created", "session-registered", "context-record"]);
    ks[3..].sort_unstable();
    assert_eq!(ks[3..], ["session-moved", "session-state"]);
    assert_eq!(events[0]["data"]["lead_session"], lead_session.as_str());
    assert_eq!(events[1]["session"], lead_session.as_str());
    assert_eq!(events[2]["data"]["record"], out["record"]);
    let moved = of_kind(&events, "session-moved");
    assert_eq!(moved[0]["data"]["place"], "parked");
    assert_eq!(moved[0]["data"]["pane"], out["lead"]["pane"]);
    let state = of_kind(&events, "session-state");
    assert_eq!(state[0]["session"], lead_session.as_str());
    assert_eq!(state[0]["data"]["from"], "stopped");
    assert_eq!(state[0]["data"]["to"], "idle");
    assert_eq!(events[0]["data"]["cwd"], fx.env.root().canonicalize().unwrap().display().to_string());
    let message = fs::read_to_string(fx.env.cpd_home().join("run/pay/lead-message.md")).unwrap();
    assert!(message.starts_with("You are the Lead of Plan pay \"Retry failed payouts\" in Context ctx."), "{message}");
    // The Lead's `session run` started pi fresh with that message (§6.5), and
    // recorded the session file pi wrote.
    assert_eq!(lead["place"], "parked");
    assert_eq!(lead["plan_lead"], true);
    let calls = fx.pi_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(flag(&calls[0], "--session-id"), Some(lead_session.as_str()));
    assert_eq!(flag(&calls[0], "--model"), Some("p/opus:high"));
    assert_eq!(flag(&calls[0], "-n"), Some(PLAN));
    let message_file = fx.env.cpd_home().join("run/pay/lead-message.md");
    assert_eq!(calls[0][calls[0].len() - 2..], ["--".to_owned(), format!("@{}", message_file.display())]);
    assert!(!calls[0].contains(&"--fork".to_owned()), "{calls:?}");
    cursor = fx.cursor();
    let out = fx.ok(&["pr", "add", "pay", "--repo", repo, "--plan", PLAN]);
    assert_eq!(out["pr"]["base"], "main");
    assert_eq!(out["pr"]["lead_branch"], "cpd/pay/pay");
    assert_eq!(kinds(&fx.events_since(cursor)), ["pr-added"]);
    cursor = fx.cursor();

    // ---------- 3. plan phase plan: the phase and its Event, no Session and no window
    let out = fx.ok(&["plan", "phase", "plan", "--plan", PLAN]);
    assert_eq!(out["phase"], "plan");
    assert!(out.get("grounded_lead").is_none(), "{out}");
    let events = fx.events_since(cursor);
    assert_eq!(kinds(&events), ["plan-phase"]);
    assert_eq!(events[0]["data"], json!({"from": "grounding", "to": "plan"}));
    cursor = fx.cursor();

    // ---------- 4. two Jobs, `ui` needing `api`; the Plan review, approved
    let out = fx.ok(&["job", "add", "api", "--title", "The API", "--tier", "S2", "--spec", &spec_file(&fx, "api"), "--plan", PLAN]);
    assert_eq!(out["job"], json!({"key": "api", "pr": "pay", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": false}));
    let out = fx.ok(&[
        "job", "add", "ui", "--title", "The UI", "--tier", "H1", "--spec", &spec_file(&fx, "ui"), "--needs", "api", "--plan", PLAN,
    ]);
    assert_eq!(out["job"], json!({"key": "ui", "pr": "pay", "status": "draft", "appended": false, "tier": "H1", "needs": ["api"], "review": false, "urgent": false}));
    let review = fx.ok(&["plan", "review", "--plan", PLAN]);
    assert_eq!(review["review"]["id"], "r1");
    let decided = fx.ok(&["plan", "decide", "r1", "--approve", "--plan", PLAN]);
    assert_eq!(decided["current"], true);
    let events = fx.events_since(cursor);
    assert_eq!(kinds(&events), ["job-added", "job-added", "plan-review", "plan-review"]);
    assert_eq!(events[1]["job"], "ui");
    assert_eq!(events[1]["data"], json!({"pr": "pay", "status": "draft", "appended": false, "tier": "H1", "needs": ["api"], "review": false, "urgent": false}));
    assert_eq!(events[2]["data"]["status"], "pending");
    assert_eq!(events[3]["data"]["status"], "approved");
    cursor = fx.cursor();

    // ---------- 5. plan lock: the Lead worktree, Jobs queued, the engine started
    let out = fx.ok(&["plan", "lock", "--plan", PLAN]);
    assert_eq!(out["phase"], "dispatch");
    assert_eq!(out["prs"][0]["lead_worktree"], fx.lead.to_str().unwrap());
    assert_eq!(out["jobs"], json!(["api", "ui"]));
    assert_eq!(out["watch"]["started"], true, "{out}");
    let base_sha = fx.git(&fx.repo, &["rev-parse", "main"]);
    assert_eq!(out["prs"][0]["base_sha"], base_sha.as_str());
    assert!(fx.hooks_log().contains("pre-start cpd/pay/pay\n"), "{}", fx.hooks_log());
    let events = fx.events_since(cursor);
    assert_eq!(&kinds(&events)[..1], ["plan-phase"]);
    assert_eq!(events[0]["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["api", "ui"], "prs": ["pay"]}));
    assert!(kinds(&events).contains(&"watch-started"), "{events:?}");

    // ---------- 6. the engine runs in the background, once per machine
    let again = fx.ok(&["watch", "--ensure"]);
    assert_eq!(again, json!({"ok": true, "started": false, "already": true}));
    let engine = fx.engine_pid();

    // ---------- 7, 8. each Attempt's Worker commits and submits; the merges follow the Dependency
    let merged = |key: &'static str| {
        fx.wait_event(cursor, "job-merged", 90, |e| e["job"] == key)
    };
    let api_merged = merged("api");
    let ui_merged = merged("ui");
    let events = fx.events_since(cursor);
    let started_ui = of_kind(&events, "attempt-started").into_iter().find(|e| e["job"] == "ui").unwrap().clone();
    assert!(api_merged["id"].as_i64() < started_ui["id"].as_i64(), "ui started before api was merged: {events:?}");
    for (key, attempt, tier) in [("api", "api-a1", "S2"), ("ui", "ui-a1", "H1")] {
        let about: Vec<Value> = events.iter().filter(|e| e["attempt"] == attempt).cloned().collect();
        assert_eq!(
            kinds(&about),
            ["attempt-started", "attempt-status", "attempt-status", "job-merged"],
            "{key}: {about:?}"
        );
        assert_eq!(about[0]["data"], json!({"number": 1, "tier": tier, "origin": "ladder"}));
        assert_eq!(about[1]["data"], json!({"from": "starting", "to": "running"}));
        assert_eq!(about[2]["data"], json!({"from": "running", "to": "merging"}));
        let submit: Value =
            serde_json::from_str(&fs::read_to_string(fx.repo.parent().unwrap().join(format!("submit-{attempt}.json"))).unwrap()).unwrap();
        assert_eq!(submit["submitted"], true, "{submit}");
        let shown = fx.ok(&["attempt", "show", attempt, "--plan", PLAN])["attempt"].clone();
        assert_eq!((&shown["status"], &shown["outcome"], &shown["cause"]), (&json!("ended"), &json!("merged"), &json!("merged")));
        assert_eq!(shown["merged_sha"], about[3]["data"]["sha"]);
        assert_eq!(about[3]["data"]["tier"], tier);
        // The Worker's Session: registered, started in the work session.
        let worker = shown["worker_session"].as_str().unwrap();
        let mine: Vec<&Value> = events.iter().filter(|e| e["session"] == worker).collect();
        assert_eq!(mine.iter().filter(|e| e["kind"] == "session-registered").count(), 1, "{mine:?}");
        let moved = mine.iter().find(|e| e["kind"] == "session-moved").unwrap();
        assert_eq!(moved["data"]["place"], "work");
        // A9: the worktree and branch of a Merged Attempt are removed.
        // Cleanup runs after the merge transaction (which appends job-merged), so wait for it.
        let worktree = fx.repo.parent().unwrap().join(format!("pay.pay--{attempt}"));
        fx.wait(&format!("A9 to remove {attempt}'s worktree and branch"), 30, || {
            let branch = fx.git(&fx.repo, &["branch", "--list", &format!("cpd/pay--{attempt}")]);
            (!worktree.exists() && branch.is_empty()).then_some(())
        });
    }
    assert!(fx.hooks_log().contains("pre-commit cpd/pay--api-a1\n"), "{}", fx.hooks_log());
    assert!(fx.hooks_log().contains("pre-commit cpd/pay--ui-a1\n"), "{}", fx.hooks_log());
    // The ui Attempt was cut from the Lead Branch after api's merge.
    let ui = fx.ok(&["attempt", "show", "ui-a1", "--plan", PLAN])["attempt"].clone();
    assert_eq!(ui["base_sha"], api_merged["data"]["sha"]);
    // The Lead Branch holds both Jobs' commits, in order, and the Lead worktree has them.
    let lead_sha = fx.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay/pay"]);
    assert_eq!(lead_sha, ui_merged["data"]["sha"].as_str().unwrap());
    assert_eq!(fx.git(&fx.repo, &["log", "--format=%s", "main..cpd/pay/pay"]), "ui-a1\napi-a1");
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fs::read_to_string(fx.lead.join("ui.txt")).unwrap(), "ui\n");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");

    // ---------- 9. all-merged, once
    let all = fx.wait_event(cursor, "all-merged", 30, |_| true);
    assert_eq!(all["data"], json!({"pr": "pay", "merged": ["api", "ui"], "cancelled": []}));
    let shown = fx.plan();
    assert_eq!(shown["plan"]["phase"], "dispatch");
    assert!(shown["prs"][0]["all_merged_at"].is_string(), "{shown}");
    assert_eq!(shown["counts"], json!({"draft": 0, "queued": 0, "running": 0, "merged": 2, "failed": 0, "cancelled": 0}));
    assert_eq!(shown["tamper"], json!([]));

    // The engine stops on SIGTERM, so the `pr poll` below is the only one that finds the PR. First,
    // E2 records that each Worker's pane is gone after A9 killed its window
    // (Escalation e2e-a1-q1): `stopped`, no pane, place `none`.
    for attempt in ["api-a1", "ui-a1"] {
        let worker = fx.ok(&["attempt", "show", attempt, "--plan", PLAN])["attempt"]["worker_session"].as_str().unwrap().to_owned();
        let moved = fx.wait_event(cursor, "session-moved", 30, |e| e["session"] == worker.as_str() && e["data"]["place"] == "none");
        assert_eq!(moved["data"], json!({"pane": null, "window": null, "place": "none"}));
        let session = fx.session(&worker);
        assert_eq!((&session["state"], &session["tmux_pane"], &session["place"]), (&json!("stopped"), &Value::Null, &json!("none")));
    }
    let session = fx.session(&lead_session);
    assert_eq!((&session["state"], &session["place"]), (&json!("idle"), &json!("parked")), "{session}");
    kill(Pid::from_raw(engine), Signal::SIGTERM).unwrap();
    fx.wait_event(cursor, "watch-stopped", 30, |e| e["data"]["pid"] == engine);
    let events = fx.events_since(cursor);
    assert_eq!(of_kind(&events, "all-merged").len(), 1, "{events:?}");
    cursor = fx.cursor();

    // ---------- 10. pr open: the Gate on the Lead Branch, the push, gh
    let out = fx.ok(&["pr", "open", "--plan", PLAN]);
    assert_eq!(out["phase"], "deploy");
    assert_eq!(out["pr"]["key"], "pay");
    assert_eq!(out["pr"]["pr"]["number"], 7);
    assert_eq!(out["pr"]["pr"]["url"], PR_URL);
    assert!(fx.hooks_log().ends_with("pre-merge cpd/pay/pay\n"), "{}", fx.hooks_log());
    let pushed = fx.git(&fx.origin, &["rev-parse", "refs/heads/cpd/pay/pay"]);
    assert_eq!(pushed, lead_sha);
    // The engine's polls (`api graphql`) aside, `pr open` made these two calls.
    let gh: Vec<_> = fx.env.calls("gh").into_iter().filter(|c| c[0] != "api").collect();
    assert_eq!(gh[0], ["pr", "list", "--head", "cpd/pay/pay", "--state", "all", "--json", "number,url,state"]);
    assert_eq!(gh[1][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay/pay", "--title", "Retry failed payouts"]);
    let body = &gh[1][9];
    for sha in [&api_merged["data"]["sha"], &ui_merged["data"]["sha"]] {
        assert!(body.contains(sha.as_str().unwrap()), "{body}");
    }
    let events = fx.events_since(cursor);
    assert_eq!(kinds(&events), ["pr-opened", "plan-phase"]);
    assert_eq!(events[0]["data"], json!({"pr": "pay", "number": 7, "url": PR_URL}));
    assert_eq!(events[1]["data"], json!({"from": "dispatch", "to": "deploy", "prs": [{"key": "pay", "number": 7, "url": PR_URL}]}));
    cursor = fx.cursor();

    // ---------- 11. pr poll reports the failing checks, once (D7)
    let out = fx.ok(&["pr", "poll", "--plan", PLAN]);
    assert_eq!(out["feedback"], json!([{"plan": PLAN, "pr": "pay"}]));
    assert_eq!(out["prs"][0]["pull_request"]["checks"], "failed");
    let events = fx.events_since(cursor);
    assert_eq!(kinds(&events), ["pr-feedback"]);
    assert_eq!(
        events[0]["data"],
        json!({"pr": "pay", "number": 7, "url": PR_URL, "state": "open", "checks": "failed", "review_decision": "none", "mergeable": "unknown", "merge_state": "unknown", "conflict": false, "reasons": ["checks_failed"]})
    );
    let again = fx.ok(&["pr", "poll", "--plan", PLAN]);
    assert_eq!((again["feedback"].clone(), again["changed"].clone()), (json!([]), json!([])));
    assert_eq!(fx.events_since(cursor).len(), 1);

    // ---------- the whole run: no `error` Event, nothing tampered, nothing left running
    let all = fx.events_since(0);
    assert!(of_kind(&all, "error").is_empty(), "{:?}", of_kind(&all, "error"));
    let shown = fx.plan();
    assert_eq!(shown["plan"]["phase"], "deploy");
    assert_eq!(shown["tamper"], json!([]));
    assert_eq!(fx.ok(&["attempt", "list", "--active", "--plan", PLAN])["attempts"], json!([]));
    assert!(started.elapsed() < Duration::from_secs(120), "took {:?}", started.elapsed());
}
