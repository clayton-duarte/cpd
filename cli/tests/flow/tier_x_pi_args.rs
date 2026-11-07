//! Tier `X` sends pi no effort (contract §9 "Tier rules": R3): the title writer (`session retitle`, Tier `GX`) and the
//! distiller (`context distill`, Tier `GX`) pass `--model <provider>/<model-id>` with no `:<level>` suffix.
//! pi is a route of the `FakeRunner`; its argv is the runner's record (`fx.calls("pi")`), so it is observed directly.

use crate::common;

use std::fs;
use std::sync::Arc;

use common::fx::Fx;
use cpd_core::host::runner::Reply;
use serde_json::json;

/// The `--model` value of the one pi call `fx` recorded.
fn model_of_the_only_pi_call(fx: &Fx) -> String {
    let calls = fx.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let args = &calls[0].args;
    let at = args.iter().position(|a| a == "--model").expect("--model") + 1;
    args[at].clone()
}

/// pi's install directory, as the catalog reader looks for it.
fn install_pi(env: &common::TestEnv) {
    let install = env.root().join("install");
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("current-version"), "1.0.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = install
            .join("releases/1.0.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.js"), "").unwrap();
    }
}

/// `session retitle` of Lead `lead1`: its pi argument.
fn retitle_model() -> String {
    let fx = Fx::with_ports();
    let env = &fx.env;
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    let pi = common::session::FakePi::install(&fx);
    pi.say(Reply::exit(0, "TITLE: A title\n", ""));
    install_pi(env);
    let file = env.root().join("lead1.jsonl");
    let lines = [
        json!({"type": "session", "id": "lead1"}),
        json!({"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "message 1"}]}}),
        json!({"type": "message", "message": {"role": "assistant", "content": "message 2"}}),
    ];
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    fs::write(&file, text.join("\n") + "\n").unwrap();
    env.sql(&format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
           ('1.0.0', 'H', 'p/haiku', '[1,2,3]', 't'), ('1.0.0', 'G', 'p/gemini', '[]', 't');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at, archived_at, forked_from) VALUES
           ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{}', '/', 'O3', 'idle', 't', 't1', NULL, NULL);
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');",
        file.display()
    ));
    fx.cpd(&["session", "retitle", "lead1", "--width", "68"])
        .ok();
    model_of_the_only_pi_call(&fx)
}

/// `context distill` of Context `ctx`: its pi argument.
fn distill_model() -> String {
    let fx = Fx::with_ports();
    let env = &fx.env;
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\ncontext_curation_min_records = 1\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    fx.fake_node(Reply::ok(
        r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium","high","xhigh","max"],"H":["off","low","medium","high"],"G":["off"]}
"#,
    ));
    install_pi(env);
    let _pi: Arc<common::pi::FakePi> = common::pi::FakePi::install(&fx);
    fx.cpd(&["context", "new", "ctx", "--title", "Ctx", "--owner", "acme"])
        .ok();
    fx.cpd(&["context", "decide", "Use sqlite.", "--context", "ctx"])
        .ok();
    fx.cpd(&["context", "distill", "--context", "ctx"]).ok();
    model_of_the_only_pi_call(&fx)
}

#[test]
fn retitle_and_distill_pass_x_without_a_level_suffix() {
    assert_eq!(retitle_model(), "p/gemini", "the title writer runs on GX");
    assert_eq!(distill_model(), "p/gemini", "the distiller runs on GX");
}
