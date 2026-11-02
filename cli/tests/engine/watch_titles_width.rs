//! Engine step E10 passes the chat slot's width to `session retitle` (contract §3.7 E10, commands.md §10.14; hs-d543),
//! in-process on a prepared database with the fake tmux server of the tmux route. The width is seen where it has an effect:
//! the prompt of the fake `pi` names the limit N = width - 8. The detached `cpd session retitle` child is recorded by the
//! `FakeSpawn` and run by the test (`Fx::children`).

use crate::common;

use std::fs;

use common::fx::Fx as Ip;
use cpd_core::host::runner::Reply;

/// The chat slot's width, odd and large so that N = 115 occurs nowhere else in a prompt.
const WIDTH: u32 = 123;

struct Fx {
    env: Ip,
    /// The janitor lock, held for the test: E16 starts no `cpd janitor`, so the launches are the retitles' alone.
    _janitor: fs::File,
}

/// A tick a second, the Tier catalog of a pi install, a fake `pi` that answers a title, Context `ctx` and one idle Manager
/// `due` with a live pane (session `x`) whose turn ended a minute ago and that has no title: E10 retitles it as soon as there
/// is a chat slot. The engine runs in-process on virtual time, its `cpd pr poll` answered.
fn fx() -> Fx {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("");
    env.engine_routes_on_tmux();
    fs::create_dir_all(env.env.root().join("install")).unwrap();
    fs::write(env.env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    env.fake_pi(Reply::ok("TITLE: Width aware titles\n"));
    let out = env.tmux(&[
        "new-session",
        "-d",
        "-s",
        "x",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let file = env.env.root().join("due.jsonl");
    let lines = [
        serde_json::json!({"type": "session", "id": "due"}),
        serde_json::json!({"type": "message", "message": {"role": "user", "content": "How do we title Sessions?"}}),
        serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "With an auxiliary agent."}]}}),
    ];
    fs::write(
        &file,
        lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let home = cpd_core::home::CpdHome::resolve(Some(env.env.cpd_home().into()), None).unwrap();
    fs::create_dir_all(home.locks_dir()).unwrap();
    let janitor = fs::File::create(home.locks_dir().join("janitor.lock")).unwrap();
    assert!(
        cpd_core::lock::try_lock(&janitor).unwrap(),
        "the fixture takes the janitor lock"
    );
    let fx = Fx {
        env,
        _janitor: janitor,
    };
    fx.fixture(&format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
           ('1.0.0', 'H', 'p/haiku', '[0,1,2,3]', 't'), ('1.0.0', 'G', 'p/gemini', '[]', 't');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, file, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('due', 'manager', 'ctx', 'due', '{file}', '/', 'O3', '{pane}', 'idle', 't', 't');
         INSERT INTO events (txn, at, kind, context, session, text, data)
           VALUES (cpd_txn(), '{at}', 'session-state', 'ctx', 'due', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"idle\"}}');",
        file = file.display(),
        at = fx.env.stamp_ago(60_000)
    ));
    fx
}

impl Fx {
    fn fixture(&self, sql: &str) {
        self.env.env.sql(sql);
    }

    /// The engine runs `naps` naps of virtual time, in this process, and the `cpd` children it starts run before each nap passes.
    fn run(&self, naps: usize) {
        let mut children = self.env.children();
        self.env
            .watch(naps, move |_| {
                children.run_pending();
            })
            .unwrap();
    }

    fn titled(&self) -> i64 {
        self.env
            .env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'session-titled'")
    }

    /// The cockpit's `main` window, `WIDTH` columns wide, with one pane in it: the chat slot (the placeholder).
    fn chat_slot(&self) {
        let out = self.env.tmux(&[
            "new-session",
            "-d",
            "-s",
            "cpd",
            "-n",
            "main",
            "-x",
            &WIDTH.to_string(),
            "-y",
            "30",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "600",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        assert!(
            self.env
                .tmux(&["set", "-p", "-t", &pane, "@cpd_view", "chat"])
                .status
                .success()
        );
        let width = self
            .env
            .tmux(&["display", "-p", "-t", &pane, "#{pane_width}"]);
        assert_eq!(
            String::from_utf8(width.stdout).unwrap().trim(),
            WIDTH.to_string(),
            "the slot is {WIDTH} columns wide"
        );
    }
}

/// With a chat slot of `WIDTH` columns E10 retitles the due Session with `--width` = that width: the prompt names
/// N = 115.
#[test]
fn e10_passes_the_chat_slot_width_to_retitle() {
    let fx = fx();
    fx.chat_slot();
    fx.run(3);
    assert_eq!(fx.titled(), 1, "the Session is retitled");
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let prompt = calls[0].args.last().unwrap();
    assert!(
        prompt.contains(&(WIDTH - 8).to_string()),
        "the prompt names the limit, {WIDTH} - 8: {prompt}"
    );
}

/// With no chat slot pane in the cockpit E10 starts no retitle: after whole ticks there is no run and no pi call, and
/// the Session stays due: when the slot appears, it is retitled.
#[test]
fn e10_starts_no_retitle_without_a_chat_slot() {
    let fx = fx();
    // The cockpit exists but its `main` window has no chat slot: no pane with `@cpd_session` or `@cpd_view` `chat`/`history`.
    let out = fx.env.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-n",
        "main",
        "sleep",
        "600",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    fx.run(30);
    assert!(
        fx.env.calls("pi").is_empty(),
        "no retitle without a chat slot: {:?}",
        fx.env.calls("pi")
    );
    assert!(
        fx.env.fakes.spawn.launches().is_empty(),
        "no run was started: {:?}",
        fx.env.fakes.spawn.launches()
    );
    assert!(
        !fx.env
            .env
            .cpd_home()
            .join("run/titles")
            .join("due.log")
            .exists(),
        "no run was started"
    );
    assert_eq!(fx.titled(), 0);
    assert_eq!(
        fx.env
            .env
            .query::<Option<String>>("SELECT titled_at FROM sessions WHERE id = 'due'"),
        None
    );

    // The Session stayed due: with the slot in place it is retitled.
    let out = fx
        .env
        .tmux(&["list-panes", "-t", "cpd:main", "-F", "#{pane_id}"]);
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(
        fx.env
            .tmux(&["set", "-p", "-t", &pane, "@cpd_view", "chat"])
            .status
            .success()
    );
    fx.run(3);
    assert_eq!(
        fx.titled(),
        1,
        "the Session is retitled once the slot exists"
    );
}
