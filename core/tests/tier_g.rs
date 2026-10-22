//! Tier G, written `GX` (contract §9 "Tier rules", hs-d562): the syntax, and G as no Worker's Tier.

use cpd_core::config::Tier;
use cpd_core::error::ErrorCode;

#[test]
fn g_is_a_tier_written_gx() {
    let g = Tier::parse("GX").unwrap();
    assert_eq!(g.to_string(), "GX");
    assert_eq!(g.letter().as_char(), 'G');
    assert_eq!(g.level(), "off");
}

#[test]
fn g_has_no_number() {
    for n in 0..=5 {
        let name = format!("G{n}");
        let err = Tier::parse(&name).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TierInvalid, "{name}");
        let m = err.message();
        assert!(m.contains("GX") && m.contains("hs-d562"), "{name}: {m}");
    }
    for name in ["gx", "G", "GXX", "OX", "SX", "HX"] {
        assert_eq!(Tier::parse(name).unwrap_err().code(), ErrorCode::TierInvalid, "{name}");
    }
    for name in ["H5", "S3", "O1"] {
        assert_eq!(Tier::parse(name).unwrap().to_string(), name);
    }
}

#[test]
fn gx_is_never_a_workers_tier() {
    let g = Tier::parse("GX").unwrap();
    assert!(!g.is_worker_allowed());
    let err = g.check_worker().unwrap_err();
    assert_eq!(err.code(), ErrorCode::TierNotAllowed);
    assert_eq!(err.details()["allowed"], "H3, S0-S3");
    assert_eq!(err.details()["rule"], "hs-d554");
}
