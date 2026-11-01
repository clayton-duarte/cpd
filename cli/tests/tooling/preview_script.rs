//! `scripts/preview-tui.sh`: previews a build in the cockpit's views only.
//! Fakes only: tmux, the installed cpd and cargo (which writes a fake preview cpd).

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{TestEnv, write_executable};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/preview-tui.sh");
const CARGO_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/cargo.sh");

/// The cockpit's panes: `pane_id|pane_dead|@cpd_view|@cpd_session`.
const PANES: &str = "%1|0|tree|\n%2|0|plan|\n%3|0|widgets|\n%4|0|statusbar|\n%5|0|chat|\n%6|0||lead-1\n%7|0|worker|\n";

struct Preview {
    env: TestEnv,
    worktree: PathBuf,
    dir: PathBuf,
    log: PathBuf,
}

struct Out {
    exit: i32,
    stderr: String,
}

impl Preview {
    /// A cockpit of five views, a Lead and a Worker, an installed `cpd` and a fake cargo.
    /// The fakes append every call to one log, so the order across programs shows.
    fn new() -> Preview {
        let env = TestEnv::new();
        let log = env.root().join("log");
        let dir = env.root().join("preview");
        let worktree = env.git_repo("wt");
        fs::create_dir_all(worktree.join("scripts")).unwrap();
        fs::copy(CARGO_SCRIPT, worktree.join("scripts/cargo.sh")).unwrap();
        fs::write(worktree.join("Cargo.toml"), "").unwrap();

        env.fake(
            "tmux",
            &format!(
                r#"echo "tmux $*" >> "$LOG"
[ "$1 $2 $3 $4 $5" = "-u -L cpdsock -f /dev/null" ] || {{ echo "bad tmux prefix: $*" >&2; exit 98; }}
case "$6" in
  list-windows) [ -z "${{NO_COCKPIT:-}}" ] && echo main ;;
  list-panes) printf '%b' '{}' ;;
esac
exit 0"#,
                PANES.replace('\n', "\\n")
            ),
        );
        env.fake(
            "cpd",
            r#"echo "installed $*" >> "$LOG"
case "$1" in
  config) echo '{"ok":true,"config":{"tmux":{"cockpit_session":"cockpit","socket":"cpdsock","work_session":"work"}}}' ;;
  *) echo '{"ok":true}' ;;
esac"#,
        );
        // The fake cargo writes the fake preview cpd; PREVIEW_PLAN_LIST is what its `plan list` answers.
        let cargo = env.root().join("fake-cargo");
        write_executable(
            &cargo,
            r#"#!/bin/sh
echo "cargo $*" >> "$LOG"
mkdir -p "$CARGO_TARGET_DIR/release"
cat > "$CARGO_TARGET_DIR/release/cpd" <<'FAKE'
#!/bin/sh
echo "preview $*" >> "$LOG"
case "$1" in
  plan)
    if [ -n "${PREVIEW_PLAN_LIST:-}" ]; then echo "$PREVIEW_PLAN_LIST"; exit 5; fi
    echo '{"ok":true,"plans":[]}' ;;
  *) echo '{"ok":true}' ;;
esac
FAKE
chmod 755 "$CARGO_TARGET_DIR/release/cpd"
"#,
        );
        Preview { env, worktree, dir, log }
    }

    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Out {
        let mut cmd = self.env.command("/bin/sh");
        cmd.arg(SCRIPT)
            .args(args)
            .env("CARGO", self.env.root().join("fake-cargo"))
            .env("CPD_PREVIEW_DIR", &self.dir)
            .env("LOG", &self.log);
        for (k, v) in vars {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out { exit: out.status.code().unwrap(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
    }

    fn on(&self, vars: &[(&str, &str)]) -> Out {
        self.run(&["on", self.worktree.to_str().unwrap()], vars)
    }

    fn log(&self) -> Vec<String> {
        fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// The `respawn-pane` calls of the fake tmux, as argv after the tmux prefix.
    fn respawns(&self) -> Vec<Vec<String>> {
        self.env
            .calls("tmux")
            .into_iter()
            .filter(|c| c[5] == "respawn-pane")
            .map(|c| c[6..].to_vec())
            .collect()
    }

    /// The `set` calls of the fake tmux, as argv after the tmux prefix.
    fn sets(&self) -> Vec<Vec<String>> {
        self.env.calls("tmux").into_iter().filter(|c| c[5] == "set").map(|c| c[5..].to_vec()).collect()
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }
}

fn respawn_args(pane: &str, cpd_home: &Path, cpd: &str, view: &str) -> Vec<String> {
    let home = cpd_home.to_str().unwrap();
    ["-k", "-t", pane, "-c", home, "-e", &format!("CPD_HOME={home}"), "--", cpd, "tui", "view", view]
        .map(str::to_owned)
        .to_vec()
}

const VIEW_PANES: [(&str, &str); 5] =
    [("%1", "tree"), ("%2", "plan"), ("%3", "widgets"), ("%4", "statusbar"), ("%5", "chat")];

#[test]
fn on_respawns_the_five_views_with_the_preview_and_leaves_lead_and_worker_alone() {
    let p = Preview::new();
    let out = p.on(&[]);
    assert_eq!(out.exit, 0, "{}", out.stderr);

    let preview = p.dir.join("cpd");
    let expected: Vec<_> =
        VIEW_PANES.iter().map(|(pane, view)| respawn_args(pane, &p.env.cpd_home(), preview.to_str().unwrap(), view)).collect();
    assert_eq!(p.respawns(), expected);

    let sha = p.env.git(&p.worktree, &["rev-parse", "--short", "HEAD"]);
    let wt = p.worktree.display();
    assert_eq!(fs::read_to_string(p.state()).unwrap(), format!("{wt}\n{sha}\n"));
    assert!(out.stderr.contains(&format!("previewing {wt} @ {sha}; revert with: sh ")), "{}", out.stderr);
    assert!(out.stderr.contains("preview-tui.sh off"), "{}", out.stderr);
    assert!(out.stderr.contains("Worker view pane is open"), "{}", out.stderr);
    assert!(out.stderr.contains("reinstall"), "{}", out.stderr);
}

#[test]
fn on_sets_the_preview_option_to_the_worktree_and_sha_after_the_views_are_respawned() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    let sha = p.env.git(&p.worktree, &["rev-parse", "--short", "HEAD"]);
    let name = p.worktree.file_name().unwrap().to_str().unwrap();
    let sets = p.sets();
    assert_eq!(sets, [vec!["set", "-g", "@cpd_preview", &format!("{name} {sha}")]], "{sets:?}");
    let calls = p.env.calls("tmux");
    let at = |verb: &str| calls.iter().rposition(|c| c[5] == verb).unwrap();
    assert!(at("respawn-pane") < at("set"), "{calls:?}");
    // Twice is the same: the option is set again to the same value.
    assert_eq!(p.on(&[]).exit, 0);
    assert_eq!(p.sets().len(), 2);
    assert_eq!(p.sets()[0], p.sets()[1]);
}

#[test]
fn off_unsets_the_preview_option_and_twice_is_the_same() {
    let p = Preview::new();
    for _ in 0..2 {
        let out = p.run(&["off"], &[]);
        assert_eq!(out.exit, 0, "{}", out.stderr);
    }
    let unset = vec!["set", "-gu", "@cpd_preview"];
    assert_eq!(p.sets(), [unset.clone(), unset]);
}

#[test]
fn a_refused_on_and_a_missing_cockpit_leave_the_preview_option_alone() {
    let p = Preview::new();
    assert_eq!(p.on(&[("NO_COCKPIT", "1")]).exit, 1);
    assert!(p.sets().is_empty(), "{:?}", p.sets());
}

#[test]
fn on_builds_into_the_preview_dir_only() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    let wt = p.worktree.display();
    assert!(
        p.log().contains(&format!("cargo build --release --locked --manifest-path {wt}/Cargo.toml -p cpd")),
        "{:?}",
        p.log()
    );
    assert!(p.dir.join("target/release/cpd").exists());
    assert!(p.dir.join("cpd").exists());
    assert!(!p.worktree.join("target").exists());
}

#[test]
fn on_starts_the_engine_on_the_installed_cpd_before_the_previews_tui() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    let log = p.log();
    let at = |line: &str| log.iter().position(|l| l == line).unwrap_or_else(|| panic!("{line} not in {log:?}"));
    assert!(at("installed watch --ensure") < at("preview tui"));
    assert!(at("preview plan list") < at("installed watch --ensure"));
    assert!(at("preview tui") < at("tmux -u -L cpdsock -f /dev/null list-panes -t =cockpit:main -F #{pane_id}|#{pane_dead}|#{@cpd_view}|#{@cpd_session}"));
    assert!(!log.contains(&"installed tui".to_owned()));
}

#[test]
fn on_refuses_a_preview_that_cannot_read_the_store_and_changes_nothing_in_tmux() {
    let p = Preview::new();
    let refusal = r#"{"ok":false,"error":{"code":"store_too_new"}}"#;
    let out = p.on(&[("PREVIEW_PLAN_LIST", refusal)]);
    assert_ne!(out.exit, 0);
    assert!(out.stderr.contains("store_too_new"), "{}", out.stderr);
    assert!(out.stderr.contains("merge main into the branch first"), "{}", out.stderr);
    assert!(p.env.calls("tmux").is_empty(), "{:?}", p.env.calls("tmux"));
    assert!(p.env.calls("cpd").iter().all(|c| c[0] != "watch"), "{:?}", p.env.calls("cpd"));
    assert!(!p.state().exists());
}

#[test]
fn off_respawns_the_five_views_with_the_installed_cpd_runs_tui_and_removes_the_state() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    assert!(p.state().exists());
    let before = p.respawns().len();

    let out = p.run(&["off"], &[]);
    assert_eq!(out.exit, 0, "{}", out.stderr);
    let expected: Vec<_> = VIEW_PANES.iter().map(|(pane, view)| respawn_args(pane, &p.env.cpd_home(), "cpd", view)).collect();
    assert_eq!(p.respawns()[before..], expected[..]);
    assert_eq!(p.log().last().map(String::as_str), Some("installed tui"));
    assert!(!p.state().exists());
}

#[test]
fn off_twice_succeeds() {
    let p = Preview::new();
    let first = p.run(&["off"], &[]);
    assert_eq!(first.exit, 0, "{}", first.stderr);
    let second = p.run(&["off"], &[]);
    assert_eq!(second.exit, 0, "{}", second.stderr);
    assert_eq!(p.respawns().len(), 10);
    assert!(!p.state().exists());
}

#[test]
fn no_cockpit_exits_1_for_on_after_building_and_for_off() {
    let p = Preview::new();
    let on = p.on(&[("NO_COCKPIT", "1")]);
    assert_eq!(on.exit, 1);
    assert!(on.stderr.contains("no cockpit"), "{}", on.stderr);
    assert!(p.dir.join("cpd").exists(), "on builds first");
    assert!(p.respawns().is_empty());
    assert!(!p.state().exists());

    let off = p.run(&["off"], &[("NO_COCKPIT", "1")]);
    assert_eq!(off.exit, 1);
    assert!(off.stderr.contains("no cockpit"), "{}", off.stderr);
    assert!(p.respawns().is_empty());
}

#[test]
fn rejects_other_arguments() {
    let p = Preview::new();
    let status = Command::new("/bin/sh").arg(SCRIPT).arg("sideways").env_clear().env("HOME", p.env.home()).status().unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(p.env.calls("tmux").is_empty());
}
