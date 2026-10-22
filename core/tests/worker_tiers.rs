//! The Worker Tier range and ladder (contract §9, hs-d554).

use cpd_core::config::{Catalog, Letter, Tier, TierModel};
use cpd_core::error::ErrorCode;

fn catalog() -> Catalog {
    Catalog {
        pi_version: "1.3.0".into(),
        o: TierModel { model: "p/opus".into(), efforts: vec![1, 2, 3, 4, 5] },
        s: TierModel { model: "p/sonnet".into(), efforts: vec![0, 1, 2, 3, 4, 5] },
        h: TierModel { model: "p/haiku".into(), efforts: vec![0, 1, 2, 3] },
    }
}

fn t(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

#[test]
fn worker_range_allows_only_h3_and_s0_to_s3() {
    for letter in [Letter::O, Letter::S, Letter::H] {
        for effort in 0..=5 {
            let tier = Tier::new(letter, effort).unwrap();
            let expected = matches!(tier.to_string().as_str(), "H3" | "S0" | "S1" | "S2" | "S3");
            assert_eq!(tier.is_worker_allowed(), expected, "{tier}");
            assert_eq!(tier.check_worker().is_ok(), expected, "{tier}");
        }
    }
}

#[test]
fn worker_range_refusal_names_the_range_the_rule_and_hs_d554() {
    for name in ["H2", "S4", "S5", "O3"] {
        let err = t(name).check_worker().unwrap_err();
        assert_eq!(err.code(), ErrorCode::TierNotAllowed, "{name}");
        assert_eq!(err.details()["tier"], name);
        assert_eq!(err.details()["allowed"], "H3, S0-S3");
        assert_eq!(err.details()["rule"], "hs-d554");
        let m = err.message();
        assert!(m.starts_with(&format!("Tier {name} is not allowed for a Worker")), "{m}");
        for part in ["hs-d554", "cost", "H3, S0-S3"] {
            assert!(m.contains(part), "{part} in {m}");
        }
    }
}

#[test]
fn worker_ladder_ends_at_s3() {
    let c = catalog();
    assert_eq!(c.next(t("S0")).unwrap(), Some(t("S1")));
    assert_eq!(c.next(t("S1")).unwrap(), Some(t("S2")));
    assert_eq!(c.next(t("S2")).unwrap(), Some(t("S3")));
    assert_eq!(c.next(t("S3")).unwrap(), None);
}

#[test]
fn worker_ladder_steps_h3_to_s3_and_gives_h0_to_h2_s4_s5_o3_none() {
    let c = catalog();
    assert_eq!(c.next(t("H3")).unwrap(), Some(t("S3")));
    for name in ["H0", "H1", "H2", "S4", "S5", "O3"] {
        assert_eq!(c.next(t(name)).unwrap(), None, "{name}");
    }
}

#[test]
fn worker_attempt_tiers_follow_the_ladder_and_stop_at_s3() {
    let c = catalog();
    let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap();
    assert_eq!([1, 2, 3, 4].map(|n| at("S1", n)), [Some(t("S1")), Some(t("S2")), Some(t("S3")), None]);
    assert_eq!([1, 2, 3].map(|n| at("H3", n)), [Some(t("H3")), Some(t("S3")), None]);
    assert_eq!(at("H2", 2), None);
}
