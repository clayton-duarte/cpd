use super::tier::*;
use crate::error::ErrorCode;

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, efforts: &[u8]) -> TierModel {
        TierModel {
            model: format!("p/{name}"),
            efforts: efforts.to_vec(),
        }
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

    /// pi levels by effort: off=0 low=1 medium=2 high=3 xhigh=4, a catalog with gaps and models without levels.
    fn gappy() -> Catalog {
        Catalog {
            pi_version: "1.5.0".into(),
            o: model("opus", &[]),
            s: model("sonnet", &[0, 1, 3]),
            h: model("haiku", &[1, 3]),
            g: model("gemini", &[]),
        }
    }

    #[test]
    fn syntax_takes_a_letter_and_0_to_4_or_x_and_refuses_effort_5() {
        for good in [
            "O0", "S3", "H4", "G0", "G2", "G4", "GX", "HX", "SX", "OX", "O4",
        ] {
            assert_eq!(t(good).to_string(), good);
        }
        for bad in [
            "", "O", "G", "O5", "H5", "S5", "G5", "S6", "o3", "X1", "XX", "S33", "S-1", " S3",
            "S3 ", "minimal", "gx", "GXX", "SXX",
        ] {
            let err = Tier::parse(bad).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{bad:?}");
        }
    }

    #[test]
    fn a_tier_displays_as_written_g2_and_gx() {
        for written in ["G2", "G0", "GX", "HX", "S4", "OX"] {
            assert_eq!(t(written).to_string(), written);
            assert_eq!(
                t(written).letter().as_char(),
                written.chars().next().unwrap()
            );
        }
        assert_ne!(t("G0"), t("GX"), "G0 and GX are two Tiers");
    }

    #[test]
    fn the_catalog_accepts_x_only_without_levels_and_a_digit_only_when_offered() {
        let c = gappy();
        for ok in ["H1", "H3", "S0", "S1", "S3", "OX", "GX"] {
            assert!(c.is_valid(t(ok)), "{ok}");
            c.check(t(ok)).unwrap();
        }
        for bad in [
            "HX", "SX", "H0", "H2", "H4", "S2", "S4", "O0", "O3", "G0", "G2",
        ] {
            assert!(!c.is_valid(t(bad)), "{bad}");
            let err = c.check(t(bad)).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{bad}");
        }
        // A model that offers 0,2,3,4 (a gap) takes those digits and no X.
        let mut gap = gappy();
        gap.o = model("opus", &[0, 2, 3, 4]);
        for ok in ["O0", "O2", "O3", "O4"] {
            assert!(gap.is_valid(t(ok)), "{ok}");
        }
        for bad in ["O1", "OX"] {
            assert!(!gap.is_valid(t(bad)), "{bad}");
        }
    }

    #[test]
    fn model_arg_of_an_x_tier_uses_the_off_level() {
        let c = gappy();
        assert_eq!(c.model_arg(t("OX")).unwrap(), "p/opus:off");
        assert_eq!(c.model_arg(t("GX")).unwrap(), "p/gemini:off");
        assert_eq!(c.model_arg(t("H3")).unwrap(), "p/haiku:high");
        assert_eq!(c.model_arg(t("S0")).unwrap(), "p/sonnet:off");
        assert_eq!(
            c.model_arg(t("HX")).unwrap_err().code(),
            ErrorCode::TierInvalid
        );
    }

    #[test]
    fn tiers_order_by_the_ladder_g_h_s_o_then_effort_with_x_lowest() {
        let mut tiers: Vec<Tier> = [
            "OX", "S3", "G2", "H4", "GX", "O0", "H0", "SX", "G0", "S1", "O4", "HX",
        ]
        .map(t)
        .to_vec();
        tiers.sort();
        let sorted: Vec<String> = tiers.iter().map(Tier::to_string).collect();
        assert_eq!(
            sorted,
            [
                "GX", "G0", "G2", "HX", "H0", "H4", "SX", "S1", "S3", "OX", "O0", "O4"
            ]
        );
        assert!(t("H4") < t("SX"), "a higher letter beats any effort");
        assert!(t("S4") < t("OX"));
        assert!(t("GX") < t("G0"), "X is the lowest of its letter");
        assert!(t("O3") > t("O2"));
    }

    fn next(from: &str) -> Option<String> {
        catalog().next(t(from)).unwrap().map(|t| t.to_string())
    }

    #[test]
    fn the_contract_examples() {
        assert_eq!(next("S2").as_deref(), Some("S3"));
        assert_eq!(next("H3").as_deref(), Some("S1"));
        // hs-d554: the ladder ends at S3.
        assert_eq!(next("S3"), None);
        assert_eq!(next("S4"), None);
    }

    /// hs-d791: on a gappy catalog the ladder climbs to the next valid Worker-allowed effort within
    /// the letter, not effort+1: H efforts [1,3,5], S efforts [0,1,3,5] (contract §9 "Ladder").
    #[test]
    fn gappy_catalog_next_climbs_to_the_next_valid_worker_effort() {
        let gappy = Catalog {
            pi_version: "1.4.0".into(),
            o: model("opus", &[0, 1, 2, 3, 4, 5]),
            s: model("sonnet", &[0, 1, 3, 4]),
            h: model("haiku", &[1, 3, 4]),
            g: model("gemini", &[]),
        };
        let nx = |from: &str| gappy.next(t(from)).unwrap().map(|t| t.to_string());
        assert_eq!(
            nx("H1").as_deref(),
            Some("H3"),
            "H2 is not a valid effort: the ladder skips to H3, not S1"
        );
        assert_eq!(
            nx("H3").as_deref(),
            Some("S1"),
            "H5 is not a Worker's Tier: the ladder leaves H for S1"
        );
        assert_eq!(
            nx("S1").as_deref(),
            Some("S3"),
            "S2 is not a valid effort: the ladder skips to S3"
        );
        assert_eq!(nx("S0").as_deref(), Some("S1"));
        assert_eq!(
            nx("S3"),
            None,
            "S5 is not a Worker's Tier and no letter is above S"
        );
        // A catalog-VALID Tier outside the Worker range has no ladder (pro-d82); a Tier the
        // catalog refuses is tier_invalid, as an_invalid_tier_is_refused_not_stepped pins.
        for name in ["S4", "O3"] {
            assert_eq!(nx(name), None, "{name} is not a Worker's Tier: no ladder");
        }
    }

    /// hs-d791 leaves the dense ladder alone: the two rules coincide there.
    #[test]
    fn dense_catalog_next_is_unchanged() {
        assert_eq!(next("H0").as_deref(), Some("H1"));
        assert_eq!(next("H1").as_deref(), Some("H2"));
        assert_eq!(next("H2").as_deref(), Some("H3"));
        assert_eq!(next("H3").as_deref(), Some("S1"));
        assert_eq!(next("S1").as_deref(), Some("S2"));
        assert_eq!(next("S2").as_deref(), Some("S3"));
        assert_eq!(next("S3"), None);
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
        assert_eq!(
            catalog().model_arg(t("S0")).unwrap_err().code(),
            ErrorCode::TierInvalid
        );
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
}

#[cfg(test)]
mod worker_tiers {
    //! The Worker Tier range and ladder (contract §9, pro-d82).
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            pi_version: "1.3.0".into(),
            o: TierModel {
                model: "p/opus".into(),
                efforts: vec![1, 2, 3, 4, 5],
            },
            s: TierModel {
                model: "p/sonnet".into(),
                efforts: vec![0, 1, 2, 3, 4, 5],
            },
            g: TierModel {
                model: "p/gemini".into(),
                efforts: vec![],
            },
            h: TierModel {
                model: "p/haiku".into(),
                efforts: vec![0, 1, 2, 3],
            },
        }
    }

    fn t(s: &str) -> Tier {
        Tier::parse(s).unwrap()
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
    fn worker_ladder_steps_h0_to_h3_then_s1_s2_s3_and_gives_s4_o3_none() {
        let c = catalog();
        assert_eq!(c.next(t("H0")).unwrap(), Some(t("H1")));
        assert_eq!(c.next(t("H1")).unwrap(), Some(t("H2")));
        assert_eq!(c.next(t("H2")).unwrap(), Some(t("H3")));
        assert_eq!(c.next(t("H3")).unwrap(), Some(t("S1")));
        assert_eq!(c.next(t("S1")).unwrap(), Some(t("S2")));
        assert_eq!(c.next(t("S2")).unwrap(), Some(t("S3")));
        for name in ["S4", "O3"] {
            assert_eq!(c.next(t(name)).unwrap(), None, "{name}");
        }
    }

    #[test]
    fn worker_attempt_tiers_from_h0_follow_the_ladder_and_stop_at_s3() {
        let c = catalog();
        let at = |job: &str, n| c.attempt_tier(t(job), n).unwrap();
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 7, 8].map(|n| at("H0", n)),
            [
                Some(t("H0")),
                Some(t("H1")),
                Some(t("H2")),
                Some(t("H3")),
                Some(t("S1")),
                Some(t("S2")),
                Some(t("S3")),
                None
            ]
        );
        assert_eq!(
            [1, 2, 3, 4].map(|n| at("S1", n)),
            [Some(t("S1")), Some(t("S2")), Some(t("S3")), None]
        );
    }
}
