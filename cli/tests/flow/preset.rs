//! `cpd preset list|show|apply` (commands.md §14.6; contract §8.2 "`[presets.<name>]`", §5.2 `session-model-changed`;
//! the tier-preset switch, D1–D5). `list` and `show` are read-only: `list` prints every preset with the provider of its
//! `O` ref, `show` prints a preset's four refs and the current host `[model]`. `apply` validates the preset and, when
//! its refs differ from the host `[model]`, rewrites only that table of `policy/host.toml` atomically and appends ONE
//! `session-model-changed` per live Lead/Manager whose letter's model changed; every refusal (unknown name, an unlisted
//! provider, an unoffered id) writes nothing. The registry `apply` lists is the model-discovery helper's `--all`
//! answer (one entry per provider: provider, auth, base, ids, built-ins included), which the fake `node` on `PATH`
//! prints through the host runner.
//!
//! These tests are red before the tier-preset switch: `cpd preset` is not a command, so every run here is a `usage`
//! refusal, not the behavior each test pins.

use crate::common;

use std::fs;

use common::{Run, TestEnv, refused};
use serde_json::{Value, json};

/// The registry `apply` lists: `github-copilot` (a built-in) with the four offered ids the presets use.
const REGISTRY: &str = r#"{"ok": true, "pi_version": "1.3.0", "providers": [
  {"provider": "github-copilot", "auth": "oauth", "base": null,
   "ids": ["claude-haiku-4.5", "claude-sonnet-5.5", "claude-opus-5.5", "gemini-3.8-flash"]}]}"#;

/// A `[presets.<name>]` table of four refs for provider `provider` and `letter`.
fn preset(name: &str, provider: &str, letter: &str) -> String {
    format!(
        "[presets.{name}]\nO = \"{provider}/{letter}-o\"\nS = \"{provider}/{letter}-s\"\nH = \"{provider}/{letter}-h\"\nG = \"{provider}/{letter}-g\"\n"
    )
}

/// `[presets.fast]`: the docs' example — O is `haiku`, S/H/G are the host's model for the S, H and G letters.
const FAST: &str = "[presets.fast]\nO = \"github-copilot/claude-haiku-4.5\"\nS = \"github-copilot/claude-sonnet-5.5\"\nH = \"github-copilot/claude-haiku-4.5\"\nG = \"github-copilot/gemini-3.8-flash\"\n";

/// The host `[model]` before an apply: `O` is `opus` (the preset's is `haiku`), the other letters are already the
/// preset's; a comment and another table surround it, so a rewrite must leave them byte-identical.
const HOST: &str = "# not part of the model\n[model]\nO = \"github-copilot/claude-opus-5.5\"\nS = \"github-copilot/claude-sonnet-5.5\"\nH = \"github-copilot/claude-haiku-4.5\"\nG = \"github-copilot/gemini-3.8-flash\"\n\n[pr]\nfeedback.checks = \"none\"\n";

impl TestEnv {
    /// The `session-model-changed` Events, each as `{session, data}`; scoped rows list the Session in `session`.
    fn model_changed(&self) -> Vec<Value> {
        self.read(|conn| {
            let mut stmt = conn.prepare("SELECT session, data FROM events WHERE kind = 'session-model-changed' ORDER BY id")?;
            let rows = stmt
                .query_map([], |r| {
                    let session: Option<String> = r.get(0)?;
                    let data: String = r.get(1)?;
                    Ok(json!({"session": session, "data": serde_json::from_str::<Value>(&data).unwrap_or(Value::Null)}))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }
}

struct Fx {
    env: TestEnv,
}

impl Fx {
    /// A fresh env with a fake pi install (so `locate_pi` and the adapter dir resolve) and a fake `node` that answers
    /// the registry `--all` listing from `registry-answer` (a `0 <json>` line, the `models refresh` helper's seam).
    fn new() -> Fx {
        let env = TestEnv::new();
        env.fake_pi("exit 0");
        fs::create_dir_all(env.root().join("install")).unwrap();
        fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
        let scope = env
            .root()
            .join("install/releases/1.0.0/node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            fs::create_dir_all(scope.join(package).join("dist")).unwrap();
            fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
        }
        let adapter = env.root().join("adapter");
        fs::create_dir_all(&adapter).unwrap();
        fs::write(adapter.join("package.json"), "{}").unwrap();
        let root = env.root().display().to_string();
        env.fake("node", &format!("read -r status json < '{root}/registry-answer'\nn=0\nwhile IFS= read -r line; do n=$((n + 1)); [ \"$n\" -eq 1 ] || json=\"$json$line\"; done < '{root}/registry-answer'\necho \"$json\"\nexit \"$status\"\n"));
        fs::write(
            env.root().join("registry-answer"),
            format!("0 {REGISTRY}\n"),
        )
        .unwrap();
        Fx { env }
    }

    /// `config.toml` pointing `pi_adapter` at the fake adapter dir, with `presets` ("" for none).
    fn config(&self, presets: &str) {
        self.env.write_config(&format!(
            "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n{presets}",
            self.env.root().join("adapter").display()
        ));
    }

    /// The registry answer `apply` uses; `value` is one provider per entry, built-ins included.
    fn registry(&self, value: &Value) {
        let answer = json!({"ok": true, "pi_version": "1.3.0", "providers": value});
        fs::write(
            self.env.root().join("registry-answer"),
            format!("0 {answer}\n"),
        )
        .unwrap();
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.env.cpd_home().join("policy").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn host(&self, text: &str) {
        self.write("host.toml", text);
    }

    fn host_text(&self) -> String {
        fs::read_to_string(self.env.cpd_home().join("policy").join("host.toml")).unwrap()
    }

    fn cpd(&self, args: &[&str]) -> Run {
        self.env.cpd(args)
    }

    /// A live Lead on `O3` (letter O) and a Manager on `S3` (letter S) in a context with no owner, so the host
    /// `[model]` is the one layer that decides their models.
    fn sessions(&self) {
        self.env.sql(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', NULL, 't', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('p1', 'ctx', 'P', 'grounding', 'lead1', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'p1', 'L', '/', 'O3', 'idle', 't', 't'),
                      ('mgr1', 'manager', 'ctx', NULL, 'M', '/', 'S3', 'idle', 't', 't');",
        );
    }
}

// ---------- apply writes the host [model] ----------

#[test]
fn apply_rewrites_host_toml_model_to_the_preset_s_provider() {
    let fx = Fx::new();
    fx.config(FAST);
    fx.host(HOST);
    let config_before = fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap();

    let out = fx.cpd(&["preset", "apply", "fast"]).ok();
    assert_eq!(out["preset"], "fast");
    assert_eq!(out["already"], false);
    assert_eq!(out["changed"], json!([]));
    assert!(
        out["host"].as_str().unwrap().ends_with("policy/host.toml"),
        "{out}"
    );

    // The four letters are rewritten to the preset's refs; the comment and the [pr] table are byte-identical.
    let after = fx.host_text();
    assert!(
        after.starts_with("# not part of the model\n"),
        "the comment survives: {after}"
    );
    assert!(
        after.contains("[model]\n"),
        "the [model] header names the section"
    );
    assert!(after.contains("O = \"github-copilot/claude-haiku-4.5\""));
    assert!(after.contains("S = \"github-copilot/claude-sonnet-5.5\""));
    assert!(after.contains("H = \"github-copilot/claude-haiku-4.5\""));
    assert!(after.contains("G = \"github-copilot/gemini-3.8-flash\""));
    assert!(
        after.contains("[pr]\nfeedback.checks = \"none\""),
        "the other table survives: {after}"
    );
    assert!(
        !after.contains("claude-opus-5.5"),
        "the old O is replaced: {after}"
    );
    // Nothing else is written.
    assert_eq!(
        fs::read_to_string(fx.env.cpd_home().join("config.toml")).unwrap(),
        config_before,
        "config.toml is unchanged"
    );
    assert!(
        !fx.env
            .cpd_home()
            .join("policy")
            .join("host.toml.tmp")
            .exists(),
        "atomic write leaves no temporary file"
    );
}

#[test]
fn apply_refuses_a_provider_not_in_the_registry_and_writes_nothing() {
    let fx = Fx::new();
    fx.config(&preset("ext", "other-provider", "fast"));
    fx.registry(&json!([{"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["fast-o", "fast-s", "fast-h", "fast-g"]}]));
    fx.host(HOST);

    refused(
        &fx.cpd(&["preset", "apply", "ext"]),
        "preset_provider_unconfigured",
        3,
    );
    assert_eq!(
        fx.host_text(),
        HOST,
        "host.toml is untouched by the refusal"
    );
    assert!(fx.env.model_changed().is_empty(), "no Event is appended");
}

#[test]
fn apply_refuses_a_model_id_not_offered_and_writes_nothing() {
    let fx = Fx::new();
    // The preset's letters differ from the registry's offered ids.
    fx.config(&preset("ext", "github-copilot", "ghost"));
    fx.registry(&json!([{"provider": "github-copilot", "auth": "oauth", "base": null, "ids": ["real-o", "real-s", "real-h", "real-g"]}]));
    fx.host(HOST);

    refused(
        &fx.cpd(&["preset", "apply", "ext"]),
        "preset_model_unoffered",
        3,
    );
    assert_eq!(
        fx.host_text(),
        HOST,
        "host.toml is untouched by the refusal"
    );
    assert!(fx.env.model_changed().is_empty(), "no Event is appended");
}

#[test]
fn apply_of_an_unknown_preset_refuses_preset_unknown() {
    let fx = Fx::new();
    fx.config(FAST);
    fx.host(HOST);

    refused(&fx.cpd(&["preset", "apply", "nope"]), "preset_unknown", 4);
    assert_eq!(
        fx.host_text(),
        HOST,
        "host.toml is untouched by the refusal"
    );
    assert!(fx.env.model_changed().is_empty());
}

// ---------- idempotence and the live-Session events ----------

#[test]
fn apply_same_preset_twice_is_a_no_op_with_no_event() {
    let fx = Fx::new();
    fx.config(FAST);
    fx.host(HOST);
    fx.sessions();

    fx.cpd(&["preset", "apply", "fast"]).ok();
    assert_eq!(
        fx.env.model_changed().len(),
        1,
        "the first apply changed the Lead's letter"
    );
    let host_after_first = fx.host_text();

    let out = fx.cpd(&["preset", "apply", "fast"]).ok();
    assert_eq!(out["already"], true, "{out}");
    assert_eq!(out["changed"], json!([]), "{out}");
    assert_eq!(
        fx.host_text(),
        host_after_first,
        "host.toml bytes are unchanged by the no-op"
    );
    assert_eq!(
        fx.env.model_changed().len(),
        1,
        "the no-op appends no Event"
    );
}

#[test]
fn apply_emits_one_session_model_changed_per_changed_live_session() {
    let fx = Fx::new();
    fx.config(FAST);
    fx.host(HOST);
    fx.sessions();

    let out = fx.cpd(&["preset", "apply", "fast"]).ok();
    assert_eq!(out["already"], false, "{out}");
    let changed = fx.env.model_changed();
    assert_eq!(
        changed.len(),
        1,
        "exactly one Session's letter changed: {changed:?}"
    );
    // The Lead is on the O letter, which the apply changed; the Manager is on the unchanged S letter.
    assert_eq!(changed[0]["session"], "lead1", "{changed:?}");
    assert_eq!(
        changed[0]["data"]["from"], "github-copilot/claude-opus-5.5",
        "{changed:?}"
    );
    assert_eq!(
        changed[0]["data"]["to"], "github-copilot/claude-haiku-4.5",
        "{changed:?}"
    );
}

// ---------- list and show ----------

#[test]
fn list_and_show_print_the_presets_and_the_current_model() {
    let fx = Fx::new();
    fx.config(FAST);
    fx.host(HOST);

    let out = fx.cpd(&["preset", "list"]).ok();
    assert_eq!(
        out["presets"],
        json!([{"name": "fast", "provider": "github-copilot"}]),
        "{out}"
    );

    let out = fx.cpd(&["preset", "show", "fast"]).ok();
    assert_eq!(out["name"], "fast", "{out}");
    assert_eq!(out["O"], "github-copilot/claude-haiku-4.5", "{out}");
    assert_eq!(out["S"], "github-copilot/claude-sonnet-5.5", "{out}");
    assert_eq!(out["H"], "github-copilot/claude-haiku-4.5", "{out}");
    assert_eq!(out["G"], "github-copilot/gemini-3.8-flash", "{out}");
    assert_eq!(
        out["host"]["O"], "github-copilot/claude-opus-5.5",
        "{out}: the current host [model]"
    );
    assert_eq!(
        out["host"]["S"], "github-copilot/claude-sonnet-5.5",
        "{out}"
    );
    assert_eq!(out["host"]["H"], "github-copilot/claude-haiku-4.5", "{out}");
    assert_eq!(out["host"]["G"], "github-copilot/gemini-3.8-flash", "{out}");

    // A config with no presets lists none.
    let fx = Fx::new();
    fx.config("");
    assert_eq!(fx.cpd(&["preset", "list"]).ok()["presets"], json!([]));
}
