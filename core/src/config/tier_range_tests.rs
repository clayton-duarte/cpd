//! Tier ranges of the roles and the ladder inside them (Tier model v2, contract §9): the ladder of valid rungs, the range of
//! each role from the role defaults, the next rung and the step up. Pure functions of a [`Catalog`] and the defaults.

use super::tier::*;

fn model(name: &str, efforts: &[u8]) -> TierModel {
    TierModel {
        model: format!("p/{name}"),
        efforts: efforts.to_vec(),
    }
}

/// H offers efforts 1 and 3, S offers 0, 1 and 3, O and G offer no levels (only `OX` and `GX`).
fn catalog() -> Catalog {
    Catalog {
        pi_version: "1.5.0".into(),
        o: model("opus", &[]),
        s: model("sonnet", &[0, 1, 3]),
        h: model("haiku", &[1, 3]),
        g: model("gemini", &[]),
    }
}

fn t(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

fn names(tiers: &[Tier]) -> Vec<String> {
    tiers.iter().map(Tier::to_string).collect()
}

fn defaults(auxiliar: &str, worker: &str, lead: &str, manager: &str) -> RoleTiers {
    RoleTiers {
        auxiliar: t(auxiliar),
        worker: t(worker),
        lead: t(lead),
        manager: t(manager),
    }
}

fn range(c: &Catalog, role: Role, d: &RoleTiers) -> Option<(String, String)> {
    c.range(role, d)
        .map(|r| (r.floor.to_string(), r.ceiling.to_string()))
}

fn span(floor: &str, ceiling: &str) -> Range {
    Range {
        floor: t(floor),
        ceiling: t(ceiling),
    }
}

fn pair(floor: &str, ceiling: &str) -> Option<(String, String)> {
    Some((floor.to_owned(), ceiling.to_owned()))
}

#[test]
fn the_ladder_lists_valid_rungs_g_h_s_o_then_effort_ascending() {
    assert_eq!(
        names(&catalog().ladder()),
        ["GX", "H1", "H3", "S0", "S1", "S3", "OX"]
    );
    // A gap is kept and a model with levels has no X rung.
    let mut c = catalog();
    c.o = model("opus", &[0, 2, 3, 4]);
    c.g = model("gemini", &[1, 2]);
    assert_eq!(
        names(&c.ladder()),
        [
            "G1", "G2", "H1", "H3", "S0", "S1", "S3", "O0", "O2", "O3", "O4"
        ]
    );
    // Every rung is valid, and they ascend.
    let ladder = c.ladder();
    assert!(ladder.iter().all(|r| c.is_valid(*r)));
    assert!(ladder.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn a_range_runs_from_its_floor_to_the_rung_below_the_next_role_default() {
    let c = catalog();
    let d = defaults("GX", "H1", "S1", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("H1", "SX"));
    assert_eq!(range(&c, Role::Lead, &d), pair("S1", "S3"));
    // The next default is itself excluded, and a role default may be effort 0.
    let d = defaults("GX", "H3", "S0", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("H3", "H3"));
    assert_eq!(range(&c, Role::Lead, &d), pair("S0", "S3"));
    let r = c.range(Role::Lead, &d).unwrap();
    assert!(r.contains(t("S0")) && r.contains(t("S1")) && r.contains(t("S3")));
    assert!(!r.contains(t("H3")) && !r.contains(t("OX")));
}

#[test]
fn the_manager_range_ends_at_the_top_rung() {
    let c = catalog();
    assert_eq!(
        range(&c, Role::Manager, &defaults("GX", "H1", "S0", "S1")),
        pair("S1", "OX")
    );
    assert_eq!(
        range(&c, Role::Manager, &defaults("GX", "H1", "S1", "OX")),
        pair("OX", "OX")
    );
    let mut gap = catalog();
    gap.o = model("opus", &[0, 2, 3, 4]);
    assert_eq!(
        range(&gap, Role::Manager, &defaults("GX", "H1", "S1", "O3")),
        pair("O3", "O4")
    );
}

#[test]
fn the_auxiliar_range_is_its_default_only() {
    let c = catalog();
    let d = defaults("GX", "H1", "S1", "OX");
    assert_eq!(range(&c, Role::Auxiliar, &d), pair("GX", "GX"));
    let r = c.range(Role::Auxiliar, &d).unwrap();
    assert!(r.contains(t("GX")) && !r.contains(t("H1")) && !r.contains(t("G0")));
    assert_eq!(
        range(&c, Role::Auxiliar, &defaults("H1", "S0", "S1", "OX")),
        pair("H1", "H1")
    );
}

#[test]
fn a_default_that_is_not_a_rung_floors_up_and_ceilings_down() {
    let c = catalog();
    // H2 and S2 are no rungs: the floor is the next rung up, the ceiling the last rung below the next default.
    let d = defaults("GX", "H2", "S2", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("H3", "S1"));
    assert_eq!(range(&c, Role::Lead, &d), pair("S3", "S3"));
    assert_eq!(range(&c, Role::Manager, &d), pair("OX", "OX"));
    // A default below the whole ladder floors to its lowest rung.
    let d = defaults("G0", "H0", "S2", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("HX", "S1"));
}

#[test]
fn a_role_with_no_valid_rung_in_its_span_has_no_range() {
    let c = catalog();
    // [H2, H3) holds no rung; the Lead's span [H3, OX) still does.
    let d = defaults("GX", "H2", "H3", "OX");
    assert_eq!(c.range(Role::Worker, &d), None);
    assert_eq!(range(&c, Role::Lead, &d), pair("H3", "S3"));
    // A manager default above the top rung leaves nothing to floor up to.
    assert_eq!(
        c.range(Role::Manager, &defaults("GX", "H1", "S1", "O4")),
        None
    );
}

/// R5: the rungs of a range are the ladder plus the X of every letter. A default `<L>X` is its own floor, also on a model
/// with efforts; the ceiling is the highest rung strictly below the next default, an X included; the auxiliar takes
/// `GX` on any G model.
#[test]
fn tier_x_is_its_own_floor_in_a_role_range() {
    let c = catalog(); // H offers 1,3, S offers 0,1,3: HX and SX are no ladder rung, but are rungs of a range
    let d = defaults("GX", "HX", "S1", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("HX", "SX"));
    let r = c.range(Role::Worker, &d).unwrap();
    assert!(
        r.contains(t("HX")) && r.contains(t("H1")) && r.contains(t("S0")) && r.contains(t("SX"))
    );
    assert!(!r.contains(t("S1")) && !r.contains(t("GX")));
    // A lead whose default is SX floors at SX; the worker below it ends at S0.
    let d = defaults("GX", "H1", "SX", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("H1", "S0"));
    assert_eq!(range(&c, Role::Lead, &d), pair("SX", "S3"));
    assert!(c.range(Role::Lead, &d).unwrap().contains(t("SX")));
    // A default that is no X floors up on the ladder, and a range holds the X between its rungs.
    let d = defaults("GX", "H1", "S1", "OX");
    assert_eq!(range(&c, Role::Worker, &d), pair("H1", "SX"));
    assert!(c.range(Role::Worker, &d).unwrap().contains(t("SX")));
    // The auxiliar is its default only: `GX` is valid on a G model that offers efforts.
    let mut g = catalog();
    g.g = model("gemini", &[1, 3]);
    let d = defaults("GX", "H1", "S1", "OX");
    assert_eq!(range(&g, Role::Auxiliar, &d), pair("GX", "GX"));
    // The manager's default `OX` is its own floor on a model with efforts.
    let mut o = catalog();
    o.o = model("opus", &[1, 2]);
    let d = defaults("GX", "H1", "S1", "OX");
    assert_eq!(range(&o, Role::Manager, &d), pair("OX", "O2"));
}

/// R6: only effort 0 is excluded: a scout keeps letter H, a research Job letter S, from the letter's `<L>X` up to the
/// worker ceiling.
#[test]
fn tier_x_is_in_the_explore_range_and_effort_0_is_not() {
    let c = catalog(); // H offers 1,3, S offers 0,1,3
    let worker = span("HX", "S3");
    let scout = c.explore_range(Letter::H, worker).unwrap();
    assert_eq!((scout.floor, scout.ceiling), (t("HX"), t("H3")));
    assert!(scout.contains(t("HX")) && scout.contains(t("H1")) && scout.contains(t("H3")));
    assert!(!scout.contains(t("GX")) && !scout.contains(t("SX")));
    let research = c.explore_range(Letter::S, worker).unwrap();
    assert_eq!((research.floor, research.ceiling), (t("SX"), t("S3")));
    assert!(research.contains(t("SX")) && research.contains(t("S1")));
    assert!(!research.contains(t("S0")), "effort 0 is excluded");
    assert!(!research.contains(t("HX")) && !research.contains(t("OX")));
    // A letter with efforts 0 and 1 only: its X is in, its 0 is out.
    let mut c2 = catalog();
    c2.h = model("haiku", &[0, 1]);
    let scout = c2.explore_range(Letter::H, span("HX", "S3")).unwrap();
    assert_eq!((scout.floor, scout.ceiling), (t("HX"), t("H1")));
    assert!(!scout.contains(t("H0")));
}

#[test]
fn next_in_climbs_one_rung_and_stops_at_the_ceiling() {
    let c = catalog();
    let r = span("H1", "OX");
    let mut walk = vec![t("H1")];
    while let Some(next) = c.next_in(r, *walk.last().unwrap()) {
        walk.push(next);
        assert!(walk.len() < 20, "the climb ends");
    }
    assert_eq!(names(&walk), ["H1", "H3", "S1", "S3", "OX"]);
    let capped = span("H1", "S1");
    assert_eq!(c.next_in(capped, t("H1")), Some(t("H3")));
    assert_eq!(c.next_in(capped, t("H3")), Some(t("S1")));
    assert_eq!(c.next_in(capped, t("S1")), None);
    // Inside a letter, a gap is skipped to the next valid effort.
    assert_eq!(c.next_in(span("S0", "S3"), t("S1")), Some(t("S3")));
    assert_eq!(c.next_in(span("S0", "S3"), t("S3")), None);
}

#[test]
fn next_in_leaves_a_letter_at_effort_1_or_x() {
    let c = catalog();
    // Never effort 0 when leaving a letter: S0 is skipped.
    assert_eq!(c.next_in(span("H1", "OX"), t("H3")), Some(t("S1")));
    assert_eq!(c.next_in(span("GX", "OX"), t("GX")), Some(t("H1")));
    // But a role default may itself be effort 0, and the climb goes on from it.
    assert_eq!(c.next_in(span("S0", "OX"), t("S0")), Some(t("S1")));
    // The lowest valid effort >= 1: S offers 0, 2, 3 here.
    let mut c2 = catalog();
    c2.s = model("sonnet", &[0, 2, 3]);
    assert_eq!(c2.next_in(span("H1", "OX"), t("H3")), Some(t("S2")));
    // A letter without levels is entered at X.
    let mut c3 = catalog();
    c3.s = model("sonnet", &[]);
    assert_eq!(c3.next_in(span("H1", "OX"), t("H3")), Some(t("SX")));
    assert_eq!(c3.next_in(span("H1", "OX"), t("SX")), Some(t("OX")));
    // O offers 0 and 4: leaving S lands on its lowest effort >= 1, O4, never O0.
    let mut c4 = catalog();
    c4.o = model("opus", &[0, 4]);
    assert_eq!(c4.next_in(span("S1", "O4"), t("S3")), Some(t("O4")));
}

#[test]
fn step_up_in_takes_the_lowest_valid_rung_at_or_above() {
    let c = catalog();
    let r = span("H1", "S3");
    let up = |from: &str| c.step_up_in(r, t(from)).map(|t| t.to_string());
    // Valid and in the range: itself, effort 0 included.
    assert_eq!(up("H1").as_deref(), Some("H1"));
    assert_eq!(up("S0").as_deref(), Some("S0"));
    // Not offered: the next valid rung up, across letters.
    assert_eq!(up("H2").as_deref(), Some("H3"));
    assert_eq!(up("H4").as_deref(), Some("S0"));
    assert_eq!(up("S2").as_deref(), Some("S3"));
    // Below the floor: the floor.
    assert_eq!(up("H0").as_deref(), Some("H1"));
    assert_eq!(up("GX").as_deref(), Some("H1"));
    // Above the ceiling: nothing.
    assert_eq!(up("S4"), None);
    assert_eq!(up("OX"), None);
}
