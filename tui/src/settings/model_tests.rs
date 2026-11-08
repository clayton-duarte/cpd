//! The Agent Tiers state machine, from hand-built reads: no process, no drawing.

use super::model::*;
use super::view_tests::{Act, Commands, call, popup, reads, workers_screen};
use cpd_core::config::{Catalog, Letter, Role, RoleTiers, Tier, TierModel};
use cpd_core::error::{CpdError, ErrorCode};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn t(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

fn tiers(a: &str, w: &str, l: &str, m: &str) -> RoleTiers {
    RoleTiers {
        decide: cpd_core::config::Tier::x(cpd_core::config::Letter::O),
        auxiliar: t(a),
        worker: t(w),
        lead: t(l),
        manager: t(m),
    }
}

fn tm(model: &str, efforts: &[u8]) -> TierModel {
    TierModel {
        model: model.into(),
        efforts: efforts.to_vec(),
    }
}

fn catalog_of(o: &[u8], s: &[u8], h: &[u8], g: &[u8]) -> Catalog {
    Catalog {
        j: cpd_core::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.5.0".into(),
        o: tm("bifrost/vertex/gemini-4-argon", o),
        s: tm("bifrost/vertex/claude-sonnet-5-5", s),
        h: tm("bifrost/vertex/claude-haiku-5-5", h),
        g: tm("bifrost/vertex/gemini-3.8-flash", g),
    }
}

fn catalog() -> Catalog {
    catalog_of(&[0, 2, 3, 4], &[0, 1, 2, 3, 4], &[0, 1, 2, 3, 4], &[])
}

fn catalog_two() -> Catalog {
    catalog_of(&[0, 2, 3, 4], &[1, 3], &[0, 1, 2, 3, 4], &[0, 1])
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

const COPILOT: [&str; 4] = [
    "github-copilot/claude-opus-5.5",
    "github-copilot/claude-sonnet-5.5",
    "github-copilot/claude-haiku-4.5",
    "github-copilot/gemini-3.8-flash",
];
const BIFROST: [&str; 4] = [
    "bifrost/vertex/gemini-4-argon",
    "bifrost/vertex/claude-sonnet-5-5",
    "bifrost/vertex/claude-haiku-5-5",
    "bifrost/vertex/gemini-3.8-flash",
];

fn model() -> Model {
    let preset = |name: &str, models: [&str; 4]| Preset {
        name: name.into(),
        models: models.map(String::from),
    };
    Model::new(Reads {
        tiers: tiers("GX", "H3", "S4", "O3"),
        presets: vec![preset("bifrost", BIFROST), preset("copilot", COPILOT)],
        catalog: catalog(),
        providers: vec![
            Provider {
                provider: "bifrost".into(),
                ids: strings(&[
                    "vertex/gemini-4-argon",
                    "vertex/claude-sonnet-5-5",
                    "vertex/claude-haiku-5-5",
                    "vertex/gemini-3.8-flash",
                ]),
            },
            Provider {
                provider: "github-copilot".into(),
                ids: strings(&[
                    "claude-opus-5.5",
                    "claude-sonnet-5.5",
                    "claude-haiku-4.5",
                    "gemini-3.8-flash",
                ]),
            },
        ],
    })
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(m: &mut Model, text: &str) {
    for c in text.chars() {
        assert_eq!(m.key(key(KeyCode::Char(c))), Effect::None);
    }
}

fn row(m: &Model, row: Row) -> RowView {
    m.rows().into_iter().find(|r| r.row == row).unwrap()
}

/// Picks `text` in `row`'s offered list.
fn pick(m: &mut Model, r: Row, text: &str) -> Effect {
    let at = m
        .offered(r)
        .iter()
        .position(|e| e == text)
        .unwrap_or_else(|| panic!("{text} not offered by {r:?}"));
    m.pick(r, at)
}

fn err(code: ErrorCode, message: &str) -> CpdError {
    CpdError::new(code, message)
}

const AUX: Row = Row::Tier(Role::Auxiliar);
const WORKER: Row = Row::Tier(Role::Worker);
const LEAD: Row = Row::Tier(Role::Lead);
const MANAGER: Row = Row::Tier(Role::Manager);

#[test]
fn the_rows_are_preset_provider_o_s_h_g_then_auxiliar_worker_lead_manager() {
    let m = model();
    let got: Vec<(Row, String, String)> = m
        .rows()
        .into_iter()
        .map(|r| (r.row, r.label, r.value))
        .collect();
    let want = [
        (Row::Preset, "Preset", "bifrost"),
        (Row::Provider, "Provider", "bifrost"),
        (Row::Model(Letter::O), "O  opus", "vertex/gemini-4-argon"),
        (
            Row::Model(Letter::S),
            "S  sonnet",
            "vertex/claude-sonnet-5-5",
        ),
        (Row::Model(Letter::H), "H  haiku", "vertex/claude-haiku-5-5"),
        (
            Row::Model(Letter::G),
            "G  gemini",
            "vertex/gemini-3.8-flash",
        ),
        (AUX, "auxiliar", "GX"),
        (WORKER, "worker", "H3"),
        (LEAD, "lead", "S4"),
        (MANAGER, "manager", "O3"),
    ]
    .map(|(r, l, v)| (r, l.to_string(), v.to_string()));
    assert_eq!(got, want);
    let rows = m.rows();
    assert_eq!(
        rows.iter().map(|r| r.focused).collect::<Vec<_>>(),
        [
            true, false, false, false, false, false, false, false, false, false
        ]
    );
    assert!(
        rows.iter()
            .all(|r| !r.open && !r.changed && !r.invalid && !r.dimmed)
    );
}

#[test]
fn the_range_column_is_default_only_for_the_auxiliar_and_floor_dots_ceiling_for_the_others() {
    let mut m = model();
    let ranges = |m: &Model| m.rows().into_iter().map(|r| r.range).collect::<Vec<_>>();
    assert_eq!(
        ranges(&m),
        [
            "",
            "",
            "",
            "",
            "",
            "",
            "default only",
            "H3..S3",
            "S4..O2",
            "O3..O4"
        ]
    );
    // A lower lead raises nothing below it and moves the worker's ceiling and the lead's own range.
    pick(&mut m, LEAD, "S2");
    assert_eq!(
        ranges(&m),
        [
            "",
            "",
            "",
            "",
            "",
            "",
            "default only",
            "H3..S1",
            "S2..O2",
            "O3..O4"
        ]
    );
}

#[test]
fn a_tier_row_offers_only_rungs_between_its_neighbours_defaults() {
    let mut m = model();
    assert_eq!(m.offered(AUX), strings(&["GX", "H0", "H1", "H2"]));
    assert_eq!(
        m.offered(WORKER),
        strings(&["H0", "H1", "H2", "H3", "H4", "S0", "S1", "S2", "S3"])
    );
    assert_eq!(
        m.offered(LEAD),
        strings(&["H4", "S0", "S1", "S2", "S3", "S4", "O0", "O2"])
    );
    assert_eq!(m.offered(MANAGER), strings(&["O0", "O2", "O3", "O4"]));
    // The neighbours' current defaults move the bounds.
    pick(&mut m, WORKER, "H1");
    assert_eq!(m.offered(AUX), strings(&["GX", "H0"]));
}

#[test]
fn changing_the_provider_keeps_a_model_only_when_the_new_provider_offers_it() {
    let mut m = model();
    assert_eq!(
        m.offered(Row::Provider),
        strings(&["bifrost", "github-copilot"])
    );
    assert_eq!(pick(&mut m, Row::Provider, "github-copilot"), Effect::None);
    let value = |l| row(&m, Row::Model(l)).value;
    assert_eq!(value(Letter::O), "(unset)");
    assert_eq!(value(Letter::S), "(unset)");
    assert_eq!(value(Letter::H), "(unset)");
    assert_eq!(value(Letter::G), "(unset)");
    assert_eq!(row(&m, Row::Provider).value, "github-copilot");
    assert!(row(&m, Row::Model(Letter::O)).invalid);
    assert!(row(&m, Row::Model(Letter::G)).invalid);
    assert_eq!(
        m.offered(Row::Model(Letter::S)),
        strings(&[
            "claude-opus-5.5",
            "claude-sonnet-5.5",
            "claude-haiku-4.5",
            "gemini-3.8-flash"
        ])
    );
}

#[test]
fn an_unset_model_is_invalid_and_blocks_save() {
    let mut m = model();
    assert_eq!(m.blocked(), None);
    pick(&mut m, Row::Provider, "github-copilot");
    let why = "Model O is not set.".to_string();
    assert_eq!(m.blocked(), Some(why.clone()));
    assert_eq!(m.save(), Effect::None);
    assert_eq!(m.footer(), Some(why));
}

#[test]
fn load_fills_the_provider_and_the_four_models_from_the_preset_and_writes_nothing() {
    let mut m = model();
    assert_eq!(m.offered(Row::Preset), strings(&["bifrost", "copilot"]));
    assert_eq!(pick(&mut m, Row::Preset, "copilot"), Effect::None);
    // Picking a preset only selects it.
    assert_eq!(row(&m, Row::Provider).value, "bifrost");
    assert!(m.steps().is_empty());
    let want = COPILOT
        .iter()
        .zip([Letter::O, Letter::S, Letter::H, Letter::G])
        .map(|(r, l)| (l, r.to_string()))
        .collect();
    assert_eq!(m.load(), Effect::Reread(want));
    assert_eq!(row(&m, Row::Provider).value, "github-copilot");
    let values: Vec<String> = [Letter::O, Letter::S, Letter::H, Letter::G]
        .map(|l| row(&m, Row::Model(l)).value)
        .into();
    assert_eq!(
        values,
        strings(&[
            "claude-opus-5.5",
            "claude-sonnet-5.5",
            "claude-haiku-4.5",
            "gemini-3.8-flash"
        ])
    );
    // Nothing is written until Save: the step is pending.
    assert_eq!(m.steps()[0], Step::PresetSet(COPILOT.map(String::from)));
}

#[test]
fn picking_a_model_asks_for_a_reread_with_the_four_candidates() {
    let mut m = model();
    let want = vec![
        (Letter::O, BIFROST[0].to_string()),
        (Letter::S, BIFROST[0].to_string()),
        (Letter::H, BIFROST[2].to_string()),
        (Letter::G, BIFROST[3].to_string()),
    ];
    assert_eq!(
        pick(&mut m, Row::Model(Letter::S), "vertex/gemini-4-argon"),
        Effect::Reread(want)
    );
    assert!(row(&m, Row::Model(Letter::S)).changed);
}

#[test]
fn a_reread_that_drops_a_default_rung_flags_that_tier_and_blocks_save() {
    let mut m = model();
    pick(&mut m, Row::Model(Letter::S), "vertex/gemini-4-argon");
    m.reread_done(catalog_two());
    let invalid: Vec<bool> = [AUX, WORKER, LEAD, MANAGER]
        .map(|r| row(&m, r).invalid)
        .into();
    // GX is valid on every model (contract §9, R2), so only S4, no rung of an S with efforts 1 and 3, is flagged.
    assert_eq!(invalid, [false, false, true, false]);
    let why = "lead S4 is not a rung of S's model.".to_string();
    assert_eq!(m.blocked(), Some(why.clone()));
    assert_eq!(m.save(), Effect::None);
    assert_eq!(m.footer(), Some(why));
    // The new rungs are offered.
    assert_eq!(m.offered(AUX), strings(&["G0", "G1", "H0", "H1", "H2"]));
    // Picking a valid rung for the lead clears the flag.
    pick(&mut m, LEAD, "S3");
    assert!(!row(&m, LEAD).invalid);
    assert_eq!(m.blocked(), None);
}

#[test]
fn a_failed_reread_blocks_the_tiers_section_and_shows_its_reason() {
    let mut m = model();
    pick(&mut m, Row::Model(Letter::S), "vertex/gemini-4-argon");
    m.reread_failed(&err(ErrorCode::CatalogFailed, "pi did not answer."));
    let why = "catalog_failed: pi did not answer.".to_string();
    assert_eq!(m.blocked(), Some(why.clone()));
    assert_eq!(m.footer(), Some(why));
    assert_eq!(m.save(), Effect::None);
    for r in [AUX, WORKER, LEAD, MANAGER] {
        assert!(row(&m, r).dimmed, "{r:?}");
        assert!(m.offered(r).is_empty(), "{r:?}");
    }
    for l in [Letter::O, Letter::S, Letter::H, Letter::G] {
        assert!(!row(&m, Row::Model(l)).dimmed);
    }
}

#[test]
fn the_steps_are_models_then_tiers_then_retier_and_skip_what_did_not_change() {
    let mut m = model();
    pick(&mut m, Row::Preset, "copilot");
    m.load();
    m.reread_done(catalog());
    pick(&mut m, WORKER, "H2");
    pick(&mut m, MANAGER, "O4");
    assert_eq!(m.save(), Effect::Save);
    assert_eq!(
        m.steps(),
        vec![
            Step::PresetSet(COPILOT.map(String::from)),
            Step::TiersSet(tiers("GX", "H2", "S4", "O4")),
            Step::Retier,
        ]
    );
    // Only the tiers changed.
    let mut m = model();
    pick(&mut m, WORKER, "H2");
    assert_eq!(
        m.steps(),
        vec![Step::TiersSet(tiers("GX", "H2", "S4", "O3")), Step::Retier]
    );
    // Only the models changed.
    let mut m = model();
    pick(&mut m, Row::Model(Letter::H), "vertex/gemini-3.8-flash");
    let mut want = BIFROST.map(String::from);
    want[2] = BIFROST[3].into();
    assert_eq!(m.steps(), vec![Step::PresetSet(want), Step::Retier]);
}

#[test]
fn a_done_step_becomes_the_baseline_so_a_retry_runs_only_the_failed_one() {
    let mut m = model();
    pick(&mut m, Row::Model(Letter::H), "vertex/gemini-3.8-flash");
    pick(&mut m, WORKER, "H2");
    let steps = m.steps();
    assert_eq!(steps.len(), 3);
    m.step_done(&steps[0]);
    assert!(!row(&m, Row::Model(Letter::H)).changed);
    m.step_failed(&steps[1], &err(ErrorCode::ConfigInvalid, "no."));
    assert!(row(&m, WORKER).changed);
    let tiers_step = Step::TiersSet(tiers("GX", "H2", "S4", "O3"));
    assert_eq!(m.steps(), vec![tiers_step.clone(), Step::Retier]);
    m.step_done(&tiers_step);
    assert!(!row(&m, WORKER).changed);
    assert_eq!(m.steps(), vec![Step::Retier]);
    m.step_failed(&Step::Retier, &err(ErrorCode::CatalogFailed, "no session."));
    assert_eq!(m.steps(), vec![Step::Retier]);
    m.step_done(&Step::Retier);
    assert!(m.steps().is_empty());
    assert_eq!(m.save(), Effect::Close);
}

#[test]
fn a_refused_step_shows_its_code_and_message_in_the_footer() {
    let mut m = model();
    assert_eq!(m.footer(), None);
    pick(&mut m, WORKER, "H2");
    let step = m.steps()[0].clone();
    m.step_failed(
        &step,
        &err(ErrorCode::ConfigInvalid, "worker_tier is not a rung."),
    );
    assert_eq!(
        m.footer(),
        Some("config_invalid: worker_tier is not a rung.".into())
    );
    // The refusal does not block a retry.
    assert_eq!(m.blocked(), None);
    assert_eq!(m.save(), Effect::Save);
}

#[test]
fn nothing_changed_means_no_steps_and_save_closes() {
    let mut m = model();
    assert!(m.steps().is_empty());
    assert_eq!(m.save(), Effect::Close);
    // A change put back is no change.
    pick(&mut m, WORKER, "H2");
    pick(&mut m, WORKER, "H3");
    assert!(m.steps().is_empty());
    assert_eq!(m.save(), Effect::Close);
}

#[test]
fn the_filter_narrows_an_open_list_by_substring_and_enter_picks() {
    let mut m = model();
    m.key(key(KeyCode::Down));
    m.key(key(KeyCode::Down));
    assert!(row(&m, Row::Model(Letter::O)).focused);
    assert_eq!(m.key(key(KeyCode::Enter)), Effect::None);
    assert!(row(&m, Row::Model(Letter::O)).open);
    assert_eq!(
        m.list(),
        Some(ListView {
            filter: String::new(),
            entries: strings(&BIFROST.map(|r| r.trim_start_matches("bifrost/"))),
            highlight: 0,
            current: Some(0),
        })
    );
    typed(&mut m, "claude");
    let list = m.list().unwrap();
    assert_eq!(list.filter, "claude");
    assert_eq!(
        list.entries,
        strings(&["vertex/claude-sonnet-5-5", "vertex/claude-haiku-5-5"])
    );
    assert_eq!(list.current, None);
    m.key(key(KeyCode::Down));
    let want = vec![
        (Letter::O, "bifrost/vertex/claude-haiku-5-5".to_string()),
        (Letter::S, BIFROST[1].to_string()),
        (Letter::H, BIFROST[2].to_string()),
        (Letter::G, BIFROST[3].to_string()),
    ];
    assert_eq!(m.key(key(KeyCode::Enter)), Effect::Reread(want));
    assert_eq!(m.list(), None);
    assert_eq!(
        row(&m, Row::Model(Letter::O)).value,
        "vertex/claude-haiku-5-5"
    );
}

#[test]
fn escape_closes_an_open_list_first_and_then_the_view_without_writing() {
    let mut m = model();
    pick(&mut m, WORKER, "H2");
    m.key(key(KeyCode::Down));
    m.key(key(KeyCode::Enter));
    assert!(m.list().is_some());
    // In a list `q` is text.
    typed(&mut m, "q");
    assert_eq!(m.list().unwrap().filter, "q");
    assert_eq!(m.key(key(KeyCode::Esc)), Effect::None);
    assert_eq!(m.list(), None);
    assert_eq!(m.key(key(KeyCode::Esc)), Effect::Close);
    // The pending step stays unwritten: Esc never returns Save.
    assert_eq!(m.steps().len(), 2);
    assert_eq!(m.key(key(KeyCode::Char('q'))), Effect::Close);
}

/// The Workers screen driven through the popup: the count `cpd config` carries, the editor, the Save's argv.
/// It runs the view loop too (the fake `cpd config`, in process), but it pins the screen's rows and the command
/// the popup builds, so it lives here beside the state-machine tests.
#[test]
fn the_workers_screen_shows_the_current_concurrency_and_saves_it() {
    let commands = Commands::new();
    let acts = vec![
        Act::Key(KeyCode::Down),
        Act::Key(KeyCode::Enter),
        Act::Snap,
        Act::Key(KeyCode::Enter),
        Act::Type("12"),
        Act::Snap,
        Act::Key(KeyCode::Enter),
        Act::Click("[ Save ]", "[ Save ]"),
    ];
    let run = popup(&commands, acts);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        run.snaps[0].rows(),
        workers_screen("5"),
        "the count the fake `cpd config` carries"
    );
    assert_eq!(
        run.snaps[1].rows(),
        workers_screen("12▌"),
        "the editor shows the digits typed so far and the cursor"
    );
    let mut want = reads();
    want.push(call(&["config", "concurrency", "set", "12"]));
    assert_eq!(
        run.calls, want,
        "Save runs the new value through the command"
    );
}

/// The decide letter J (contract §9, air-d113, air-d115, air-d120): the settings rows offer it
/// and label it `jev`, one model row per Tier letter.
#[test]
fn the_settings_rows_offer_the_j_letter() {
    let m = model();
    let j = m
        .rows()
        .into_iter()
        .find(|r| r.label.starts_with("J  "))
        .unwrap_or_else(|| {
            panic!(
                "no J letter row: {:?}",
                m.rows().into_iter().map(|r| r.label).collect::<Vec<_>>()
            )
        });
    assert!(
        matches!(j.row, Row::Model(_)),
        "the J row is a model row: {:?}",
        j.row
    );
    assert_eq!(j.label, "J  jev", "the decide letter J is labelled jev");
}

/// The Tier parser admits the decide letter J: JX and J3 parse, while J4 stays refused
/// (J's model offers efforts 0, 1, 2 and 3, never 4; contract §9, air-d113, air-d115, air-d120).
#[test]
fn the_tier_parser_accepts_jx() {
    assert!(Tier::parse("JX").is_ok(), "JX must parse");
    assert!(Tier::parse("J3").is_ok(), "J3 must parse");
    assert!(
        Tier::parse("J4").is_err(),
        "J4 is not an effort of J's model"
    );
}
