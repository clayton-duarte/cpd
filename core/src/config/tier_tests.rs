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

    #[test]
    fn an_invalid_tier_has_no_model_arg() {
        let err = catalog().model_arg(t("H4")).unwrap_err();
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
