//! A Plan across two repos through the built `cpd`, end to end (R34, A4–A14;
//! B2): PRs `a1` (repo `shared`) -> `b` (repo `web`) -> `a2` (repo `shared`
//! again). From `plan new` (run outside any repo) and `pr add`, through
//! `plan lock`, the engine, fake Workers, each PR's `pr open`, a fake GitHub
//! that merges the pull requests one after the other, and engine E7 starting
//! each waiting PR from a fresh base, to `plan-done`, once. `a2` also holds a
//! review Job, added after its other Job, whose Worker is a fork of the Lead.
//!
//! Everything external is faked as in `e2e.rs`: `pi`, `gh`, pi's Node, bare
//! `origin`s and a private tmux server. `common/` is not this Job's to extend,
//! so the pi install and the fake Worker are restated here. Nothing is written
//! into the database by hand: every row comes from a `cpd` command, the engine
//! or a driver.

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv, write_executable};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde_json::{Value, json};

const CONTEXT: &str = "ctx";
const PLAN: &str = "chain";

/// The fake `pi`, for every role: it records its session file and reports
/// `idle`, as in `e2e.rs`. A Lead then stays idle. A Worker (it has
/// `CPD_ATTEMPT`) waits for its Attempt to be `running`, then:
/// - the Job `wire` first checks that `schema.txt`, merged by PR `a1`, is in its
///   worktree: PR `a2` was started from a base that holds it;
/// - the review Job `audit` commits nothing: it writes its findings, the files
///   its worktree differs in from `origin/main`, to `run/<plan>/reviews/audit.md`;
/// - every other Job writes `<job>.txt` and commits it.
///
/// Then it runs `attempt submit` and stays until the driver kills its window.
const PI: &str = r##"
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
  if [ "$job" = wire ] && [ ! -f schema.txt ]; then exit 13; fi
  if [ "$job" = audit ]; then
    mkdir -p "$CPD_HOME/run/$CPD_PLAN/reviews"
    { echo "# Findings"; git diff --name-only origin/main HEAD; } > "$CPD_HOME/run/$CPD_PLAN/reviews/audit.md"
  else
    echo "$job" > "$job.txt"
    git add -A && git commit -qm "$CPD_ATTEMPT" || exit 14
  fi
  cpd attempt submit "$CPD_ATTEMPT" > "../submit-$CPD_ATTEMPT.part" 2>&1; mv "../submit-$CPD_ATTEMPT.part" "../submit-$CPD_ATTEMPT.json"
else
  printf '%s\n' '{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}' "$msg" > "$f"
fi
"##;

/// pi's catalog as the embedded module prints it (§9).
const CATALOG: &str = r#"{"O":["off","minimal","low","medium","high","xhigh","max"],"S":["off","minimal","low","medium","high","xhigh","max"],"H":["off","minimal","low","medium","high"]}"#;

/// What gh knows of each pull request: `a1` is #11, `b` #12 and `a2` #13 (`pr create` finds the number by the Lead
/// worktree it is run in). A pull request exists once `pr create` made it (`created-<number>` in `$FLAGS`) and is
/// `OPEN` until the test creates `merged-<number>` there: GitHub merging it. The poller's search lists the ones
/// that exist and are not merged; its lookup answers the ones it names.
fn gh(flags: &Path) -> String {
    format!(
        r#"FLAGS='{flags}'
node() {{
  state="$2"; merged=null
  if [ "$state" = MERGED ]; then merged='"2026-10-15T10:00:00Z"'; fi
  printf '%s' '{{"repository":{{"nameWithOwner":"acme/repo"}},"number":'"$1"',"url":"https://github.com/acme/repo/pull/'"$1"'","title":"Chain","isDraft":false,"state":"'"$state"'","createdAt":"2026-10-15T09:00:00Z","mergedAt":'"$merged"',"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{{"nodes":[]}}}}'
}}
case "$1 $2" in
  "pr list") echo '[]' ;;
  "pr create")
    case "$PWD" in
      *.{PLAN}.a1) n=11 ;;
      *.{PLAN}.b) n=12 ;;
      *.{PLAN}.a2) n=13 ;;
      *) echo "gh pr create outside a Lead worktree: $PWD" >&2; exit 8 ;;
    esac
    touch "$FLAGS/created-$n"
    echo "https://github.com/acme/repo/pull/$n" ;;
  "api graphql")
    case "$4" in
      *"search(type: ISSUE"*)
        nodes=''
        for n in 11 12 13; do
          if [ -f "$FLAGS/created-$n" ] && [ ! -f "$FLAGS/merged-$n" ]; then nodes="$nodes${{nodes:+,}}$(node $n OPEN)"; fi
        done
        echo '{{"data":{{"search":{{"issueCount":0,"nodes":['"$nodes"']}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}' ;;
      *"p0: repository"*)
        data=''; i=0
        for n in $(printf '%s' "$4" | grep -o 'pullRequest(number: [0-9]*' | grep -o '[0-9]*$'); do
          if [ -f "$FLAGS/merged-$n" ]; then state=MERGED; else state=OPEN; fi
          data="$data${{data:+,}}\"p$i\":{{\"pullRequest\":$(node $n $state)}}"
          i=$((i+1))
        done
        echo "{{\"data\":{{$data}}}}" ;;
      *) echo "unexpected graphql query: $4" >&2; exit 9 ;;
    esac ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#,
        flags = flags.display()
    )
}

/// A repo with its bare `origin`.
struct Repo {
    path: PathBuf,
    origin: PathBuf,
}

struct Fx {
    env: TestEnv,
    shared: Repo,
    web: Repo,
    /// Where the fake gh finds which pull requests GitHub has merged.
    flags: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let env = TestEnv::new();
        env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
        // `cpd` on PATH is the built binary: panes and the engine run it so (§6.5, E5).
        symlink(CPD, env.bin().join("cpd")).unwrap();
        let flags = env.root().join("github");
        fs::create_dir_all(&flags).unwrap();
        env.fake_pi(&format!("{PI}exec sleep {SLEEP_SECONDS}"));
        env.fake_gh(&gh(&flags));
        fs::create_dir_all(env.root().join("install")).unwrap();
        fs::write(env.root().join("install/current-version"), "1.2.0\n").unwrap();
        let scope = env.root().join("install/releases/1.2.0/node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            fs::create_dir_all(scope.join(package).join("dist")).unwrap();
            fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
        }
        let node = env.home().join(".local/share/pi-node/current/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        write_executable(&node, &format!("#!/bin/sh\necho '{CATALOG}'\n"));

        fs::write(
            env.cpd_home().join("config.toml"),
            format!(
                "version = 1\nmachine = \"m1\"\n\
                 [tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
                 [timeouts]\nfork_quiet_seconds = 1\n\
                 [watch]\ntick_seconds = 0.2\npr_poll_seconds = 1\n\
                 [tmux]\nsocket = \"{}\"\n",
                env.tmux_socket()
            ),
        )
        .unwrap();

        let shared = repo(&env, "shared");
        let web = repo(&env, "web");
        Fx { env, shared, web, flags }
    }

    fn ok(&self, args: &[&str]) -> Value {
        self.env.cpd(args).ok()
    }

    fn events(&self) -> Vec<Value> {
        self.ok(&["events", "list", "--since", "0", "--limit", "5000"])["events"].as_array().unwrap().clone()
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

    /// The first Event of `kind` matching `filter`.
    fn wait_event(&self, kind: &str, filter: impl Fn(&Value) -> bool) -> Value {
        self.wait(&format!("an Event {kind}"), 90, || self.events().into_iter().find(|e| e["kind"] == kind && filter(e)))
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
        let errors: Vec<Value> = self.events().into_iter().filter(|e| e["kind"] == "error").collect();
        out += &format!("error Events: {errors:?}\n");
        let windows = self.env.tmux(&["list-panes", "-a", "-F", "#{session_name}:#{window_name} #{pane_dead} #{pane_dead_status}"]);
        out += &format!("panes:\n{}", String::from_utf8_lossy(&windows.stdout));
        out
    }

    fn prs(&self) -> Vec<Value> {
        self.ok(&["plan", "show", "--plan", PLAN])["prs"].as_array().unwrap().clone()
    }

    fn pr(&self, key: &str) -> Value {
        self.prs().into_iter().find(|p| p["key"] == key).unwrap_or_else(|| panic!("no PR {key}"))
    }

    fn pr_status(&self, key: &str) -> String {
        self.pr(key)["status"].as_str().unwrap().to_owned()
    }

    /// Where PR `key`'s Lead worktree goes in `repo` (A5).
    fn lead(&self, repo: &Repo, key: &str) -> PathBuf {
        let name = repo.path.file_name().unwrap().to_str().unwrap();
        repo.path.parent().unwrap().join(format!("{name}.{PLAN}.{key}"))
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn spec_file(&self, key: &str, touches: &str) -> String {
        let path = self.env.root().join(format!("{key}.spec.json"));
        let spec = json!({
            "goal": format!("Do {key}."),
            "context": "The cross-repo end-to-end test.",
            "touches": [touches],
            "acceptance": [format!("{key} is done")],
            "out_of_scope": [],
        });
        fs::write(&path, spec.to_string()).unwrap();
        path.display().to_string()
    }

    /// `job add <key> --pr <pr>` (plus `extra`) with the Job's Spec; the run, not yet checked.
    fn add_job(&self, key: &str, pr: &str, extra: &[&str]) -> common::Run {
        let spec = self.spec_file(key, &format!("{key}.txt"));
        let title = format!("Job {key}");
        let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", PLAN];
        if !pr.is_empty() {
            args.extend(["--pr", pr]);
        }
        args.extend(extra);
        self.env.cpd(&args)
    }

    /// GitHub merges the pull request `number` of PR branch `branch` of `repo`:
    /// the branch becomes the remote `main` (a fast-forward), and gh says `MERGED`.
    fn github_merges(&self, repo: &Repo, branch: &str, number: u32) {
        self.git(&repo.path, &["push", "-q", "origin", &format!("{branch}:refs/heads/main")]);
        fs::write(self.flags.join(format!("merged-{number}")), "").unwrap();
    }

    /// The pid of the engine, `cpd-work:watch` (tmux runs its argv without a shell).
    fn engine_pid(&self) -> i32 {
        let out = self.env.tmux(&["list-panes", "-a", "-F", "#{session_name}:#{window_name} #{pane_pid}"]);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let line = text.lines().find(|l| l.starts_with("cpd-work:watch ")).unwrap_or_else(|| panic!("no engine pane: {text}"));
        line.rsplit(' ').next().unwrap().parse().unwrap()
    }
}

/// A repo `name` on `main` with hooks that log the branch they ran for (§7.3)
/// and a bare `origin` holding `main`.
fn repo(env: &TestEnv, name: &str) -> Repo {
    let path = env.keyed_repo(name).canonicalize().unwrap();
    fs::create_dir_all(path.join(".config")).unwrap();
    let log = env.root().join(format!("{name}.hooks.log"));
    fs::write(
        path.join(".config/wt.toml"),
        format!(
            "pre-start = \"echo pre-start {{{{ branch }}}} >> {log}\"\n\
             pre-commit = \"echo pre-commit {{{{ branch }}}} >> {log}\"\n\
             pre-merge = \"echo pre-merge {{{{ branch }}}} >> {log}\"\n",
            log = log.display()
        ),
    )
    .unwrap();
    env.git(&path, &["add", "-A"]);
    env.git(&path, &["commit", "-q", "-m", "hooks"]);
    let origin = env.root().join(format!("{name}.git"));
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.local_origin(&path, &origin);
    env.git(&path, &["push", "-q", "-u", "origin", "main"]);
    Repo { path, origin }
}

fn hooks(fx: &Fx, name: &str) -> String {
    fs::read_to_string(fx.env.root().join(format!("{name}.hooks.log"))).unwrap_or_default()
}

fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

fn id(event: &Value) -> i64 {
    event["id"].as_i64().unwrap()
}

/// What a Worker's `attempt submit` printed, saved by the fake pi at `path` whole (written, then renamed into place):
/// it may land after the merge the test waited for, since the driver needs only the submission in the store.
fn submitted(path: &Path) -> Value {
    let text = common::wait_some(&format!("{} to be written", path.display()), common::WAIT, || fs::read_to_string(path).map_err(|e| e.to_string()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}: {text}", path.display()))
}

/// `argv[i + 1]` where `argv[i] == flag`.
fn flag<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter().position(|a| a == flag).map(|i| argv[i + 1].as_str())
}

#[test]
fn a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job() {
    let fx = Fx::new();
    let (shared, web) = (fx.shared.path.to_str().unwrap().to_owned(), fx.web.path.to_str().unwrap().to_owned());

    // ---------- 1. plan new from a directory that is no repo (A12)
    fx.ok(&["context", "new", CONTEXT, "--title", "Chain", "--owner", "acme"]);
    assert!(!fx.env.root().join(".git").exists());
    let out = fx.ok(&["plan", "new", PLAN, "--context", CONTEXT, "--title", "Change A, then B, then A again"]);
    let lead_session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    assert_eq!(out["plan"]["prs"], json!([]));
    let created = fx.events().into_iter().find(|e| e["kind"] == "plan-created").unwrap();
    assert_eq!(created["data"]["cwd"], fx.env.root().canonicalize().unwrap().display().to_string());
    let lead = fx.wait("the Lead's session file and idle state", 30, || {
        let s = fx.ok(&["session", "show", &lead_session])["session"].clone();
        (s["state"] == "idle" && s["file"].is_string()).then_some(s)
    });

    // ---------- 2. the PRs: a1 -> b -> a2, two of them in one repo (A4)
    let out = fx.ok(&["pr", "add", "a1", "--repo", &shared, "--plan", PLAN]);
    assert_eq!(out["pr"], json!({"key": "a1", "repo": shared, "name": "shared", "base": "main", "needs": [], "status": "waiting", "lead_branch": "cpd/chain/a1", "appended": false}));
    let out = fx.ok(&["pr", "add", "b", "--repo", &web, "--needs", "a1", "--plan", PLAN]);
    assert_eq!((&out["pr"]["lead_branch"], &out["pr"]["needs"]), (&json!("cpd/chain/b"), &json!(["a1"])));
    let out = fx.ok(&["pr", "add", "a2", "--repo", &shared, "--needs", "b", "--plan", PLAN]);
    assert_eq!((&out["pr"]["lead_branch"], &out["pr"]["needs"]), (&json!("cpd/chain/a2"), &json!(["b"])));
    assert_eq!(of_kind(&fx.events(), "pr-added").len(), 3);
    fx.ok(&["plan", "phase", "plan", "--plan", PLAN]);

    // ---------- 3. a Job in each PR, and a review Job in a2, added after its other Job (B2)
    let refused = fx.add_job("schema", "", &[]);
    assert_eq!(refused.error()["code"], "pr_required", "{refused:?}");
    assert_eq!(refused.exit, 2);
    let out = fx.add_job("schema", "a1", &[]).ok();
    assert_eq!((&out["job"]["pr"], &out["job"]["review"]), (&json!("a1"), &json!(false)));
    fx.add_job("client", "b", &[]).ok();
    let refused = fx.add_job("other", "b", &["--needs", "schema"]);
    assert_eq!(refused.error()["code"], "needs_other_pr", "Dependencies stay inside one PR (A7): {refused:?}");
    assert_eq!(refused.exit, 3);
    fx.add_job("wire", "a2", &[]).ok();
    let spec = fx.spec_file("audit", &format!("run/{PLAN}/reviews/audit.md"));
    let out = fx.ok(&[
        "job", "add", "audit", "--title", "Review a2", "--tier", "S2", "--spec", &spec, "--pr", "a2", "--review", "--plan", PLAN,
    ]);
    assert_eq!(
        out["job"],
        json!({"key": "audit", "pr": "a2", "status": "draft", "appended": false, "tier": "S2", "needs": ["wire"], "review": true, "urgent": false})
    );

    // ---------- 4. review and lock: only a1 starts (A8)
    let review = fx.ok(&["plan", "review", "--plan", PLAN]);
    fx.ok(&["plan", "decide", review["review"]["id"].as_str().unwrap(), "--approve", "--plan", PLAN]);
    let out = fx.ok(&["plan", "lock", "--plan", PLAN]);
    assert_eq!(out["phase"], "dispatch");
    let by_status = |status: &str| -> Vec<&str> {
        out["prs"].as_array().unwrap().iter().filter(|p| p["status"] == status).map(|p| p["key"].as_str().unwrap()).collect()
    };
    assert_eq!((by_status("dispatch"), by_status("waiting")), (vec!["a1"], vec!["a2", "b"]), "{out}");
    assert_eq!(out["watch"]["started"], true, "{out}");
    let a1_lead = fx.lead(&fx.shared, "a1");
    assert_eq!(out["prs"][0]["lead_worktree"], a1_lead.to_str().unwrap());
    assert_eq!((&out["prs"][1]["lead_worktree"], &out["prs"][2]["lead_worktree"]), (&Value::Null, &Value::Null));
    assert_eq!(
        (fx.pr_status("a1"), fx.pr_status("b"), fx.pr_status("a2")),
        ("dispatch".to_owned(), "waiting".to_owned(), "waiting".to_owned())
    );
    assert!(a1_lead.is_dir());
    assert!(!fx.lead(&fx.web, "b").exists() && !fx.lead(&fx.shared, "a2").exists());
    // (a1's first Attempt may have started already: only a1's own `pre-start` ran at the lock.)
    assert!(hooks(&fx, "shared").starts_with("pre-start cpd/chain/a1\n"), "{}", hooks(&fx, "shared"));
    assert!(!hooks(&fx, "shared").contains("cpd/chain/a2"));
    assert_eq!(hooks(&fx, "web"), "");
    // R35 per PR branch: a1's Lead Branch is on its remote at once; the others are nowhere.
    // Its Job may have merged and pushed meanwhile, so the remote tip holds `main`, not equals it.
    let base_a = fx.git(&fx.shared.path, &["rev-parse", "main"]);
    fx.git(&fx.shared.origin, &["merge-base", "--is-ancestor", &base_a, "refs/heads/cpd/chain/a1"]);
    assert_eq!(fx.git(&fx.shared.path, &["branch", "--list", "cpd/chain/*"]), "+ cpd/chain/a1");
    assert_eq!(fx.git(&fx.web.path, &["branch", "--list", "cpd/chain/*"]), "");
    let events = fx.events();
    let locked = of_kind(&events, "plan-phase").pop().unwrap();
    assert_eq!(locked["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["schema", "client", "wire", "audit"], "prs": ["a1"]}));
    let pr_started = of_kind(&events, "pr-started");
    assert_eq!(pr_started.len(), 1, "{pr_started:?}");
    assert_eq!((&pr_started[0]["data"]["pr"], &pr_started[0]["data"]["by"]), (&json!("a1"), &json!("lock")));
    assert_eq!((&pr_started[0]["data"]["base_sha"], &pr_started[0]["data"]["pushed"]), (&json!(base_a), &json!(true)), "{pr_started:?}");
    let engine = fx.engine_pid();

    // ---------- 5. a1's Job merges and is pushed; all-merged names the PR (A2, A10)
    let schema = fx.wait_event("job-merged", |e| e["job"] == "schema");
    let a1_tip = schema["data"]["sha"].as_str().unwrap().to_owned();
    assert_eq!(fx.git(&fx.shared.path, &["rev-parse", "refs/heads/cpd/chain/a1"]), a1_tip);
    assert_eq!(fx.git(&fx.shared.origin, &["rev-parse", "refs/heads/cpd/chain/a1"]), a1_tip, "the merge pushes the Lead Branch");
    assert_eq!(fs::read_to_string(a1_lead.join("schema.txt")).unwrap(), "schema\n");
    let all = fx.wait_event("all-merged", |e| e["data"]["pr"] == "a1");
    assert_eq!(all["data"], json!({"pr": "a1", "merged": ["schema"], "cancelled": []}));
    assert_eq!(fx.pr("a1")["status"], "dispatch");

    // ---------- 6. pr open --pr a1: the Plan stays in `dispatch`, b and a2 wait
    let out = fx.ok(&["pr", "open", "--pr", "a1", "--plan", PLAN]);
    assert_eq!((&out["phase"], &out["pr"]["key"], &out["pr"]["pr"]["number"]), (&json!("dispatch"), &json!("a1"), &json!(11)));
    assert!(hooks(&fx, "shared").ends_with("pre-merge cpd/chain/a1\n"), "{}", hooks(&fx, "shared"));
    let calls = fx.env.calls("gh");
    let create = calls.iter().find(|c| c[..2] == ["pr", "create"]).unwrap();
    assert_eq!(create[..6], ["pr", "create", "--base", "main", "--head", "cpd/chain/a1"]);
    // E5 polls whatever the phase, and a1 is not merged: nothing starts. Two whole ticks after the PR was created.
    fx.env.wait_whole_ticks(2);
    assert_eq!((fx.pr_status("b"), fx.pr_status("a2")), ("waiting".to_owned(), "waiting".to_owned()));
    assert!(of_kind(&fx.events(), "attempt-started").iter().all(|e| e["job"] == "schema"));

    // ---------- 7. GitHub merges a1: E7 starts b, from a fresh fetch of its base
    fx.github_merges(&fx.shared, "cpd/chain/a1", 11);
    let merged_a1 = fx.wait_event("pr-merged", |e| e["data"]["pr"] == "a1");
    let started_b = fx.wait_event("pr-started", |e| e["data"]["pr"] == "b");
    assert!(id(&merged_a1) < id(&started_b));
    let base_b = fx.git(&fx.web.path, &["rev-parse", "origin/main"]);
    assert_eq!(
        started_b["data"],
        json!({"pr": "b", "repo": web, "base": "main", "lead_branch": "cpd/chain/b", "lead_worktree": fx.lead(&fx.web, "b").display().to_string(),
               "base_sha": base_b, "reused": false, "pushed": true, "by": "engine"})
    );
    assert_eq!(hooks(&fx, "web").lines().next(), Some("pre-start cpd/chain/b"));
    assert_eq!(fx.pr_status("a1"), "merged");
    assert_eq!(fx.pr_status("a2"), "waiting");
    assert_eq!(fx.ok(&["plan", "show", "--plan", PLAN])["plan"]["phase"], "dispatch");

    // ---------- 8. b's Job runs in the other repo, merges; pr open --pr b
    let client = fx.wait_event("job-merged", |e| e["job"] == "client");
    let started_client = of_kind(&fx.events(), "attempt-started").into_iter().find(|e| e["job"] == "client").unwrap().clone();
    assert!(id(&started_b) < id(&started_client), "client ran before b started");
    let b_tip = client["data"]["sha"].as_str().unwrap().to_owned();
    assert_eq!(fx.git(&fx.web.origin, &["rev-parse", "refs/heads/cpd/chain/b"]), b_tip);
    assert_eq!(fx.git(&fx.web.path, &["log", "--format=%s", "main..cpd/chain/b"]), "client-a1");
    let all = fx.wait_event("all-merged", |e| e["data"]["pr"] == "b");
    assert_eq!(all["data"]["merged"], json!(["client"]));
    let out = fx.ok(&["pr", "open", "--pr", "b", "--plan", PLAN]);
    assert_eq!((&out["phase"], &out["pr"]["pr"]["number"]), (&json!("dispatch"), &json!(12)));
    assert_eq!(fx.pr_status("a2"), "waiting");

    // ---------- 9. GitHub merges b: E7 starts a2, whose base holds a1's merge (A9)
    // This repo's own `main` has not moved: only a fresh fetch of `origin/main` can hold a1.
    assert_ne!(fx.git(&fx.shared.path, &["rev-parse", "main"]), a1_tip);
    fx.github_merges(&fx.web, "cpd/chain/b", 12);
    let merged_b = fx.wait_event("pr-merged", |e| e["data"]["pr"] == "b");
    let started_a2 = fx.wait_event("pr-started", |e| e["data"]["pr"] == "a2");
    assert!(id(&merged_b) < id(&started_a2));
    let a2_lead = fx.lead(&fx.shared, "a2");
    assert_eq!(
        started_a2["data"],
        json!({"pr": "a2", "repo": shared, "base": "main", "lead_branch": "cpd/chain/a2", "lead_worktree": a2_lead.display().to_string(),
               "base_sha": a1_tip, "reused": false, "pushed": true, "by": "engine"})
    );
    assert_eq!(fx.git(&a2_lead, &["rev-parse", "HEAD"]), a1_tip);
    fx.git(&fx.shared.path, &["merge-base", "--is-ancestor", &a1_tip, "refs/heads/cpd/chain/a2"]);
    assert_eq!(fx.git(&fx.shared.origin, &["rev-parse", "refs/heads/cpd/chain/a2"]), a1_tip);
    assert!(hooks(&fx, "shared").contains("pre-start cpd/chain/a2\n"), "{}", hooks(&fx, "shared"));

    // ---------- 10. a2's Job sees a1's file; the review Job, a fork of the Lead, ends merged
    let wire = fx.wait_event("job-merged", |e| e["job"] == "wire");
    let audit = fx.wait_event("job-merged", |e| e["job"] == "audit");
    assert!(id(&started_a2) < id(&wire) && id(&wire) < id(&audit), "the review Job needs the Job added before it");
    let wire_attempt = fx.ok(&["attempt", "show", "wire-a1", "--plan", PLAN])["attempt"].clone();
    assert_eq!(wire_attempt["outcome"], "merged");
    assert_eq!(fs::read_to_string(a2_lead.join("schema.txt")).unwrap(), "schema\n");
    assert_eq!(fs::read_to_string(a2_lead.join("wire.txt")).unwrap(), "wire\n");
    let wire_tip = wire["data"]["sha"].as_str().unwrap();
    assert_eq!(fx.git(&fx.shared.path, &["log", "--format=%s", &format!("{a1_tip}..cpd/chain/a2")]), "wire-a1");
    // It changed nothing: the Lead Branch is where `wire` left it, and the Attempt's `merged_sha` is that tip.
    assert_eq!(fx.git(&fx.shared.path, &["rev-parse", "refs/heads/cpd/chain/a2"]), wire_tip);
    assert_eq!(audit["data"]["sha"], wire_tip);
    let shown = fx.ok(&["job", "show", "audit", "--plan", PLAN])["job"].clone();
    assert_eq!((&shown["review"], &shown["status"], &shown["needs"]), (&json!(true), &json!("merged"), &json!(["wire"])));
    let attempt = fx.ok(&["attempt", "show", "audit-a1", "--plan", PLAN])["attempt"].clone();
    assert_eq!((&attempt["outcome"], &attempt["cause"], &attempt["merged_sha"]), (&json!("merged"), &json!("merged"), &json!(wire_tip)));
    let findings = fs::read_to_string(fx.env.cpd_home().join(format!("run/{PLAN}/reviews/audit.md"))).unwrap();
    assert_eq!(findings, "# Findings\nwire.txt\n", "the diff against the remote base holds only a2's own work");
    let submit = submitted(&fx.shared.path.parent().unwrap().join("submit-audit-a1.json"));
    assert_eq!(submit["submitted"], true, "{submit}");
    // Its Worker is a fork of the Plan's Lead, in the hidden work session.
    let worker = fx.ok(&["session", "show", attempt["worker_session"].as_str().unwrap()])["session"].clone();
    assert_eq!((&worker["role"], &worker["forked_from"]), (&json!("worker"), &json!(lead_session)));
    let pi = fx.env.calls("pi");
    let fork = pi.iter().find(|c| flag(c, "--session-id") == worker["id"].as_str()).unwrap();
    assert_eq!(flag(fork, "--fork"), lead["file"].as_str(), "{fork:?}");
    let plain = pi.iter().find(|c| flag(c, "-n") == Some("chain wire a1")).unwrap();
    assert!(!plain.contains(&"--fork".to_owned()), "a Job that is not a review starts a fresh session: {plain:?}");

    // ---------- 11. a2: all-merged, pr open --pr a2 moves the Plan to `pr`
    let all = fx.wait_event("all-merged", |e| e["data"]["pr"] == "a2");
    assert_eq!(all["data"], json!({"pr": "a2", "merged": ["audit", "wire"], "cancelled": []}));
    let out = fx.ok(&["pr", "open", "--pr", "a2", "--plan", PLAN]);
    assert_eq!((&out["phase"], &out["pr"]["pr"]["number"]), (&json!("deploy"), &json!(13)));
    assert_eq!(fx.git(&fx.shared.origin, &["rev-parse", "refs/heads/cpd/chain/a2"]), wire_tip);
    let moved = of_kind(&fx.events(), "plan-phase").pop().unwrap().clone();
    assert_eq!(moved["data"]["to"], "deploy");
    assert_eq!(
        moved["data"]["prs"],
        json!([{"key": "a1", "number": 11, "url": "https://github.com/acme/repo/pull/11"},
               {"key": "a2", "number": 13, "url": "https://github.com/acme/repo/pull/13"},
               {"key": "b", "number": 12, "url": "https://github.com/acme/repo/pull/12"}])
    );
    assert_eq!(fx.ok(&["plan", "show", "--plan", PLAN])["plan"]["phase"], "deploy");

    // ---------- 12. GitHub merges a2: plan-done, once (A10)
    fx.github_merges(&fx.shared, "cpd/chain/a2", 13);
    let done = fx.wait_event("plan-done", |_| true);
    let keys: Vec<(&str, i64)> = done["data"]["prs"].as_array().unwrap().iter().map(|p| (p["key"].as_str().unwrap(), p["number"].as_i64().unwrap())).collect();
    assert_eq!(keys, [("a1", 11), ("a2", 13), ("b", 12)]);
    assert_eq!(fx.ok(&["plan", "show", "--plan", PLAN])["plan"]["phase"], "done");
    assert!(["a1", "b", "a2"].iter().all(|k| fx.pr_status(k) == "merged"));
    // Three more whole ticks (each with its poll): still one `plan-done`, nothing new started.
    fx.env.wait_whole_ticks(3);
    let events = fx.events();
    assert_eq!(of_kind(&events, "plan-done").len(), 1);
    assert_eq!(of_kind(&events, "pr-started").len(), 3);
    assert_eq!(of_kind(&events, "pr-merged").len(), 3);
    assert_eq!(of_kind(&events, "all-merged").len(), 3);

    // ---------- the whole run: no `error` Event, nothing tampered, nothing left running
    assert!(of_kind(&events, "error").is_empty(), "{:?}", of_kind(&events, "error"));
    assert_eq!(fx.ok(&["plan", "show", "--plan", PLAN])["tamper"], json!([]));
    assert_eq!(fx.ok(&["attempt", "list", "--active", "--plan", PLAN])["attempts"], json!([]));
    kill(Pid::from_raw(engine), Signal::SIGTERM).unwrap();
    fx.wait_event("watch-stopped", |e| e["data"]["pid"] == engine);
}
