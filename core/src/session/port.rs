//! A Plan's plannotator port (contract §6.5, "The Plan's plannotator port"):
//! one port of the config range `plannotator_ports` per Plan, assigned when
//! `session run` starts the Lead's pi and kept in `run/<plan>/plannotator-port`.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use rusqlite::params;
use serde_json::json;

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::lock;
use crate::store::Store;

/// The Plan's port: the one in its file when that is inside the range and no other live Plan holds it, else
/// the lowest port of the range no other Plan holds (a `done` or archived Plan holds none),
/// written to the file. `no_port` when none is free. Idempotent. Waits for
/// the lock without a limit: the holder only reads and writes a few small files.
pub fn assign(home: &CpdHome, store: &mut Store, config: &Config, plan: &PlanId) -> Result<u16, CpdError> {
    let range = config.plannotator_ports;
    let _lock = lock(&home.plannotator_ports_lock())?;
    let file = home.plannotator_port(plan);
    let holders = holders(home, store, config, plan)?;
    let recorded = read_port(&file)?.filter(|p| range.contains(*p));
    if let Some(port) = recorded {
        match holders.iter().find(|(_, held)| *held == port) {
            None => return Ok(port),
            Some((other, _)) => eprintln!("cpd: plannotator port {port} of Plan {plan} is held by Plan {other}; assigning another."),
        }
    }
    let Some(port) = range.ports().find(|p| !holders.iter().any(|(_, held)| held == p)) else {
        let names = holders.iter().map(|(plan, port)| format!("{plan} ({port})")).collect::<Vec<_>>().join(", ");
        return Err(CpdError::new(
            ErrorCode::NoPort,
            format!("No plannotator port is free in {range}: held by {names}."),
        )
        .with("range", range.to_string())
        .with("holders", holders.iter().map(|(plan, port)| json!({"plan": plan, "port": port})).collect::<Vec<_>>()));
    };
    write_port(&file, port)?;
    Ok(port)
}

/// The other Plans, neither `done` nor archived, whose file holds a port inside the range, by Plan id.
fn holders(home: &CpdHome, store: &mut Store, config: &Config, plan: &PlanId) -> Result<Vec<(String, u16)>, CpdError> {
    let others = store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM plans WHERE id != ?1 AND phase != 'done' AND archived_at IS NULL ORDER BY id")?;
        let ids = stmt.query_map(params![plan.as_str()], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    })?;
    let range = config.plannotator_ports;
    let mut held = Vec::new();
    for id in others {
        let id_parsed = PlanId::parse(&id).map_err(|e| {
            CpdError::new(ErrorCode::StoreFailed, format!("The database holds a Plan with an invalid id {id:?}: {}", e.message()))
        })?;
        if let Some(port) = read_port(&home.plannotator_port(&id_parsed))?.filter(|p| range.contains(*p)) {
            held.push((id, port));
        }
    }
    Ok(held)
}

/// The port in `path`; `None` when there is no file or it holds no port.
fn read_port(path: &Path) -> Result<Option<u16>, CpdError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.trim().parse().ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error("read", path, &e)),
    }
}

/// Writes the port to a temporary file beside `path`, then renames it over `path`.
fn write_port(path: &Path, port: u16) -> Result<(), CpdError> {
    let dir = path.parent().expect("a Plan's file has a directory");
    fs::create_dir_all(dir).map_err(|e| io_error("create", dir, &e))?;
    let tmp = dir.join(format!(".plannotator-port.{}.tmp", std::process::id()));
    fs::write(&tmp, format!("{port}\n")).map_err(|e| io_error("write", &tmp, &e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_error("write", path, &e)
    })
}

fn lock(path: &Path) -> Result<File, CpdError> {
    let dir = path.parent().expect("a lock file has a directory");
    fs::create_dir_all(dir).map_err(|e| io_error("create", dir, &e))?;
    let file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| io_error("open", path, &e))?;
    lock::blocking(&file).map_err(|e| io_error("lock", path, &e))?;
    Ok(file)
}

fn io_error(what: &str, path: &Path, e: &std::io::Error) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot {what} {}: {e}.", path.display()))
        .with("path", path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        home: CpdHome,
        store: Store,
    }

    fn fixture(plans: &[(&str, &str)]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let mut sql = String::from(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('s1', 'lead', 'ctx', 'One', '/', 'O3', 'idle', 't', 't');",
        );
        for (id, phase) in plans {
            let extra = match *phase {
                "done" => "'t', 't'",
                _ => "NULL, NULL",
            };
            sql.push_str(&format!(
                "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('{id}', 'ctx', 'T', '{phase}', 's1', 't', {extra});"
            ));
        }
        store.write("fixture", |tx| Ok(tx.execute_batch(&sql)?)).unwrap();
        Fixture { _dir: dir, home, store }
    }

    fn config(range: &str) -> Config {
        let text = format!("version = 1\nmachine = \"m1\"\nplannotator_ports = \"{range}\"\n");
        Config::parse(&text, Path::new("config.toml")).unwrap().0
    }

    fn plan(id: &str) -> PlanId {
        PlanId::parse(id).unwrap()
    }

    fn assign_to(f: &mut Fixture, range: &str, id: &str) -> Result<u16, CpdError> {
        assign(&f.home, &mut f.store, &config(range), &plan(id))
    }

    #[test]
    fn plans_get_the_lowest_free_port_in_turn() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
        assert_eq!(fs::read_to_string(f.home.plannotator_port(&plan("bb"))).unwrap(), "19433\n");
    }

    #[test]
    fn a_plan_keeps_its_port() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
        for _ in 0..2 {
            assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
            assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        }
    }

    #[test]
    fn a_done_plans_port_is_reused() {
        let mut f = fixture(&[("aa", "done"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19432);
    }

    #[test]
    fn a_colliding_plan_moves_and_the_other_keeps_its_port() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        for id in ["aa", "bb"] {
            write_port(&f.home.plannotator_port(&plan(id)), 19432).unwrap();
        }
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
        assert_eq!(fs::read_to_string(f.home.plannotator_port(&plan("bb"))).unwrap(), "19433\n");
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
    }

    #[test]
    fn a_done_plans_leftover_file_does_not_block() {
        let mut f = fixture(&[("aa", "done"), ("bb", "grounding")]);
        for id in ["aa", "bb"] {
            write_port(&f.home.plannotator_port(&plan(id)), 19432).unwrap();
        }
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19432);
    }

    #[test]
    fn an_archived_plans_port_is_reused() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        f.store.write("archive", |tx| Ok(tx.execute("UPDATE plans SET archived_at = 't' WHERE id = 'aa'", [])?)).unwrap();
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19432);
    }

    #[test]
    fn a_live_plans_port_is_not_reused() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19441", "bb").unwrap(), 19433);
    }

    #[test]
    fn a_port_outside_a_changed_range_is_replaced() {
        let mut f = fixture(&[("aa", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "20000-20001", "aa").unwrap(), 20000);
        assert_eq!(fs::read_to_string(f.home.plannotator_port(&plan("aa"))).unwrap(), "20000\n");
    }

    #[test]
    fn a_port_outside_the_range_holds_nothing() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19441", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "20000-20000", "bb").unwrap(), 20000);
    }

    #[test]
    fn a_full_range_is_no_port_naming_the_holders() {
        let mut f = fixture(&[("aa", "grounding"), ("bb", "plan"), ("cc", "grounding")]);
        assert_eq!(assign_to(&mut f, "19432-19433", "aa").unwrap(), 19432);
        assert_eq!(assign_to(&mut f, "19432-19433", "bb").unwrap(), 19433);
        let err = assign_to(&mut f, "19432-19433", "cc").unwrap_err();
        assert_eq!(err.code(), ErrorCode::NoPort);
        assert_eq!(err.message(), "No plannotator port is free in 19432-19433: held by aa (19432), bb (19433).");
        assert_eq!(err.details()["range"], json!("19432-19433"));
        assert_eq!(err.details()["holders"], json!([{"plan": "aa", "port": 19432}, {"plan": "bb", "port": 19433}]));
        assert!(!f.home.plannotator_port(&plan("cc")).exists());
    }
}
