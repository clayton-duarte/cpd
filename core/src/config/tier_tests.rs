use super::tier::*;
use crate::error::ErrorCode;

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, efforts: &[u8]) -> TierModel {
        TierModel { model: format!("p/{name}"), efforts: efforts.to_vec() }
    }

    /// O supports 0–5, S 1–5, H 0–3.
    fn catalog() -> Catalog {
        Catalog {
            pi_version: "1.2.0".into(),
            o: model("opus", &[0, 1, 2, 3, 4, 5]),
            s: model("sonnet", &[1, 2, 3, 4, 5]),
            h: model("haiku", &[0, 1, 2, 3]),
            g: model("gemini", &[]),
        }
    }

    fn t(s: &str) -> Tier {
        Tier::parse(s).unwrap()
    }

    fn next(from: &str) -> Option<String> {
        catalog().next(t(from)).unwrap().map(|t| t.to_string())
    }

    #[test]
    fn syntax() {
        for good in ["O0", "S3", "H5", "GX"] {
            assert_eq!(t(good).to_string(), good);
        }
        for bad in ["", "O", "O6", "o3", "X1", "S33", "S-1", " S3", "S3 ", "minimal", "G0", "gx"] {
            let err = Tier::parse(bad).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{bad:?}");
        }
    }

    #[test]
    fn levels_never_use_minimal() {
        let levels: Vec<_> = (0..=5).map(|e| Tier::new(Letter::O, e).unwrap().level()).collect();
        assert_eq!(levels, ["off", "low", "medium", "high", "xhigh", "max"]);
        assert_eq!(effort_of_level("minimal"), Some(None));
        assert_eq!(effort_of_level("xhigh"), Some(Some(4)));
        assert_eq!(effort_of_level("bogus"), None);
    }

    #[test]
    fn the_contract_examples() {
        assert_eq!(next("S2").as_deref(), Some("S3"));
        assert_eq!(next("H3").as_deref(), Some("S1"));
        // hs-d554: the ladder ends at S3.
        assert_eq!(next("S3"), None);
        assert_eq!(next("S4"), None);
    }

    #[test]
    fn leaving_h_starts_s_at_its_lowest_valid_effort_from_s1() {
        let mut c = catalog();
        c.s = model("sonnet", &[2, 3, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S2")));
        c.s = model("sonnet", &[0, 1, 2]);
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S1")));
        c.s = model("sonnet", &[2, 4, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S2")));
        c.s = model("sonnet", &[4, 5]);
        assert_eq!(c.next(t("H3")).unwrap(), None);
    }

    #[test]
    fn an_invalid_tier_is_refused_not_stepped() {
        let err = catalog().next(t("H4")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TierInvalid);
        assert_eq!(err.details()["model"], "p/haiku");
        assert_eq!(catalog().model_arg(t("S0")).unwrap_err().code(), ErrorCode::TierInvalid);
    }

    #[test]
    fn model_arg() {
        assert_eq!(catalog().model_arg(t("S3")).unwrap(), "p/sonnet:high");
        assert_eq!(catalog().model_arg(t("O0")).unwrap(), "p/opus:off");
    }

    #[test]
    fn attempt_tiers_follow_the_ladder_and_stop_when_it_runs_out() {
        let c = catalog();
        let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap().map(|t| t.to_string());
        assert_eq!(at("S1", 1).as_deref(), Some("S1"));
        assert_eq!(at("S1", 2).as_deref(), Some("S2"));
        assert_eq!(at("S1", 3).as_deref(), Some("S3"));
        assert_eq!(at("S1", 4), None);
        assert_eq!(at("H3", 2).as_deref(), Some("S1"));
        assert_eq!(at("H3", 5), None);
        assert_eq!(at("H3", 9), None);
    }
}

#[cfg(test)]
mod tier_g {
    //! Tier G, written `GX` (contract §9 "Tier rules", hs-d562): the syntax, and G as no Worker's Tier.
    use super::*;

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
        assert_eq!(err.details()["allowed"], "H0-H3, S0-S3");
        assert_eq!(err.details()["rule"], "pro-d82");
    }
}

#[cfg(test)]
mod worker_tiers {
    //! The Worker Tier range and ladder (contract §9, pro-d82).
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            pi_version: "1.3.0".into(),
            o: TierModel { model: "p/opus".into(), efforts: vec![1, 2, 3, 4, 5] },
            s: TierModel { model: "p/sonnet".into(), efforts: vec![0, 1, 2, 3, 4, 5] },
            g: TierModel { model: "p/gemini".into(), efforts: vec![] },
            h: TierModel { model: "p/haiku".into(), efforts: vec![0, 1, 2, 3] },
        }
    }

    fn t(s: &str) -> Tier {
        Tier::parse(s).unwrap()
    }

    #[test]
    fn worker_range_allows_h0_to_h3_and_s0_to_s3() {
        for letter in [Letter::O, Letter::S, Letter::H] {
            for effort in 0..=5 {
                let tier = Tier::new(letter, effort).unwrap();
                let expected = matches!(tier.to_string().as_str(), "H0" | "H1" | "H2" | "H3" | "S0" | "S1" | "S2" | "S3");
                assert_eq!(tier.is_worker_allowed(), expected, "{tier}");
                assert_eq!(tier.check_worker().is_ok(), expected, "{tier}");
            }
        }
    }

    #[test]
    fn worker_range_refusal_names_the_range_the_rule_and_pro_d82() {
        for name in ["H4", "H5", "S4", "S5", "O3"] {
            let err = t(name).check_worker().unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierNotAllowed, "{name}");
            assert_eq!(err.details()["tier"], name);
            assert_eq!(err.details()["allowed"], "H0-H3, S0-S3");
            assert_eq!(err.details()["rule"], "pro-d82");
            let m = err.message();
            assert!(m.starts_with(&format!("Tier {name} is not allowed for a Worker")), "{m}");
            for part in ["pro-d82", "hs-d554", "cost", "H0-H3, S0-S3"] {
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
    fn worker_ladder_steps_h0_to_h3_then_s1_s2_s3_and_gives_s4_s5_o3_none() {
        let c = catalog();
        assert_eq!(c.next(t("H0")).unwrap(), Some(t("H1")));
        assert_eq!(c.next(t("H1")).unwrap(), Some(t("H2")));
        assert_eq!(c.next(t("H2")).unwrap(), Some(t("H3")));
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S1")));
        assert_eq!(c.next(t("S1")).unwrap(), Some(t("S2")));
        assert_eq!(c.next(t("S2")).unwrap(), Some(t("S3")));
        for name in ["S4", "S5", "O3"] {
            assert_eq!(c.next(t(name)).unwrap(), None, "{name}");
        }
    }

    #[test]
    fn worker_attempt_tiers_from_h0_follow_the_ladder_and_stop_at_s3() {
        let c = catalog();
        let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap();
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 7, 8].map(|n| at("H0", n)),
            [Some(t("H0")), Some(t("H1")), Some(t("H2")), Some(t("H3")), Some(t("S1")), Some(t("S2")), Some(t("S3")), None]
        );
        assert_eq!([1, 2, 3, 4].map(|n| at("S1", n)), [Some(t("S1")), Some(t("S2")), Some(t("S3")), None]);
    }
}
