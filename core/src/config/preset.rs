//! `cpd preset list|show|apply` (commands.md §14.6; contract §8.2 "`[presets.<name>]`"): the tier-preset switch
//! (Plan `tier-presets`, D1–D5). `list` and `show` are read-only: `list` prints every preset with the provider of its
//! `O` ref, `show` prints a preset's four refs and the current machine `[model]`. `apply` validates a preset against
//! pi's full registry (the model-discovery helper's `--all` answer, listed fresh at apply time, never the model cache)
//! and, when its four refs differ from the machine host policy's `[model]`, rewrites ONLY that table of
//! `$CPD_HOME/policy/host.toml` byte-for-byte around it, atomically, then appends ONE `session-model-changed` Event
//! per live Lead or Manager whose letter's model changed. Every refusal happens before any write, and the no-op
//! (the host already on the preset) opens no store and writes nothing.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::file::Config;
use crate::atomic::write_atomic;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::models::{Answer, Entry, Helper, Listed};
use crate::output::Output;
use crate::policy::model::{Models, check_one_provider, models_of_context, provider_of};
use crate::policy::{policy_dir, policy_invalid, resolve_machine};
use crate::session::row;

/// The `preset` commands (commands.md §14.6).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Print the presets (§14.6)
    List,
    /// Print a preset and the current host model (§14.6)
    Show(ShowArgs),
    /// Switch the machine to a preset's models (§14.6)
    Apply(ApplyArgs),
    /// Switch the machine to four model refs given as flags (§14.6a)
    Set(SetArgs),
}

#[derive(clap::Args, Debug)]
pub struct SetArgs {
    /// The model ref of the O letter
    #[arg(long = "O", value_name = "ref", value_parser = crate::policy::model::model_ref)]
    o: String,
    /// The model ref of the S letter
    #[arg(long = "S", value_name = "ref", value_parser = crate::policy::model::model_ref)]
    s: String,
    /// The model ref of the H letter
    #[arg(long = "H", value_name = "ref", value_parser = crate::policy::model::model_ref)]
    h: String,
    /// The model ref of the G letter
    #[arg(long = "G", value_name = "ref", value_parser = crate::policy::model::model_ref)]
    g: String,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The preset's name
    #[arg(value_name = "name")]
    name: String,
}

#[derive(clap::Args, Debug)]
pub struct ApplyArgs {
    /// The preset's name
    #[arg(value_name = "name")]
    name: String,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List => list(),
        Command::Show(args) => show(args),
        Command::Apply(args) => apply(args),
        Command::Set(args) => set(args),
    }
}

fn preset_unknown(name: &str) -> CpdError {
    CpdError::new(
        ErrorCode::PresetUnknown,
        format!("No preset {name}: config.toml has no `[presets.{name}]` table."),
    )
    .with("preset", name)
}

/// `preset list` (commands.md §14.6): every preset with the provider of its `O` ref, sorted by name. One read of
/// config.toml, no store.
fn list() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let presets: Vec<Value> = config
        .presets
        .iter()
        .map(|(name, preset)| json!({"name": name, "provider": provider_of(&preset.o)}))
        .collect();
    Ok(Output::success(json!({"presets": presets})))
}

/// `preset show` (commands.md §14.6): the preset's four refs and the current host `[model]` (the machine scope's,
/// or the built-in default when host.toml has no `[model]`). No registry is read and nothing is validated.
fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let preset = config
        .presets
        .get(&args.name)
        .ok_or_else(|| preset_unknown(&args.name))?;
    let host = resolve_machine(&home)?.policy.model;
    Ok(Output::success(json!({
        "name": args.name,
        "O": preset.o, "S": preset.s, "H": preset.h, "G": preset.g,
        "host": {"O": host.o, "S": host.s, "H": host.h, "G": host.g},
    })))
}

/// `preset apply` (commands.md §14.6). Steps in order; every refusal happens before any write and appends no Event.
fn apply(args: ApplyArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let name = args.name;
    // Step 1: the name.
    let preset = config
        .presets
        .get(&name)
        .ok_or_else(|| preset_unknown(&name))?;
    let preset_models = Models {
        o: preset.o.clone(),
        s: preset.s.clone(),
        h: preset.h.clone(),
        g: preset.g.clone(),
        // Placeholder: Job tier-j-impl-core carries the fifth `J` ref through presets too.
        j: String::new(),
    };
    write_host(&home, &config, &preset_models, "preset apply", Some(&name))
}

/// `preset set` (commands.md §14.6a): steps 2–7 of `apply` with the four refs of the flags.
fn set(args: SetArgs) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let models = Models {
        o: args.o,
        s: args.s,
        h: args.h,
        g: args.g,
        // Placeholder: Job tier-j-impl-core adds the fifth `J` flag.
        j: String::new(),
    };
    write_host(&home, &config, &models, "preset set", None)
}

/// Steps 2–7 of `preset apply`, shared with `preset set`: `name` is the preset's, `None` for `set`.
fn write_host(
    home: &CpdHome,
    config: &Config,
    preset_models: &Models,
    command: &str,
    name: Option<&str>,
) -> Result<Output, CpdError> {
    let label = name.map_or_else(|| "preset set".to_owned(), |n| format!("preset {n}"));
    // Step 2: host.toml must exist and hold a `[model]` table: there is nothing to rewrite without one.
    let host_path = policy_dir(home).join("host.toml");
    let host_text = match std::fs::read_to_string(&host_path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(policy_invalid(
                &host_path,
                Some("model"),
                "host.toml is missing: there is no `[model]` table to rewrite.",
            ));
        }
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::FileUnreadable,
                format!("Cannot read {}: {e}.", host_path.display()),
            )
            .with("file", host_path.display().to_string()));
        }
    };
    let table: toml::Table = host_text.parse().map_err(|e: toml::de::Error| {
        policy_invalid(
            &host_path,
            Some("model"),
            format!("not valid TOML ({})", e.message()),
        )
    })?;
    if !matches!(table.get("model"), Some(toml::Value::Table(_))) {
        return Err(policy_invalid(
            &host_path,
            Some("model"),
            "host.toml has no `[model]` table: there is nothing to rewrite.",
        ));
    }

    // Step 3: equality. The no-op opens no store and writes nothing. A preset carries the four model letters
    // `O`, `S`, `H`, `G`; the fifth, `J`, is the decide role's letter (contract §9 "Roles and ranges") and is
    // kept as the host already resolves it, so a preset never moves the decide letter and the four written
    // lines stay one provider with it.
    let pre = resolve_machine(home)?.policy.model;
    let mut preset = preset_models.clone();
    if preset.j.is_empty() {
        preset.j = pre.j.clone();
    }
    if preset == pre {
        return Ok(Output::success(
            json!({"preset": name, "already": true, "changed": []}),
        ));
    }

    // Step 4: the one-provider invariant of the table to be written, before anything external runs.
    check_one_provider(&preset, &host_path)?;

    // Step 5: the registry, listed at apply time (`--all`), through the host runner.
    let answer = Helper::new(config, home)?.all().run()?;
    check_registry(&label, name, &preset, &answer)?;

    // Steps 6–7: the byte-preserving rewrite, then one Event per changed live Lead or Manager, all in one
    // store transaction. The pre-write models are read before the file is replaced.
    let new_text = rewrite_model(&host_text, &preset);
    let mut env = Env::open()?;
    let pre_models: Vec<(row::ModelSwitchRow, String)> = env.store.read(|conn| {
        row::live_model_switch(conn)?
            .into_iter()
            .map(|r| {
                let models = models_of_context(conn, home, r.context.as_str())?;
                let (model, _) = row::model_and_level(&models, &r.id, &r.tier)?;
                Ok((r, model))
            })
            .collect()
    })?;
    write_atomic(&host_path, &new_text).map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("cannot write {}: {e}.", host_path.display()),
        )
        .with("file", host_path.display().to_string())
    })?;
    let changed: Vec<Value> = env.store.write(command, |tx| {
        let mut changed = Vec::new();
        for row in row::live_model_switch(tx)? {
            let Some((_, from)) = pre_models.iter().find(|(r, _)| r.id == row.id) else {
                continue;
            };
            let models = models_of_context(tx, home, row.context.as_str())?;
            let (to, _) = row::model_and_level(&models, &row.id, &row.tier)?;
            if *from == to {
                continue;
            }
            let text = format!("Session {} model moved from {} to {}.", row.id, from, to);
            let mut event = crate::events::NewEvent::session_model_changed(text, from, &to)
                .session(row.id.clone())
                .context(row.context.clone());
            if let Some(plan) = &row.plan {
                event = event.plan(plan.clone());
            }
            let id = crate::events::append(tx, event)?;
            changed.push(json!({"session": row.id, "role": row.role.as_str(), "from": from, "to": to, "event": id}));
        }
        Ok(changed)
    })?;
    Ok(Output::success(
        json!({"preset": name, "host": host_path.display().to_string(), "already": false, "changed": changed}),
    ))
}

/// The registry's listed providers: the `Entry::Listed` ones by provider name. A provider whose listing failed is
/// not in the registry (a preset naming it cannot be validated).
fn listed_of(answer: &Answer) -> BTreeMap<&str, &Listed> {
    answer
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Listed(l) => Some((l.provider.as_str(), l)),
            Entry::Failed(_) => None,
        })
        .collect()
}

/// Step 5 of `preset apply` (commands.md §14.6): the preset's provider is in the registry, and every ref is
/// `<provider>/<an offered id>`; else `preset_provider_unconfigured` / `preset_model_unoffered`, writing nothing.
fn check_registry(
    label: &str,
    name: Option<&str>,
    preset: &Models,
    answer: &Answer,
) -> Result<(), CpdError> {
    let listed = listed_of(answer);
    for (letter, model_ref) in [
        ("O", &preset.o),
        ("S", &preset.s),
        ("H", &preset.h),
        ("G", &preset.g),
    ] {
        let provider = provider_of(model_ref);
        let entry = listed.get(provider).ok_or_else(|| {
            CpdError::new(
                ErrorCode::PresetProviderUnconfigured,
                format!("{label}: {letter} is {model_ref}, but provider {provider:?} is not in pi's registry on this machine."),
            )
            .with("preset", name)
            .with("letter", letter)
            .with("model", model_ref.as_str())
            .with("provider", provider)
        })?;
        let id = model_ref
            .strip_prefix(provider)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(model_ref);
        if !entry.ids.iter().any(|offered| offered == id) {
            return Err(CpdError::new(
                ErrorCode::PresetModelUnoffered,
                format!("{label}: {letter} is {model_ref}, but provider {provider:?} does not offer model {id:?}."),
            )
            .with("preset", name)
            .with("letter", letter)
            .with("model", model_ref.as_str())
            .with("provider", provider));
        }
    }
    Ok(())
}

/// Step 6 of `preset apply` (commands.md §14.6): the byte-preserving rewrite of host.toml's `[model]` table. The
/// section header `[model]` stays; its `O`/`S`/`H`/`G` key lines are replaced by the preset's four refs, in that
/// order, and every other byte of the file is unchanged. The caller has parsed the file and found the table.
fn rewrite_model(host_text: &str, preset: &Models) -> String {
    let mut lines: Vec<String> = host_text.split_inclusive('\n').map(str::to_owned).collect();
    let header = lines
        .iter()
        .position(|l| l.trim() == "[model]")
        .expect("the caller found the [model] table");
    let next = lines[header + 1..]
        .iter()
        .position(|l| l.starts_with('['))
        .map(|i| header + 1 + i)
        .unwrap_or(lines.len());
    let letters: [(&str, &String); 4] = [
        ("O", &preset.o),
        ("S", &preset.s),
        ("H", &preset.h),
        ("G", &preset.g),
    ];
    let is_key = |line: &str, letter: &str| {
        let bare = line.trim().trim_end_matches(['\n', '\r']);
        bare.strip_prefix(letter)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let mut missing: Vec<(&str, &String)> = Vec::new();
    for (letter, value) in letters {
        match (header + 1..next).find(|i| is_key(&lines[*i], letter)) {
            Some(i) => lines[i] = format!("{letter} = {value:?}\n"),
            None => missing.push((letter, value)),
        }
    }
    // A letter the table does not set is added at the section's end, before the next header (or EOF).
    for (letter, value) in missing {
        lines.insert(next, format!("{letter} = {value:?}\n"));
    }
    lines.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_replaces_only_the_model_table() {
        let host = "# not part of the model\n[model]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\nG = \"p/gemini\"\n\n[pr]\nfeedback.checks = \"none\"\n";
        let preset = Models {
            o: "p/haiku".into(),
            s: "p/sonnet".into(),
            h: "p/haiku".into(),
            g: "p/gemini".into(),
            j: String::new(),
        };
        let after = rewrite_model(host, &preset);
        assert_eq!(
            after,
            "# not part of the model\n[model]\nO = \"p/haiku\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\nG = \"p/gemini\"\n\n[pr]\nfeedback.checks = \"none\"\n"
        );
    }

    #[test]
    fn rewrite_keeps_surrounding_bytes_and_last_section() {
        let host =
            "# c\n[model]\nG = \"p/gemini\"\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n";
        let preset = Models {
            o: "p/opus".into(),
            s: "p/sonnet".into(),
            h: "p/haiku".into(),
            g: "p/gemini".into(),
            j: String::new(),
        };
        // The letters keep their places; the trailing newline survives.
        assert_eq!(
            rewrite_model(host, &preset),
            "# c\n[model]\nG = \"p/gemini\"\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n"
        );
    }
}
