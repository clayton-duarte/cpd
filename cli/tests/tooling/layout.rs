//! The per-area layout of `cli/tests/` (docs/research/workspace-split.md §4.2; docs/implementing.md, Tests): one test
//! binary per area, `cli/tests/<area>/main.rs`, whose `mod` lines are the area table. `common/` is the shared harness.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// The `.rs` files under `dir`, as paths relative to `base`, without those under a `common/` directory.
fn rust_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "common") {
                continue;
            }
            rust_files(base, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path.strip_prefix(base).unwrap().to_owned());
        }
    }
}

/// The modules a `main.rs` declares: its `mod x;` lines, each with the `#[path]` attribute on the line before, if any.
fn declared(main: &Path) -> Vec<(String, Option<String>)> {
    let text = fs::read_to_string(main).unwrap();
    let mut out = Vec::new();
    let mut path_attr: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#[path = \"") {
            path_attr = rest.strip_suffix("\"]").map(str::to_owned);
        } else if let Some(name) = line.strip_prefix("mod ").and_then(|r| r.strip_suffix(';')) {
            out.push((name.trim().to_owned(), path_attr.take()));
        } else if !line.starts_with("//") && !line.is_empty() {
            path_attr = None;
        }
    }
    out
}

/// The binaries: each `cli/tests/<area>/main.rs`, by area name.
fn binaries() -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(tests_dir()).unwrap() {
        let dir = entry.unwrap().path();
        if dir.join("main.rs").is_file() {
            out.insert(dir.file_name().unwrap().to_string_lossy().into_owned(), dir.join("main.rs"));
        }
    }
    out
}

#[test]
fn every_test_file_is_a_module_of_exactly_one_area_main() {
    let tests = tests_dir();
    let mut files = Vec::new();
    rust_files(&tests, &tests, &mut files);
    let mut problems = Vec::new();

    // Every main.rs is an area binary: it shares the harness at `cli/tests/common/` by path.
    if !tests.join("common/mod.rs").is_file() {
        problems.push("cli/tests/common/mod.rs (the shared harness) does not exist".to_owned());
    }
    let binaries = binaries();
    let mut declared_in: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (area, main) in &binaries {
        let mods = declared(main);
        if !mods.iter().any(|(n, p)| n == "common" && p.as_deref() == Some("../common/mod.rs")) {
            problems.push(format!("{area}/main.rs lacks `#[path = \"../common/mod.rs\"] mod common;`"));
        }
        for (name, path) in mods.into_iter().filter(|(n, _)| n != "common") {
            if path.is_some() {
                problems.push(format!("{area}/main.rs declares `mod {name};` with a #[path]: a module is a file of its own area"));
            }
            declared_in.entry(format!("{area}/{name}.rs")).or_default().push(area.clone());
        }
    }

    // Every file but `common/` and the `main.rs` files is declared by the main.rs of its own directory, once.
    for file in &files {
        let rel = file.to_string_lossy().into_owned();
        let components: Vec<_> = file.components().collect();
        if file.file_name().is_some_and(|n| n == "main.rs") && components.len() == 2 {
            continue;
        }
        if components.len() != 2 {
            problems.push(format!("{rel}: a test file is `cli/tests/<area>/<module>.rs`, directly under its area"));
            continue;
        }
        match declared_in.remove(&rel) {
            Some(areas) if areas.len() == 1 => {}
            Some(areas) => problems.push(format!("{rel}: declared {} times", areas.len())),
            None => problems.push(format!("{rel}: not declared by its area's main.rs")),
        }
    }
    for (declared_file, _) in declared_in {
        problems.push(format!("{declared_file}: declared by a main.rs but the file does not exist"));
    }
    // No main.rs of an area that is not under cli/tests/<area>/, and no file outside any binary.
    assert!(!binaries.is_empty(), "cli/tests/<area>/main.rs: no area binary exists");
    assert!(problems.is_empty(), "the per-area layout is broken:\n{}", problems.join("\n"));
}

/// The area table of docs/implementing.md, Tests: binary name to the patterns of its modules.
fn area_table() -> BTreeMap<String, Vec<String>> {
    let doc = fs::read_to_string(root().join("docs/implementing.md")).unwrap();
    let mut lines = doc.lines().skip_while(|l| !l.contains("The area table"));
    let mut table = BTreeMap::new();
    for line in lines.by_ref().skip(1).skip_while(|l| !l.trim_start().starts_with('|')) {
        let line = line.trim();
        if !line.starts_with('|') {
            break;
        }
        let cells: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 2 || !cells[0].starts_with('`') || cells[0].contains('<') {
            continue; // the header and the separator
        }
        let area = cells[0].trim_matches('`').to_owned();
        let patterns = cells[1].split(',').map(|p| p.trim().trim_matches('`').to_owned()).collect();
        table.insert(area, patterns);
    }
    table
}

/// `x*` is `x` and `x_<anything>`; `*_x` is a name ending in `_x`; anything else is the name itself.
fn matches(pattern: &str, module: &str) -> bool {
    if let Some(stem) = pattern.strip_suffix('*') {
        module == stem || module.starts_with(&format!("{stem}_"))
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        module.ends_with(suffix)
    } else {
        module == pattern
    }
}

#[test]
fn the_binaries_are_those_of_the_area_table() {
    let table = area_table();
    assert_eq!(table.len(), 7, "docs/implementing.md's area table: engine, pr, flow, session, store, tui, tooling: {table:?}");

    let binaries = binaries();
    assert_eq!(
        binaries.keys().collect::<Vec<_>>(),
        table.keys().collect::<Vec<_>>(),
        "the directories `cli/tests/<area>/main.rs` are the table's binaries, no more, no fewer"
    );

    // Each module is in the binary whose row matches it, and in only that one.
    let mut problems = Vec::new();
    for (area, main) in &binaries {
        let modules: Vec<_> = declared(main).into_iter().map(|(n, _)| n).filter(|n| n != "common").collect();
        for module in &modules {
            let rows: Vec<_> = table.iter().filter(|(_, ps)| ps.iter().any(|p| matches(p, module))).map(|(a, _)| a.as_str()).collect();
            if rows != [area.as_str()] {
                problems.push(format!("{area}/{module}: the table gives {rows:?}"));
            }
        }
        // Every row's pattern matches something in its own binary: the table does not describe a binary that is not there.
        for pattern in &table[area] {
            if !modules.iter().any(|m| matches(pattern, m)) {
                problems.push(format!("{area}: the pattern `{pattern}` matches no module of the binary"));
            }
        }
    }
    assert!(problems.is_empty(), "the modules do not follow the area table:\n{}", problems.join("\n"));
}

/// A Cargo.toml read as text: the repo's own manifests and `Cargo.lock` are the subject, not cargo's resolver, so no
/// cargo runs here (`cargo metadata` and `cargo tree` are cargo's, not ours).
fn read_toml(path: &Path) -> toml::Table {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.parse().unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The `[dependencies]`-like tables of a manifest: `(kind, table)` for the plain ones and those under `[target.*]`.
/// `kind` is `dependencies`, `dev-dependencies` or `build-dependencies`.
fn dependency_tables(manifest: &toml::Table) -> Vec<(&'static str, &toml::Table)> {
    let mut tables = Vec::new();
    let scopes: Vec<&toml::Table> = std::iter::once(manifest)
        .chain(manifest.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|t| t.values().filter_map(toml::Value::as_table)))
        .collect();
    for scope in scopes {
        for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(table) = scope.get(kind).and_then(toml::Value::as_table) {
                tables.push((kind, table));
            }
        }
    }
    tables
}

/// The package a dependency entry `key = spec` names: `package = "x"` renames it.
fn dependency_package<'a>(key: &'a str, spec: &'a toml::Value) -> &'a str {
    spec.get("package").and_then(toml::Value::as_str).unwrap_or(key)
}

/// The workspace's packages from the manifests: package name to (its directory, the names it depends on).
fn packages() -> BTreeMap<String, (PathBuf, Vec<String>)> {
    let workspace = read_toml(&root().join("Cargo.toml"));
    let members = workspace["workspace"]["members"].as_array().expect("[workspace] members");
    members
        .iter()
        .map(|member| {
            let dir = root().join(member.as_str().expect("a member is a path"));
            let manifest = read_toml(&dir.join("Cargo.toml"));
            let name = manifest["package"]["name"].as_str().expect("[package] name").to_owned();
            let deps = dependency_tables(&manifest)
                .into_iter()
                .flat_map(|(_, table)| table.iter().map(|(key, spec)| dependency_package(key, spec).to_owned()))
                .collect();
            (name, (dir.canonicalize().expect("member dir"), deps))
        })
        .collect()
}

/// Whether `src` has the module `name`: `<name>.rs` or `<name>/mod.rs`.
fn has_module(src: &Path, name: &str) -> bool {
    src.join(format!("{name}.rs")).is_file() || src.join(name).join("mod.rs").is_file()
}

/// docs/implementing.md, Layout: four members; `cpd-engine` holds `watch` and `doctor` and depends on `cpd-core`;
/// `cpd-tui` does not depend on `cpd-engine` (`watch::restart` is passed in); `cpd-core` has neither module.
#[test]
fn the_four_crates_cpd_engine_holds_watch_and_doctor_and_cpd_tui_does_not_depend_on_it() {
    let packages = packages();
    let mut problems = Vec::new();

    let names: Vec<_> = packages.keys().map(String::as_str).collect();
    if names != ["cpd", "cpd-core", "cpd-engine", "cpd-tui"] {
        problems.push(format!("the workspace members are {names:?}, not cpd, cpd-core, cpd-engine and cpd-tui"));
    }

    match packages.get("cpd-engine") {
        None => problems.push("cpd-engine is not a workspace member".to_owned()),
        Some((dir, deps)) => {
            if !dir.ends_with("engine") {
                problems.push(format!("cpd-engine is in {}, not `engine/`", dir.display()));
            }
            for module in ["watch", "doctor"] {
                if !has_module(&dir.join("src"), module) {
                    problems.push(format!("engine/src/{module} does not exist"));
                }
            }
            if !deps.iter().any(|d| d == "cpd-core") {
                problems.push("cpd-engine does not depend on cpd-core".to_owned());
            }
        }
    }

    match packages.get("cpd-tui") {
        None => problems.push("cpd-tui is not a workspace member".to_owned()),
        Some((_, deps)) => {
            if deps.iter().any(|d| d == "cpd-engine") {
                problems.push("cpd-tui depends on cpd-engine".to_owned());
            }
        }
    }

    match packages.get("cpd-core") {
        None => problems.push("cpd-core is not a workspace member".to_owned()),
        Some((dir, _)) => {
            for module in ["watch", "doctor"] {
                if has_module(&dir.join("src"), module) {
                    problems.push(format!("core/src/{module} exists: it belongs to cpd-engine"));
                }
            }
        }
    }

    assert!(problems.is_empty(), "the crates are not those of docs/implementing.md's Layout:\n{}", problems.join("\n"));
}

/// The dependencies that decide the features other crates resolve, which every member must have (as a dependency or a
/// dev-dependency, `workspace = true`): ratatui is what enables rustix `termios`/`stdio`, hashbrown 0.16's features,
/// syn `extra-traits` and time `local-offset`, and only `tui` uses it.
const UNIFIERS: [&str; 1] = ["ratatui"];

/// `-p <member>` resolves every dependency's features as `--workspace` does (docs/implementing.md, Tests): the first
/// `-p` run after the Gate's workspace test build then compiles nothing. `-p` unifies features over that member alone,
/// `--workspace` over all of them, so a member that lacks a dependency another member has (ratatui: tui only) resolves
/// fewer features, and cargo rebuilds what they touch. The fix is a dev-dependency on what differs, in the member.
///
/// Read from the text of the manifests, no cargo runs (`cargo tree`'s resolver is cargo's, not ours; research
/// tests-mock-mig-rest.md §5 row 3). That is the weaker check: it knows only the dependencies named in [`UNIFIERS`], where
/// `cargo tree` found every difference. A new one is added there, with its reason, when a `-p` run rebuilds.
#[test]
fn every_member_resolves_the_features_of_the_workspace_build() {
    let workspace = read_toml(&root().join("Cargo.toml"));
    let shared = workspace["workspace"]["dependencies"].as_table().expect("[workspace.dependencies]");
    let mut problems = Vec::new();
    for unifier in UNIFIERS {
        if !shared.contains_key(unifier) {
            problems.push(format!("{unifier} is not in [workspace.dependencies]"));
        }
        for (member, (_, deps)) in packages() {
            if !deps.iter().any(|d| d == unifier) {
                problems.push(format!("{member} has no `{unifier}` dependency"));
            }
        }
        // Where it is the member's own dependency it is inherited, so every member resolves it with the same features.
        for (member, (dir, _)) in packages() {
            let manifest = read_toml(&dir.join("Cargo.toml"));
            for (kind, table) in dependency_tables(&manifest) {
                if let Some(spec) = table.get(unifier)
                    && spec.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
                        problems.push(format!("{member}: [{kind}] {unifier} is not `workspace = true`"));
                    }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "`-p <member>` may resolve other features than `--workspace`, so its first run after the workspace test build rebuilds them; \
         add `ratatui.workspace = true` to the member's [dev-dependencies] (as core's):\n{}",
        problems.join("\n")
    );
}
