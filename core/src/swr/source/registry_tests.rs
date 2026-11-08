//! `registry()` is the one place a source becomes live (swr.md §11). When it carries the dead `Github` stub
//! instead of the live `GithubSource`, the engine's GitHub pass never runs: nothing fetches `github:pass:all`,
//! `swr_repos` stays empty and every merge is refused (air-n72).

use super::github_tests::Rig;
use super::{Cx, Source, registry};
use crate::clock::Clock;

/// The source the registry carries for the system `github`.
fn github_source() -> &'static dyn Source {
    registry()
        .iter()
        .copied()
        .find(|s| s.system() == "github")
        .expect("a github source is registered")
}

/// The keys the registered github source reports as due at the rig's clock (the endpoint is unpaused on a fresh store).
fn due(rig: &mut Rig) -> Vec<String> {
    let now = rig.fakes.clock.utc();
    let source = github_source();
    let mut cx = Cx {
        store: &mut rig.store,
        ports: &rig.fakes.ports,
        home: &rig.home,
        config: &rig.config,
    };
    source.due(&mut cx, now).unwrap()
}

#[test]
fn the_registry_carries_the_live_github_source() {
    let mut rig = Rig::new();
    // An unpaused github endpoint: the live source reports the whole pass as due. The dead stub reports nothing.
    assert_eq!(
        due(&mut rig),
        vec!["github:pass:all".to_owned()],
        "the source the registry carries for github is the live one, whose pass key is due"
    );
}

#[test]
fn the_registry_github_source_reports_the_pass_key_as_due() {
    let mut rig = Rig::new();
    let keys = due(&mut rig);
    assert_eq!(
        keys.iter().filter(|k| *k == "github:pass:all").count(),
        1,
        "the pass key is due exactly once per pass: {keys:?}"
    );
    // A due pass key reaches fetch(), which reads GitHub through the (mocked) runner.
    let now = rig.fakes.clock.utc();
    let source = github_source();
    let mut cx = Cx {
        store: &mut rig.store,
        ports: &rig.fakes.ports,
        home: &rig.home,
        config: &rig.config,
    };
    source.fetch(&mut cx, &keys, now).unwrap();
    assert!(
        !rig.fakes.runner.calls().is_empty(),
        "the due pass reaches fetch() and reads through the runner"
    );
}
