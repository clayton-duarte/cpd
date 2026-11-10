//! Red test: cpd config tiers list CLI command (commands.md §14.2a, contract §9).

use crate::common;
use common::fx::Fx;
use serde_json::Value;

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

#[test]
fn config_tiers_list_returns_ladder_and_role_ranges() {
    let fx = Fx::new();
    fx.env.write_config(MINIMAL);

    let run = fx.cpd(&["config", "tiers", "list"]);
    assert_eq!(
        run.exit, 0,
        "cpd config tiers list must exit 0: stderr={}",
        run.stderr
    );

    let out = run.ok();

    // 1. Returns tiers array of valid Tier objects in ladder order (letter, effort, model)
    let tiers = out["tiers"]
        .as_array()
        .expect("tiers must be an array of Tier objects");
    assert!(!tiers.is_empty(), "tiers array must not be empty");

    // Ladder order: J (if present) < G < H < S < O, and efforts 0 < 1 < X < 2 < 3 < 4
    let letter_rank = |l: &str| match l {
        "J" => 0,
        "G" => 1,
        "H" => 2,
        "S" => 3,
        "O" => 4,
        other => panic!("unknown letter: {other}"),
    };
    let effort_rank = |val: &Value| {
        if val.is_null() || val.as_str() == Some("X") {
            2
        } else if let Some(e) = val.as_u64() {
            if e < 2 { e as u8 } else { (e + 1) as u8 }
        } else if let Some(s) = val.as_str() {
            let e: u8 = s.parse().expect("valid effort digit or X");
            if e < 2 { e } else { e + 1 }
        } else {
            panic!("unexpected effort value: {val:?}");
        }
    };

    let mut prev_rank: Option<(u8, u8)> = None;
    for item in tiers {
        let tier_str = item["tier"].as_str().expect("tier string e.g. H3");
        let letter = item["letter"].as_str().expect("letter e.g. H");
        let model = item["model"].as_str().expect("model string");
        assert!(!tier_str.is_empty());
        assert!(!letter.is_empty());
        assert!(!model.is_empty());

        let l_rank = letter_rank(letter);
        let e_rank = effort_rank(&item["effort"]);
        let rank = (l_rank, e_rank);

        if let Some(prev) = prev_rank {
            assert!(
                prev < rank,
                "tiers must strictly ascend on the ladder: prev {prev:?}, current {rank:?} ({tier_str})"
            );
        }
        prev_rank = Some(rank);
    }

    // 2. Returns ranges object mapping each role (decide, auxiliar, worker, lead, manager)
    //    to its allowed range (floor, ceiling, allowed rungs list)
    let ranges = out["ranges"]
        .as_object()
        .expect("ranges must be an object mapping roles to range");

    for role in ["decide", "auxiliar", "worker", "lead", "manager"] {
        let r = &ranges[role];
        assert!(
            r.is_object(),
            "role {role} must have a range object in ranges"
        );
        let floor = r["floor"].as_str().expect("floor tier string");
        let ceiling = r["ceiling"].as_str().expect("ceiling tier string");
        assert!(!floor.is_empty());
        assert!(!ceiling.is_empty());

        let allowed = r["allowed"]
            .as_array()
            .or_else(|| r["rungs"].as_array())
            .expect("allowed rungs array");
        assert!(
            !allowed.is_empty(),
            "allowed rungs must not be empty for {role}"
        );
        assert!(
            allowed.iter().any(|v| v.as_str() == Some(floor)),
            "floor {floor} must be in allowed rungs for {role}"
        );
        assert!(
            allowed.iter().any(|v| v.as_str() == Some(ceiling)),
            "ceiling {ceiling} must be in allowed rungs for {role}"
        );
    }
}
