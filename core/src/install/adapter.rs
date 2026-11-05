//! The adapter swap, the verify and the cleanup of `cpd install` (docs/implementing.md "The adapter swap"; contract §3.7).

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use time::macros::format_description;

use super::{Install, Stop, refused};
use crate::host::spawn::Pid;
use crate::ports::Ports;

/// A version that is not the current or the displaced one stays this long after its last modification, so a
/// concurrent install that has built it and not yet linked it is not undone.
const KEEP_SECONDS: i64 = 60 * 60;

/// A pin is a symlink `<share>/pi.pin.<pid>` whose target is the bare name of the version that pid loaded.
const PIN_PREFIX: &str = "pi.pin.";

/// `<home>/.local/share/cpd`: where cpd finds the adapter.
fn share(install: &Install) -> PathBuf {
    install.home.join(".local/share/cpd")
}

/// `<share>/pi`, the adapter link.
pub(super) fn link_of(install: &Install) -> PathBuf {
    share(install).join("pi")
}

fn failed(what: impl std::fmt::Display, e: io::Error) -> Stop {
    Stop(format!("cpd install failed: {what}: {e}"))
}

/// Install step 1: the adapter path is absent, a symlink or a directory.
pub(super) fn preflight(install: &Install) -> Result<(), Stop> {
    let link = link_of(install);
    match fs::symlink_metadata(&link) {
        Ok(meta) if !meta.file_type().is_symlink() && !meta.is_dir() => Err(refused(format!(
            "{} is neither a symlink nor a directory",
            link.display()
        ))),
        _ => Ok(()),
    }
}

/// Copies the directory `from` into the existing directory `to`, skipping the top-level entries named in `skip`
/// (`cp -R from/. to/`, symlinks as symlinks).
pub(super) fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if skip.iter().any(|s| entry.file_name() == *s) {
            continue;
        }
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            symlink(fs::read_link(entry.path())?, &target)?;
        } else if kind.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&entry.path(), &target, &[])?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Removes `path`, a directory, a file or a symlink (never followed).
fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// The swap: builds `pi.<id>`, links it and puts the link over `<share>/pi`; returns the displaced version's name
/// (none when there was no adapter). A failure removes `pi.tmp.<pid>` and `pi.link.<pid>`.
pub(super) fn swap(ports: &Ports, install: &Install) -> Result<Option<String>, Stop> {
    let share = share(install);
    let adapter = share.join("pi");
    let tmp = share.join(format!("pi.tmp.{}", install.pid));
    let link = share.join(format!("pi.link.{}", install.pid));
    let result = build_and_swap(ports, install, &share, &adapter, &tmp, &link);
    if result.is_err() {
        let _ = remove_any(&tmp);
        let _ = remove_any(&link);
    }
    result
}

fn build_and_swap(
    ports: &Ports,
    install: &Install,
    share: &Path,
    adapter: &Path,
    tmp: &Path,
    link: &Path,
) -> Result<Option<String>, Stop> {
    fs::create_dir_all(share)
        .map_err(|e| failed(format!("cannot create {}", share.display()), e))?;
    let stamp = ports
        .clock
        .utc()
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]"
        ))
        .expect("a UTC time always formats");
    let version = format!("pi.{stamp}-{}", install.pid);
    fs::create_dir(tmp).map_err(|e| failed(format!("cannot create {}", tmp.display()), e))?;
    let source = install.root.join("pi");
    copy_tree(&source, tmp, &["test"]).map_err(|e| {
        failed(
            format!("cannot copy {} to {}", source.display(), tmp.display()),
            e,
        )
    })?;
    let built = share.join(&version);
    fs::rename(tmp, &built).map_err(|e| {
        failed(
            format!("cannot rename {} to {}", tmp.display(), built.display()),
            e,
        )
    })?;
    symlink(&version, link)
        .map_err(|e| failed(format!("cannot link {} to {version}", link.display()), e))?;

    let (displaced, is_dir) = match fs::symlink_metadata(adapter) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = fs::read_link(adapter)
                .map_err(|e| failed(format!("cannot read the link {}", adapter.display()), e))?;
            (Some(target.to_string_lossy().into_owned()), false)
        }
        Ok(meta) if meta.is_dir() => (Some("pi.legacy".to_owned()), true),
        _ => (None, false),
    };
    let swapped = if is_dir {
        // The migration: a rename of a symlink over a directory fails, and no portable call exchanges the two.
        let legacy = share.join("pi.legacy");
        fs::remove_dir_all(&legacy)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .and_then(|()| fs::rename(adapter, &legacy))
            .and_then(|()| {
                fs::rename(link, adapter).inspect_err(|_| {
                    let _ = fs::rename(&legacy, adapter);
                })
            })
    } else {
        fs::rename(link, adapter)
    };
    let back = if is_dir {
        "; the old directory is back"
    } else {
        ""
    };
    swapped.map_err(|e| {
        failed(
            format!(
                "cannot swap the adapter link over {}{back}",
                adapter.display()
            ),
            e,
        )
    })?;
    Ok(displaced)
}

/// The verify: `<adapter>/package.json` exists.
pub(super) fn verify(install: &Install) -> Result<(), Stop> {
    let adapter = link_of(install);
    if adapter.join("package.json").is_file() {
        return Ok(());
    }
    Err(Stop(format!(
        "cpd install failed: {} has no package.json after the swap",
        adapter.display()
    )))
}

/// Whether `name` is `pi.legacy` or `pi.<digit>...-<digit>...`.
fn is_version(name: &str) -> bool {
    let Some(id) = name.strip_prefix("pi.") else {
        return false;
    };
    id == "legacy"
        || (id.starts_with(|c: char| c.is_ascii_digit())
            && id
                .match_indices('-')
                .any(|(i, _)| id[i + 1..].starts_with(|c: char| c.is_ascii_digit())))
}

/// The cleanup: versions that are neither current, nor displaced, nor pinned by a live pid, nor modified in the
/// last 60 minutes, the pins and the scratch of pids that are gone. A failure is a warning, never a failed install.
pub(super) fn cleanup(
    ports: &Ports,
    install: &Install,
    displaced: Option<&str>,
    err: &mut dyn Write,
) {
    let share = share(install);
    let current = fs::read_link(share.join("pi"))
        .ok()
        .map(|t| t.to_string_lossy().into_owned());
    let now = ports.clock.utc().unix_timestamp();
    let mut cleaned = true;
    let entries = match fs::read_dir(&share) {
        Ok(entries) => entries,
        Err(_) => {
            let _ = writeln!(
                err,
                "cpd install warning: could not remove every old adapter version under {}",
                share.display()
            );
            return;
        }
    };
    let entries: Vec<_> = entries.flatten().collect();
    let mut protected: Vec<String> = Vec::new();
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(PIN_PREFIX) {
            continue;
        }
        let alive = name
            .rsplit('.')
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .is_some_and(|p| ports.spawn.alive(Pid(p)));
        if !alive {
            cleaned &= remove_any(&entry.path()).is_ok();
        } else if let Ok(target) = fs::read_link(entry.path()) {
            let target = target.to_string_lossy().into_owned();
            if is_version(&target) {
                protected.push(target);
            }
        }
    }
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(PIN_PREFIX) {
            continue;
        }
        if is_version(&name) {
            if Some(name.as_str()) == current.as_deref()
                || Some(name.as_str()) == displaced
                || protected.contains(&name)
            {
                continue;
            }
            let modified = fs::symlink_metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
            match modified {
                Some(at)
                    if now - i64::try_from(at.as_secs()).unwrap_or(i64::MAX) > KEEP_SECONDS =>
                {
                    cleaned &= remove_any(&path).is_ok()
                }
                Some(_) => {}
                None => cleaned = false,
            }
        } else if name.starts_with("pi.tmp.") || name.starts_with("pi.link.") {
            let alive = name
                .rsplit('.')
                .next()
                .and_then(|p| p.parse::<u32>().ok())
                .is_some_and(|p| ports.spawn.alive(Pid(p)));
            if !alive {
                cleaned &= remove_any(&path).is_ok();
            }
        }
    }
    if !cleaned {
        let _ = writeln!(
            err,
            "cpd install warning: could not remove every old adapter version under {}",
            share.display()
        );
    }
}
