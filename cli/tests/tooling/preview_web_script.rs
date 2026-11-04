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
        env.fake("pnpm", "exit 0");
        let worktree = env.root().join("cpd.preview.web");
        let dir = env.root().join("preview");
        Preview { env, worktree, dir }
    }

    fn run(&self, args: &[&str], vars: &[(&str, &str)]) -> Out {
        let mut cmd = self.env.command("/bin/sh");
        cmd.arg(SCRIPT)
            .args(args)
            .env("CPD_PREVIEW_DIR", &self.dir)
            .env("CPD_WEB_PREVIEW_WORKTREE", &self.worktree);
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
            "CPD_ELECTRON_DEV_URL=http://localhost:5173 pnpm --dir {wt}/web run electron"
        )),
        "{}",
        out.text
    );
    // Twice is the same.
    assert_eq!(p.on(&[]).exit, 0);
    assert_eq!(p.tmux("set").len(), 2);
}

#[test]
fn on_builds_electron_only_when_asked() {
    let p = Preview::new();
    assert_eq!(p.on(&[]).exit, 0);
    assert!(
        !p.pnpm_calls().iter().any(|c| c.contains("build:electron")),
        "{:?}",
        p.pnpm_calls()
    );
    let out = p.on(&[("CPD_PREVIEW_ELECTRON", "1")]);
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
