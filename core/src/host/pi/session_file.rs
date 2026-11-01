//! Finding a Session's pi file after start (contract §6.5).

use std::path::{Path, PathBuf};

use crate::ids::SessionId;

use super::PiDirs;

/// The Session's pi file once pi has created it (contract §6.5):
/// `<agent dir>/sessions/*/*_<id>.jsonl`, or `$PI_CODING_AGENT_SESSION_DIR/*_<id>.jsonl`
/// when that is set (pi then stores sessions directly in it). None until it exists.
pub fn session_file(dirs: &PiDirs, session: &SessionId) -> Option<PathBuf> {
    let suffix = format!("_{session}.jsonl");
    let in_dir = |dir: &Path| -> Option<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
            })
            .collect();
        found.sort();
        found.into_iter().next()
    };
    if let Some(dir) = &dirs.session_dir {
        return in_dir(dir);
    }
    let mut groups: Vec<PathBuf> = std::fs::read_dir(dirs.agent_dir.join("sessions"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    groups.sort();
    groups.iter().find_map(|g| in_dir(g))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const ID: &str = "596d007f-1111-4222-8333-444455556666";

    #[test]
    fn finds_the_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let session = SessionId::parse(ID).unwrap();
        let dirs = PiDirs::resolve(Some(home.clone()), None, None).unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let group = dirs.agent_dir.join("sessions/--home-u-repo--");
        fs::create_dir_all(&group).unwrap();
        fs::write(group.join("2026-10-14T10-00-00-000Z_other.jsonl"), "").unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let file = group.join(format!("2026-10-14T10-00-00-000Z_{ID}.jsonl"));
        fs::write(&file, "").unwrap();
        assert_eq!(session_file(&dirs, &session), Some(file));

        let flat = home.join("flat");
        fs::create_dir_all(&flat).unwrap();
        let dirs = PiDirs::resolve(Some(home), None, Some(flat.clone().into_os_string())).unwrap();
        assert_eq!(session_file(&dirs, &session), None);
        let file = flat.join(format!("2026_{ID}.jsonl"));
        fs::write(&file, "").unwrap();
        assert_eq!(session_file(&dirs, &session), Some(file));
    }
}
