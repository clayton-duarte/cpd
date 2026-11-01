//! The `[-e …]` rule (contract §6.5): which of cpd's extension sources pi's
//! settings already declare, compared by package identity the way pi compares
//! packages.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::dirs::expand_tilde;
use super::{PiDirs, project_settings};

/// A package's identity, the way pi compares packages (pi `packages.md`,
/// "Understand scope and identity"; pi's `getPackageIdentity`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageId {
    /// An npm spec's package name, without the version.
    Npm(String),
    /// A git source's `<host>/<path>`, without the ref or `.git`.
    Git(String),
    /// A local path, resolved to an absolute path.
    Local(PathBuf),
}

/// The identity of `source`; a relative local path resolves from `base_dir`.
pub fn package_identity(source: &str, base_dir: &Path, home: &Path) -> PackageId {
    let source = source.trim();
    if let Some(spec) = source.strip_prefix("npm:") {
        return PackageId::Npm(npm_name(spec.trim()));
    }
    let non_local = ["git:", "github:", "http:", "https:", "ssh:", "builtin:"];
    if non_local.iter().any(|p| source.starts_with(p))
        && let Some(git) = git_identity(source)
    {
        return PackageId::Git(git);
    }
    PackageId::Local(resolve_path(source, base_dir, home))
}

/// pi's `parseNpmSpec`: `(@scope/)name` before the version's `@`.
fn npm_name(spec: &str) -> String {
    let (scope, rest) = match spec.strip_prefix('@') {
        Some(rest) => ("@", rest),
        None => ("", spec),
    };
    match rest.split_once('@') {
        Some((name, _)) if !name.is_empty() => format!("{scope}{name}"),
        _ => spec.to_owned(),
    }
}

/// pi's `parseGitUrl` reduced to the identity `<host>/<path>`: `git:` sources
/// in any shorthand (`git:host/user/repo@ref`, `git:git@host:user/repo`), and
/// `http(s)://`, `ssh://`, `git://` URLs. None when it is not a git source.
fn git_identity(source: &str) -> Option<String> {
    let lower = source.to_ascii_lowercase();
    let url = if ["https://", "http://", "ssh://", "git://"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        source
    } else {
        source.strip_prefix("git:")?.trim()
    };
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split_once(':').map_or(host, |(h, _)| h);
        (host.to_ascii_lowercase(), path)
    } else if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        (host.to_owned(), path)
    } else {
        let (host, path) = url.split_once('/')?;
        if !host.contains('.') && host != "localhost" {
            return None;
        }
        (host.to_owned(), path)
    };
    let path = path.split_once('@').map_or(path, |(repo, _)| repo);
    let path = path.trim_start_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    let unsafe_part = |s: &str| s.contains('\\') || s.contains('\0');
    if host.is_empty()
        || segments.len() < 2
        || segments.contains(&"..")
        || unsafe_part(&host)
        || unsafe_part(path)
    {
        return None;
    }
    Some(format!("{host}/{path}"))
}

/// pi's `resolvePath`: `~` and `file://` expanded, then an absolute path with
/// `.` and `..` removed lexically (no symlinks followed).
fn resolve_path(input: &str, base_dir: &Path, home: &Path) -> PathBuf {
    let input = input.strip_prefix("file://").unwrap_or(input);
    let path = expand_tilde(input, home);
    normalize(&base_dir.join(path))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// One package a settings file declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared {
    pub settings: PathBuf,
    pub source: String,
    pub id: PackageId,
}

/// The packages a settings file declares, each with its identity (relative
/// paths resolve from the file's directory). A missing file declares none; so
/// does an unreadable or invalid one, because pi then reads it as empty too
/// (its `tryLoadFromStorage`), which is logged.
pub fn declared(settings: &Path, home: &Path, log: &mut dyn Write) -> Vec<Declared> {
    let text = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            note(
                log,
                &format!(
                    "cannot read pi settings {}: {e}; treating it as declaring no packages",
                    settings.display()
                ),
            );
            return Vec::new();
        }
    };
    let value: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(e) => {
            note(
                log,
                &format!(
                    "pi settings {} are not valid JSON ({e}); treating it as declaring no packages",
                    settings.display()
                ),
            );
            return Vec::new();
        }
    };
    let base_dir = settings.parent().expect("a settings file has a directory");
    let Some(Value::Array(packages)) = value.get("packages") else {
        return Vec::new();
    };
    packages
        .iter()
        .filter_map(|entry| match entry {
            Value::String(s) => Some(s.as_str()),
            Value::Object(o) => o.get("source").and_then(Value::as_str),
            _ => None,
        })
        .map(|source| Declared {
            settings: settings.to_path_buf(),
            source: source.to_owned(),
            id: package_identity(source, base_dir, home),
        })
        .collect()
}

/// Writes one line to the log. A log that cannot be written is not worth
/// failing a launch for, so a write error is ignored here.
fn note(log: &mut dyn Write, line: &str) {
    let _ = writeln!(log, "cpd: {line}");
}

/// The npm package cpd's adapter replaced with its own copy (`pi/ask-user-question/`).
const RETIRED_NPM_PACKAGE: &str = "@juicesharp/rpiv-ask-user-question";

/// The `[-e …]` rule (contract §6.5): of `candidates`, the ones pi's global
/// settings (`<agent dir>/settings.json`) and project settings
/// (`<cwd>/.pi/settings.json`) do not already declare, compared by package
/// identity. Candidates resolve from `cwd`, as pi resolves `-e`. Each skipped
/// candidate is logged to `log` with the settings file that declares it.
/// A candidate that is the retired package (`RETIRED_NPM_PACKAGE`, whatever
/// the version) is dropped first, with one log line, before the settings rule.
pub fn extensions_to_pass(
    candidates: &[String],
    cwd: &Path,
    dirs: &PiDirs,
    log: &mut dyn Write,
) -> Vec<String> {
    let mut declared_packages = declared(&dirs.global_settings(), &dirs.home, log);
    declared_packages.extend(declared(&project_settings(cwd), &dirs.home, log));
    candidates
        .iter()
        .filter(|candidate| {
            let id = package_identity(candidate, cwd, &dirs.home);
            if id == PackageId::Npm(RETIRED_NPM_PACKAGE.to_owned()) {
                note(
                    log,
                    &format!(
                        "not passing -e {candidate}: cpd's adapter carries its own copy of it"
                    ),
                );
                return false;
            }
            match declared_packages.iter().find(|d| d.id == id) {
                Some(d) => {
                    note(
                        log,
                        &format!(
                            "not passing -e {candidate}: {} already declares it as {:?}",
                            d.settings.display(),
                            d.source
                        ),
                    );
                    false
                }
                None => true,
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn identities() {
        let base = Path::new("/base/dir");
        let home = Path::new("/home/u");
        let id = |s: &str| package_identity(s, base, home);
        let npm = |s: &str| PackageId::Npm(s.into());
        let git = |s: &str| PackageId::Git(s.into());
        let local = |s: &str| PackageId::Local(s.into());

        assert_eq!(id("npm:@example/ext@0.28.17"), npm("@example/ext"));
        assert_eq!(id("npm:@example/ext"), npm("@example/ext"));
        assert_eq!(id("npm:pi-tools@^1.2"), npm("pi-tools"));
        assert_eq!(id(" npm: pi-tools "), npm("pi-tools"));

        assert_eq!(
            id("git:github.com/example/pi-tools@v1"),
            git("github.com/example/pi-tools")
        );
        assert_eq!(
            id("git:github.com/example/pi-tools"),
            git("github.com/example/pi-tools")
        );
        assert_eq!(
            id("https://github.com/example/pi-tools"),
            git("github.com/example/pi-tools")
        );
        assert_eq!(
            id("https://github.com/example/pi-tools.git@abc123"),
            git("github.com/example/pi-tools")
        );
        assert_eq!(
            id("ssh://git@GitHub.com:22/example/pi-tools.git"),
            git("github.com/example/pi-tools")
        );
        assert_eq!(
            id("git:git@github.com:example/pi-tools@v2"),
            git("github.com/example/pi-tools")
        );

        assert_eq!(id("./pkg"), local("/base/dir/pkg"));
        assert_eq!(id("../other/./pkg/"), local("/base/other/pkg"));
        assert_eq!(id("/abs/pkg"), local("/abs/pkg"));
        assert_eq!(id("~/pkgs/p"), local("/home/u/pkgs/p"));
        assert_eq!(id("file:///abs/pkg"), local("/abs/pkg"));
        assert_eq!(id("pkg"), local("/base/dir/pkg"));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        dirs: PiDirs,
        cwd: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            let cwd = dir.path().join("repo");
            fs::create_dir_all(cwd.join(".pi")).unwrap();
            let dirs = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
            fs::create_dir_all(&dirs.agent_dir).unwrap();
            Fixture {
                _dir: dir,
                dirs,
                cwd,
            }
        }

        fn global(&self, json: &str) -> PathBuf {
            fs::write(self.dirs.global_settings(), json).unwrap();
            self.dirs.global_settings()
        }

        fn project(&self, json: &str) -> PathBuf {
            fs::write(project_settings(&self.cwd), json).unwrap();
            project_settings(&self.cwd)
        }

        fn pass(&self, candidates: &[String]) -> (Vec<String>, String) {
            let mut log = Vec::new();
            let kept = extensions_to_pass(candidates, &self.cwd, &self.dirs, &mut log);
            (kept, String::from_utf8(log).unwrap())
        }
    }

    #[test]
    fn no_settings_pass_every_candidate() {
        let f = Fixture::new();
        let candidates = strs(&["/opt/pi", "npm:@example/ext@0.28.17"]);
        assert_eq!(f.pass(&candidates), (candidates.clone(), String::new()));
    }

    #[test]
    fn skips_npm_specs_declared_globally_whatever_the_version() {
        let f = Fixture::new();
        let settings = f.global(
            r#"{"packages": ["npm:@example/ext@0.30.0",
                {"source": "npm:@example/other-tool", "skills": []}]}"#,
        );
        let candidates = strs(&[
            "/opt/pi",
            "npm:@example/ext@0.28.17",
            "npm:@example/other-tool@2.0.0",
            "npm:@other/pkg@1.0.0",
        ]);
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, strs(&["/opt/pi", "npm:@other/pkg@1.0.0"]));
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log}");
        assert!(lines[0].contains("-e npm:@example/ext@0.28.17"), "{log}");
        assert!(lines[0].contains(&settings.display().to_string()), "{log}");
        assert!(
            lines[1].contains("-e npm:@example/other-tool@2.0.0"),
            "{log}"
        );
    }

    #[test]
    fn skips_git_sources_declared_in_the_project_whatever_the_ref() {
        let f = Fixture::new();
        let settings = f.project(r#"{"packages": ["git:github.com/example/pi-tools@v1"]}"#);
        let candidates = strs(&[
            "https://github.com/example/pi-tools@v2",
            "git:github.com/example/other",
        ]);
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, strs(&["git:github.com/example/other"]));
        assert!(
            log.contains("-e https://github.com/example/pi-tools@v2"),
            "{log}"
        );
        assert!(log.contains(&settings.display().to_string()), "{log}");
    }

    #[test]
    fn local_paths_resolve_from_the_declaring_settings_file() {
        let f = Fixture::new();
        // The global file's relative path resolves from the agent dir, the
        // project's from `<cwd>/.pi`; candidates resolve from cwd.
        f.global(r#"{"packages": ["../../share/cpd/pi"]}"#);
        let adapter = f.dirs.home.join("share/cpd/pi");
        f.project(r#"{"packages": [{"source": "../tools/x"}]}"#);
        let candidates = vec![
            adapter.display().to_string(),
            "./tools/x".to_owned(),
            "./tools/y".to_owned(),
            f.dirs
                .agent_dir
                .join("../../share/cpd/other")
                .display()
                .to_string(),
        ];
        let (kept, log) = f.pass(&candidates);
        assert_eq!(kept, vec!["./tools/y".to_owned(), candidates[3].clone()]);
        assert_eq!(log.lines().count(), 2, "{log}");
        assert!(
            log.contains(&f.dirs.global_settings().display().to_string()),
            "{log}"
        );
        assert!(
            log.contains(&project_settings(&f.cwd).display().to_string()),
            "{log}"
        );
    }

    #[test]
    fn invalid_settings_declare_nothing_and_say_so() {
        let f = Fixture::new();
        f.global("{not json");
        f.project(r#"{"packages": "npm:x"}"#);
        let (kept, log) = f.pass(&strs(&["npm:x@1"]));
        assert_eq!(kept, strs(&["npm:x@1"]));
        assert!(log.contains("not valid JSON"), "{log}");
    }

    // The retired `@juicesharp/rpiv-ask-user-question` package (contract: the ask_user_question extension; the cpd
    // adapter carries its own copy).
    #[test]
    fn retired_package_is_not_passed_and_logged() {
        const SAMPLE: &str = "npm:@example/ext@0.28.17";
        const RETIRED: &str = "npm:@juicesharp/rpiv-ask-user-question@2.13.8";
        let root = tempfile::tempdir().unwrap();
        let dirs = PiDirs {
            home: root.path().join("home"),
            agent_dir: root.path().join("home/.pi/agent"),
            session_dir: None,
        };
        let cwd = root.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let candidates: Vec<String> = ["/opt/pi", SAMPLE, RETIRED, "npm:@other/pkg@1.0.0"]
            .map(str::to_owned)
            .to_vec();
        let mut log = Vec::new();
        let kept = extensions_to_pass(&candidates, &cwd, &dirs, &mut log);
        assert_eq!(kept, ["/opt/pi", SAMPLE, "npm:@other/pkg@1.0.0"]);
        assert_eq!(
            String::from_utf8(log).unwrap(),
            format!("cpd: not passing -e {RETIRED}: cpd's adapter carries its own copy of it\n")
        );
    }
}
