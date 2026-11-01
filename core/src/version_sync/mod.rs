//! `cpd version sync [--print]` (docs/commands.md §26.2) and the migration file the sync follows. Files only (std `fs`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::ports::Ports;

/// The crates of the workspace, whose `Cargo.lock` stanzas carry the version.
const WORKSPACE_CRATES: [&str; 4] = ["cpd", "cpd-core", "cpd-engine", "cpd-tui"];

fn fail<T>(why: String) -> Result<T, String> {
    Err(format!("sync-version: {why}"))
}

/// `0.<PROTOCOL>.<migrations>` of the repository at `root`.
fn derive(root: &Path) -> Result<String, String> {
    let kind = root.join("core/src/events/kind.rs");
    let dir = root.join("core/src/store/migrations");
    let protocol = fs::read_to_string(&kind).ok().and_then(|text| {
        text.lines().find_map(|l| {
            let digits = l.strip_prefix("pub const PROTOCOL: u32 = ")?;
            let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
            (!n.is_empty() && digits[n.len()..].starts_with(';')).then_some(n)
        })
    });
    let Some(protocol) = protocol else {
        return fail(format!(
            "no 'pub const PROTOCOL: u32 = <n>;' line in {}.",
            kind.display()
        ));
    };
    let n = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "sql"))
                .count()
        })
        .unwrap_or(0);
    if n == 0 {
        return fail(format!("no .sql file in {}.", dir.display()));
    }
    Ok(format!("0.{protocol}.{n}"))
}

/// `line` with its `version = "..."` replaced, keeping its line ending.
fn with_version(line: &str, version: &str) -> String {
    let ending = &line[line.trim_end_matches(['\n', '\r']).len()..];
    format!("version = \"{version}\"{ending}")
}

/// Cargo.toml with the first `version = "` under `[workspace.package]` replaced; `None` when there is none.
fn rewrite_toml(text: &str, version: &str) -> Option<String> {
    let (mut inside, mut done) = (false, false);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare.starts_with('[') {
            inside = bare == "[workspace.package]";
        }
        if inside && !done && line.starts_with("version = \"") {
            out.push_str(&with_version(line, version));
            done = true;
        } else {
            out.push_str(line);
        }
    }
    done.then_some(out)
}

/// Cargo.lock with the `version` of each workspace crate's stanza (one with no `source`) replaced.
fn rewrite_lock(text: &str, version: &str) -> String {
    fn flush(out: &mut String, stanza: &mut Vec<&str>, version: &str) {
        let bare = |l: &str| l.trim_end_matches(['\n', '\r']).to_owned();
        let ours = stanza.iter().any(|l| {
            WORKSPACE_CRATES
                .iter()
                .any(|c| bare(l) == format!("name = \"{c}\""))
        });
        let sourced = stanza.iter().any(|l| l.starts_with("source = "));
        for line in stanza.drain(..) {
            if ours && !sourced && line.starts_with("version = \"") {
                out.push_str(&with_version(line, version));
            } else {
                out.push_str(line);
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut stanza = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with("[[package]]") {
            flush(&mut out, &mut stanza, version);
        }
        stanza.push(line);
    }
    flush(&mut out, &mut stanza, version);
    out
}

/// `0.<PROTOCOL>.<migrations>` of the repository at `root`; with `print` nothing is written. `Ok` is the stdout line
/// without its newline: the version for `print`, else `sync-version: <version>`. `Err` is the one `sync-version: ...`
/// line of a failure (no newline), and nothing is written then.
pub fn sync(root: &Path, print: bool) -> Result<String, String> {
    let version = derive(root)?;
    if print {
        return Ok(version);
    }
    let (toml_path, lock_path) = (root.join("Cargo.toml"), root.join("Cargo.lock"));
    let toml = fs::read_to_string(&toml_path).unwrap_or_default();
    if !toml.lines().any(|l| l.starts_with("[workspace.package]")) {
        return fail(format!(
            "no [workspace.package] in {}.",
            toml_path.display()
        ));
    }
    let Some(new_toml) = rewrite_toml(&toml, &version) else {
        return fail(format!(
            "no version line under [workspace.package] in {}.",
            toml_path.display()
        ));
    };
    let lock = fs::read_to_string(&lock_path)
        .map_err(|e| format!("sync-version: cannot read {}: {e}.", lock_path.display()))?;
    let new_lock = rewrite_lock(&lock, &version);
    fs::write(&toml_path, new_toml)
        .map_err(|e| format!("sync-version: cannot write {}: {e}.", toml_path.display()))?;
    fs::write(&lock_path, new_lock)
        .map_err(|e| format!("sync-version: cannot write {}: {e}.", lock_path.display()))?;
    Ok(format!("sync-version: {version}"))
}

/// Creates `core/src/store/migrations/<YYYYMMDDTHHMM>-<slug>.sql` (the UTC minute of `ports.clock`) holding
/// `-- cpd-migration: additive`, runs [`sync`] and returns the file's path.
pub fn new_migration(ports: &Ports, root: &Path, slug: &str) -> Result<PathBuf, String> {
    let dir = root.join("core/src/store/migrations");
    if !dir.is_dir() {
        return Err(format!("new-migration: {} does not exist.", dir.display()));
    }
    let t = ports.clock.utc();
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    );
    let file = dir.join(format!("{stamp}-{slug}.sql"));
    if file.exists() {
        return Err(format!("new-migration: {} already exists.", file.display()));
    }
    fs::write(&file, "-- cpd-migration: additive\n")
        .map_err(|e| format!("new-migration: cannot write {}: {e}.", file.display()))?;
    sync(root, false)?;
    Ok(file)
}

#[cfg(test)]
mod tests;
