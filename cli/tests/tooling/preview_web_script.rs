//! `scripts/preview-web.sh`: previews the web UI's Vite dev server of a worktree in the work session's `web` window.
//! Fakes only: tmux, cpd, git and pnpm. Every fake records its calls (`TestEnv::calls`).

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::TestEnv;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/preview-web.sh");
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

const UNSET: &str = "env -u CPD_SESSION -u PI_SESSION_ID -u CPD_SESSION_ID -u CPD_ATTEMPT_ID pnpm";

struct Preview {
    env: TestEnv,
    worktree: PathBuf,
    dir: PathBuf,
}

struct Out {
    exit: i32,
    text: String,
}

impl Preview {
    fn new() -> Preview {
        let env = TestEnv::new();
        env.fake(
            "tmux",
            r#"[ "$1 $2 $3 $4 $5" = "-u -L cpdsock -f /dev/null" ] || { echo "bad tmux prefix: $*" >&2; exit 98; }
case "$6" in
  list-windows) echo main; [ -z "${NO_WEB_WINDOW:-}" ] && echo web ;;
esac
exit 0"#,
        );
        env.fake(
            "cpd",
            r#"case "$1" in
  config) echo '{"ok":true,"config":{"tmux":{"cockpit_session":"cockpit","socket":"cpdsock","work_session":"work"}}}' ;;
  *) echo '{"ok":true}' ;;
esac"#,
        );
        env.fake(
            "git",
            r#"case "$*" in
  *"worktree add"*)
    for a in "$@"; do prev=${last:-}; last=$a; done
    # the worktree path is the argument before the last (origin/main)
    n=$#; i=0
    for a in "$@"; do i=$((i+1)); [ "$i" = $((n-1)) ] && mkdir -p "$a"; done ;;
  *"rev-parse --short HEAD"*) echo abc1234 ;;
esac
exit 0"#,
        );
        // Every fake of the reload flow appends to $ORDER_LOG, so the order across programs is readable.
        env.fake(
            "pnpm",
            r#"echo "pnpm $*" >> "${ORDER_LOG:?}"
case "$*" in *typecheck*) [ -z "${TYPECHECK_FAILS:-}" ] || { echo "typecheck: error TS2322" >&2; exit 1; } ;; esac
exit 0"#,
        );
        // Exits at once: no process outlives the test.
        env.fake(
            "node",
            r#"echo "node $* CPD_MODE=${CPD_MODE-unset} CPD_CHECKOUT=${CPD_CHECKOUT-unset} CPD_ELECTRON_DEV_URL=$CPD_ELECTRON_DEV_URL" >> "${ORDER_LOG:?}"
[ -z "${NODE_FIFO:-}" ] || echo started > "$NODE_FIFO"
exit 0"#,
        );
        // `kill -0` answers with $KILL0_STATUS (default 0: alive).
        env.fake(
            "kill",
            r#"echo "kill $*" >> "${ORDER_LOG:?}"
[ "$1" = -0 ] && exit "${KILL0_STATUS:-0}"
exit 0"#,
        );
        let worktree = env.root().join("cpd.preview.web");
        let dir = env.root().join("preview");
        Preview { env, worktree, dir }
    }

    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Out {
        let mut cmd = self.env.command("/bin/sh");
        cmd.arg(SCRIPT)
            .args(args)
            .env("CPD_PREVIEW_DIR", &self.dir)
            .env("CPD_WEB_PREVIEW_WORKTREE", &self.worktree)
            .env("ORDER_LOG", self.order_log());
        for (k, v) in vars {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out {
            exit: out.status.code().unwrap(),
            text: format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        }
    }

    fn on(&self, vars: &[(&str, &str)]) -> Out {
        self.run(&["on", self.worktree.to_str().unwrap()], vars)
    }

    /// The tmux calls of one verb, as argv after the tmux prefix (`-u -L <socket> -f /dev/null`).
    fn tmux(&self, verb: &str) -> Vec<Vec<String>> {
        self.env
            .calls("tmux")
            .into_iter()
            .filter(|c| c[5] == verb)
            .map(|c| c[5..].to_vec())
            .collect()
    }

    fn order_log(&self) -> PathBuf {
        self.env.root().join("order.log")
    }

    /// Lines of the fakes pnpm, node and kill, in the order they ran.
    fn order(&self) -> Vec<String> {
        fs::read_to_string(self.order_log())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn run_dir(&self) -> PathBuf {
        self.env.cpd_home().join("run")
    }

    fn health_file(&self, text: &str) {
        fs::create_dir_all(self.run_dir()).unwrap();
        fs::write(self.run_dir().join("preview-health.json"), text).unwrap();
    }

    fn watchdog_dir(&self) -> PathBuf {
        self.dir.join("watchdog")
    }

    fn watchdog_pid_file(&self) -> PathBuf {
        self.dir.join("watchdog.pid")
    }

    fn node_fifo(&self) -> PathBuf {
        self.env.root().join("node.fifo")
    }

    /// `on` with the fake node told to signal through a named pipe once it has recorded its call.
    fn on_signalled(&self) -> Out {
        let made = std::process::Command::new("mkfifo")
            .arg(self.node_fifo())
            .status()
            .unwrap();
        assert!(made.success());
        let fifo = self.node_fifo();
        self.on(&[("NODE_FIFO", fifo.to_str().unwrap())])
    }

    /// The order log once the detached fake node has recorded its call: blocks on the pipe it writes to (after `on_signalled`).
    fn order_with_node(&self) -> Vec<String> {
        fs::read_to_string(self.node_fifo()).unwrap();
        self.order()
    }

    fn state(&self) -> PathBuf {
        self.dir.join("web-state")
    }

    fn pnpm_calls(&self) -> Vec<String> {
        self.env.calls("pnpm").iter().map(|c| c.join(" ")).collect()
    }

    /// No call of the fake git is a checkout, a reset or a clean.
    fn assert_git_never_rewrites(&self) {
        for c in self.env.calls("git") {
            assert!(
                !c.iter()
                    .any(|a| ["checkout", "reset", "clean"].contains(&a.as_str())),
                "{c:?}"
            );
        }
    }

    fn worktree_adds(&self) -> Vec<String> {
        self.env
            .calls("git")
            .iter()
            .map(|c| c.join(" "))
            .filter(|c| c.contains("worktree add"))
            .collect()
    }
}

fn repo() -> String {
    fs::canonicalize(REPO).unwrap().display().to_string()
}

#[test]
fn on_creates_the_preview_worktree_from_origin_main_when_it_is_missing() {
    let p = Preview::new();
    assert!(!p.worktree.exists());
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let wt = p.worktree.display();
    let adds = p.worktree_adds();
    assert_eq!(adds.len(), 1, "{adds:?}");
    assert!(
        adds[0].ends_with(&format!("worktree add -B preview/web {wt} origin/main")),
        "{adds:?}"
    );
    assert!(adds[0].contains(&format!("-C {}", repo())), "{adds:?}");
    assert!(p.worktree.is_dir());
    p.assert_git_never_rewrites();
}

#[test]
fn on_uses_an_existing_worktree_without_touching_it() {
    let p = Preview::new();
    fs::create_dir_all(&p.worktree).unwrap();
    fs::write(p.worktree.join("keep"), "mine").unwrap();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    assert!(p.worktree_adds().is_empty(), "{:?}", p.worktree_adds());
    p.assert_git_never_rewrites();
    assert_eq!(fs::read_to_string(p.worktree.join("keep")).unwrap(), "mine");
    assert!(!p.tmux("respawn-pane").is_empty() || !p.tmux("new-window").is_empty());
}

#[test]
fn on_starts_the_dev_server_in_the_web_window_without_the_session_identity() {
    let p = Preview::new();
    let out = p.on(&[("NO_WEB_WINDOW", "1")]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let wt = p.worktree.display();
    let news = p.tmux("new-window");
    assert_eq!(news.len(), 1, "{news:?}");
    assert!(p.tmux("respawn-pane").is_empty());
    let args = news[0].join(" ");
    assert!(args.contains("-d -n web -c "), "{args}");
    assert!(args.contains(&format!("-c {wt}")), "{args}");
    assert!(args.contains("=work"), "{args}");
    assert!(
        args.contains(&format!(
            "{UNSET} --dir {wt} --filter cpd-web-client run dev --host"
        )),
        "{args}"
    );
    // The listing asks the work session, by exact name.
    let lists = p.tmux("list-windows");
    assert!(
        lists.iter().any(|l| l.join(" ").contains("-t =work")),
        "{lists:?}"
    );
}

#[test]
fn on_respawns_the_web_window_when_it_exists() {
    let p = Preview::new();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let wt = p.worktree.display();
    assert!(p.tmux("new-window").is_empty());
    let respawns = p.tmux("respawn-pane");
    assert_eq!(respawns.len(), 1, "{respawns:?}");
    let args = respawns[0].join(" ");
    assert!(args.contains("-k"), "{args}");
    assert!(args.contains("-t =work:web"), "{args}");
    assert!(
        args.contains(&format!(
            "{UNSET} --dir {wt} --filter cpd-web-client run dev --host"
        )),
        "{args}"
    );
}

#[test]
fn on_sets_the_preview_option_and_writes_the_state_file() {
    let p = Preview::new();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let wt = p.worktree.display();
    assert_eq!(
        p.tmux("set"),
        [vec![
            "set".to_owned(),
            "-g".to_owned(),
            "@cpd_web_preview".to_owned(),
            "cpd.preview.web abc1234".to_owned()
        ]]
    );
    let calls = p.env.calls("tmux");
    let at = |verb: &str| calls.iter().rposition(|c| c[5] == verb).unwrap();
    assert!(at("respawn-pane") < at("set"), "{calls:?}");
    assert_eq!(
        fs::read_to_string(p.state()).unwrap(),
        format!("{wt}\nabc1234\n")
    );
    assert!(
        out.text.contains(&format!(
            "CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html pnpm --dir {wt}/web run electron"
        )),
        "{}",
        out.text
    );
    // Twice is the same.
    assert_eq!(p.on(&[]).exit, 0);
    assert_eq!(p.tmux("set").len(), 2);
}

#[test]
fn on_builds_electron_when_the_bundle_is_missing() {
    let p = Preview::new();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let wt = p.worktree.display();
    let builds: Vec<_> = p
        .pnpm_calls()
        .into_iter()
        .filter(|c| c.contains("build:electron"))
        .collect();
    assert_eq!(builds.len(), 1, "{builds:?}");
    assert!(builds[0].contains(&format!("--dir {wt}")), "{builds:?}");
    assert!(builds[0].contains("run build:electron"), "{builds:?}");
}

#[test]
fn on_does_not_build_electron_when_the_bundle_exists() {
    let p = Preview::new();
    let dist = p.worktree.join("web/dist-electron");
    fs::create_dir_all(&dist).unwrap();
    fs::write(dist.join("main.js"), "").unwrap();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    assert!(
        !p.pnpm_calls().iter().any(|c| c.contains("build:electron")),
        "{:?}",
        p.pnpm_calls()
    );
    assert!(p.watchdog_pid_file().exists(), "watchdog not started");
}

#[test]
fn off_puts_the_server_back_on_the_primary_checkout_and_clears_the_option() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    assert!(p.state().exists());
    let before = p.tmux("respawn-pane").len();
    for _ in 0..2 {
        let out = p.run(&["off"], &[]);
        assert_eq!(out.exit, 0, "{}", out.text);
    }
    let respawns = p.tmux("respawn-pane");
    assert_eq!(respawns.len(), before + 2, "{respawns:?}");
    let args = respawns[before].join(" ");
    let repo = repo();
    assert!(args.contains("-k"), "{args}");
    assert!(args.contains("-t =work:web"), "{args}");
    assert!(
        args.contains(&format!(
            "{UNSET} --dir {repo} --filter cpd-web-client run dev --host"
        )),
        "{args}"
    );
    let unset = vec!["set", "-gu", "@cpd_web_preview"];
    let sets = p.tmux("set");
    assert_eq!(sets[1..], [unset.clone(), unset], "{sets:?}");
    assert!(!p.state().exists());
    p.assert_git_never_rewrites();
}

#[test]
fn status_prints_the_worktree_and_sha_or_none() {
    let p = Preview::new();
    let none = p.run(&["status"], &[]);
    assert_eq!(none.exit, 0, "{}", none.text);
    assert!(none.text.contains("no web preview"), "{}", none.text);

    assert_eq!(p.on(&[]).exit, 0);
    let some = p.run(&["status"], &[]);
    assert_eq!(some.exit, 0, "{}", some.text);
    assert!(
        some.text
            .lines()
            .any(|l| l == format!("{} abc1234", p.worktree.display())),
        "{}",
        some.text
    );

    assert_eq!(p.run(&["off"], &[]).exit, 0);
    assert!(p.run(&["status"], &[]).text.contains("no web preview"));
}

#[test]
fn a_bad_usage_exits_2() {
    let p = Preview::new();
    for args in [
        &["sideways"][..],
        &[][..],
        &["off", "extra"][..],
        &["status", "extra"][..],
        &["on", "a", "b"][..],
    ] {
        let out = p.run(args, &[]);
        assert_eq!(out.exit, 2, "{args:?}: {}", out.text);
    }
    assert!(p.env.calls("tmux").is_empty());
    assert!(p.env.calls("git").is_empty());
}

const HEALTH: &str = r#"{"status":"healthy","reloadEpoch":3,"pid":4242,"startedAt":"2026-01-01T00:00:00.000Z","healthyAt":"2026-01-01T00:00:01.000Z"}"#;

fn pos(order: &[String], needle: &str) -> usize {
    order
        .iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in {order:?}"))
}

#[test]
fn reload_runs_the_typecheck_before_the_build_and_the_signal() {
    let p = Preview::new();
    p.health_file(HEALTH);
    let wt = p.worktree.display().to_string();
    let out = p.run(&["reload", &wt], &[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order();
    let check = pos(&order, "--filter cpd-web-client run typecheck");
    let build = pos(&order, "--filter cpd-web-client run build:electron");
    let usr1 = pos(&order, "kill -USR1 4242");
    assert!(check < build && build < usr1, "{order:?}");
    assert!(order[check].contains(&format!("--dir {wt}")), "{order:?}");
    assert!(order[build].contains(&format!("--dir {wt}")), "{order:?}");
}

#[test]
fn reload_stops_at_a_failed_typecheck_without_building_or_signalling() {
    let p = Preview::new();
    p.health_file(HEALTH);
    let wt = p.worktree.display().to_string();
    let out = p.run(&["reload", &wt], &[("TYPECHECK_FAILS", "1")]);
    assert_eq!(out.exit, 1, "{}", out.text);
    assert!(out.text.contains("TS2322"), "{}", out.text);
    let order = p.order();
    assert!(
        order.iter().any(|l| l.contains("run typecheck")),
        "{order:?}"
    );
    assert!(
        !order.iter().any(|l| l.contains("build:electron")),
        "{order:?}"
    );
    assert!(!order.iter().any(|l| l.starts_with("kill ")), "{order:?}");
}

#[test]
fn reload_signals_the_pid_of_the_health_file() {
    let p = Preview::new();
    p.health_file(HEALTH);
    let wt = p.worktree.display().to_string();
    let out = p.run(&["reload", &wt], &[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let kills = p.env.calls("kill");
    assert!(
        kills.contains(&vec!["-0".to_owned(), "4242".to_owned()]),
        "{kills:?}"
    );
    assert!(
        kills.contains(&vec!["-USR1".to_owned(), "4242".to_owned()]),
        "{kills:?}"
    );

    // A dead pid: a message, exit 1, no signal.
    let dead = p.run(&["reload", &wt], &[("KILL0_STATUS", "1")]);
    assert_eq!(dead.exit, 1, "{}", dead.text);
    assert!(dead.text.contains("4242"), "{}", dead.text);
    assert_eq!(
        p.env
            .calls("kill")
            .iter()
            .filter(|c| c[0] == "-USR1")
            .count(),
        1
    );
}

#[test]
fn reload_fails_when_the_health_file_has_no_pid() {
    let p = Preview::new();
    let wt = p.worktree.display().to_string();
    // No health file at all, then one without a pid.
    let missing = p.run(&["reload", &wt], &[]);
    assert_eq!(missing.exit, 1, "{}", missing.text);
    p.health_file(r#"{"status":"loading","reloadEpoch":1}"#);
    let out = p.run(&["reload", &wt], &[]);
    assert_eq!(out.exit, 1, "{}", out.text);
    assert!(!out.text.is_empty());
    assert!(
        !p.order().iter().any(|l| l.starts_with("kill ")),
        "{:?}",
        p.order()
    );
}

#[test]
fn on_installs_the_frozen_watchdog_copy_outside_the_worktree() {
    let p = Preview::new();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let frozen = p.watchdog_dir();
    assert!(!frozen.starts_with(&p.worktree));
    for f in [
        "preview-watchdog.mjs",
        "health.ts",
        "watchdog.ts",
        "restore-points.ts",
    ] {
        assert!(frozen.join(f).is_file(), "{f} missing in {frozen:?}");
    }
}

#[test]
fn on_starts_the_watchdog_with_the_worktree_and_the_relaunch_command() {
    let p = Preview::new();
    let out = p.on_signalled();
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order_with_node();
    let node = order
        .iter()
        .find(|l| l.starts_with("node "))
        .unwrap_or_else(|| panic!("watchdog not started: {order:?}"));
    let wt = p.worktree.display();
    let home = p.env.cpd_home();
    assert!(
        node.contains(&format!(
            "{}/preview-watchdog.mjs",
            p.watchdog_dir().display()
        )),
        "{node}"
    );
    assert!(node.contains(&format!("--worktree {wt}")), "{node}");
    assert!(
        node.contains(&format!("--home {}", home.display())),
        "{node}"
    );
    assert!(
        node.contains(&format!("--relaunch pnpm --dir {wt}/web exec electron .")),
        "{node}"
    );
    let pid = fs::read_to_string(p.watchdog_pid_file()).expect("pid file");
    assert!(pid.trim().parse::<u32>().is_ok(), "{pid:?}");
    assert!(home.join("run").is_dir());
}

#[test]
fn on_stops_a_running_watchdog_before_starting_another() {
    let p = Preview::new();
    fs::create_dir_all(&p.dir).unwrap();
    fs::write(p.watchdog_pid_file(), "4242\n").unwrap();
    let out = p.on_signalled();
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order_with_node();
    let term = order
        .iter()
        .position(|l| l == "kill -TERM 4242" || l == "kill -15 4242" || l == "kill -SIGTERM 4242")
        .unwrap_or_else(|| panic!("no SIGTERM to 4242: {order:?}"));
    assert!(term < pos(&order, "node "), "{order:?}");
    assert_ne!(
        fs::read_to_string(p.watchdog_pid_file()).unwrap().trim(),
        "4242"
    );
}

#[test]
fn off_stops_the_watchdog_and_removes_its_pid_file() {
    let p = Preview::new();
    fs::create_dir_all(&p.dir).unwrap();
    fs::write(p.watchdog_pid_file(), "4242\n").unwrap();
    let out = p.run(&["off"], &[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order();
    assert!(
        order.iter().any(|l| {
            l == "kill -TERM 4242" || l == "kill -15 4242" || l == "kill -SIGTERM 4242"
        }),
        "{order:?}"
    );
    assert!(!p.watchdog_pid_file().exists());
}

#[test]
fn off_without_a_watchdog_is_fine() {
    let p = Preview::new();
    let out = p.run(&["off"], &[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    assert!(!p.order().iter().any(|l| l.starts_with("kill ")));
    // A dead pid in the file is simply removed, and nothing is signalled.
    fs::create_dir_all(&p.dir).unwrap();
    fs::write(p.watchdog_pid_file(), "4242\n").unwrap();
    let out = p.run(&["off"], &[("KILL0_STATUS", "1")]);
    assert_eq!(out.exit, 0, "{}", out.text);
    assert!(!p.watchdog_pid_file().exists());
    assert!(
        !p.order()
            .iter()
            .any(|l| l.contains("-TERM") || l.contains("-15"))
    );
}

#[test]
// The relaunch inherits the script environment, so the watchdog must see the dev URL.
fn on_starts_the_watchdog_with_the_dev_url_in_its_environment() {
    let p = Preview::new();
    let out = p.on_signalled();
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order_with_node();
    let node = order
        .iter()
        .find(|l| l.starts_with("node "))
        .unwrap_or_else(|| panic!("watchdog not started: {order:?}"));
    assert!(
        node.ends_with("CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html"),
        "{node}"
    );
}

#[test]
fn on_exports_the_preview_mode_and_checkout_to_the_dev_server() {
    let p = Preview::new();
    let out = p.on(&[("NO_WEB_WINDOW", "1")]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let news = p.tmux("new-window");
    assert_eq!(news.len(), 1, "{news:?}");
    let args = news[0].join(" ");
    let wt = p.worktree.display();
    assert!(args.contains("CPD_MODE=preview"), "{args}");
    assert!(args.contains(&format!("CPD_CHECKOUT={wt}")), "{args}");
}

#[test]
fn on_starts_the_watchdog_with_the_preview_mode_and_checkout_in_its_environment() {
    let p = Preview::new();
    let out = p.on_signalled();
    assert_eq!(out.exit, 0, "{}", out.text);
    let order = p.order_with_node();
    let node = order
        .iter()
        .find(|l| l.starts_with("node "))
        .unwrap_or_else(|| panic!("watchdog not started: {order:?}"));
    assert!(node.contains(" CPD_MODE=preview "), "{node}");
    let wt = p.worktree.display();
    assert!(node.contains(&format!(" CPD_CHECKOUT={wt} ")), "{node}");
}

#[test]
fn off_serves_the_primary_checkout_in_production_without_a_checkout() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    let before = p.tmux("respawn-pane").len();
    let out = p.run(&["off"], &[]);
    assert_eq!(out.exit, 0, "{}", out.text);
    let respawns = p.tmux("respawn-pane");
    let args = respawns[before].join(" ");
    assert!(args.contains("CPD_MODE=production"), "{args}");
    assert!(args.contains("-u CPD_CHECKOUT"), "{args}");
    assert!(!args.contains("CPD_CHECKOUT="), "{args}");
}
