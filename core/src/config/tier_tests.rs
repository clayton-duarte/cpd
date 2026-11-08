use super::tier::*;
use crate::error::ErrorCode;

/// A catalog whose `J` model (`openrouter/typesafe/jev-router`) offers efforts 0, 1, 2 and 3.
fn letter_catalog() -> Catalog {
    let model = |name: &str, efforts: &[u8]| TierModel {
        model: format!("p/{name}"),
        efforts: efforts.to_vec(),
    };
    Catalog {
        pi_version: "1.0.0".into(),
        o: model("opus", &[0, 1, 2, 3, 4]),
        s: model("sonnet", &[1, 2, 3]),
        h: model("haiku", &[1, 3]),
        g: model("gemini", &[]),
        j: model("jev-router", &[0, 1, 2, 3]),
    }
}

/// J is the fifth Tier letter (contract §9 "Syntax"): `JX` parses and round-trips as `JX`; `J4` is within the
/// syntax `<Letter><0-4|X>` but J's model offers no effort 4, so the catalog refuses it; the parse error names
/// the letters in ladder order `J < G < H < S < O`.
#[test]
fn letter_j_parses_and_round_trips() {
    let jx = Tier::parse("JX").expect("JX parses");
    assert_eq!(jx.to_string(), "JX");
    assert_eq!(jx.letter().as_char(), 'J');
    for written in ["J0", "J1", "J2", "J3"] {
        assert_eq!(
            Tier::parse(written).expect("a J effort parses").to_string(),
            written
        );
    }
    let catalog = letter_catalog();
    let j4 = Tier::parse("J4").expect("J4 is within the syntax <Letter><0-4|X>");
    assert!(
        !catalog.is_valid(j4),
        "J's model offers efforts 0, 1, 2 and 3"
    );
    assert_eq!(
        catalog.check(j4).unwrap_err().code(),
        ErrorCode::TierInvalid
    );
    // `Z` is neither a letter nor an effort, so the message names only the syntax's letters.
    let err = Tier::parse("Z").unwrap_err();
    assert_eq!(err.code(), ErrorCode::TierInvalid);
    let hint = err.message();
    let at = |ch: char| {
        hint.find(ch)
            .unwrap_or_else(|| panic!("the parse hint names {ch}: {hint}"))
    };
    assert!(
        at('J') < at('G') && at('G') < at('H') && at('H') < at('S') && at('S') < at('O'),
        "the letters are in ladder order J < G < H < S < O: {hint}"
    );
}

/// J ranks below G (contract §9 "Ladder order"): every J rung is below every G rung, and within J
/// effort 0 < X < 1.
#[test]
fn letter_rank_puts_j_below_g() {
    assert!(
        Tier::parse("JX").unwrap() < Tier::parse("G0").unwrap(),
        "JX < G0"
    );
    assert!(
        Tier::parse("J4").unwrap() < Tier::parse("G0").unwrap(),
        "a J effort is below every G rung"
    );
    let mut tiers: Vec<Tier> = ["GX", "G0", "JX", "J0", "J1"]
        .map(|s| Tier::parse(s).unwrap())
        .to_vec();
    tiers.sort();
    let sorted: Vec<String> = tiers.iter().map(Tier::to_string).collect();
    assert_eq!(sorted, ["J0", "JX", "J1", "G0", "GX"]);
}

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
            j: crate::config::TierModel {
                model: String::new(),
                efforts: Vec::new(),
            },
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
            j: crate::config::TierModel {
                model: String::new(),
                efforts: Vec::new(),
            },
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
    fn the_catalog_accepts_x_on_every_model_and_a_digit_only_when_offered() {
        let c = gappy();
        for ok in ["H1", "H3", "S0", "S1", "S3", "HX", "SX", "OX", "GX"] {
            assert!(c.is_valid(t(ok)), "{ok}");
            c.check(t(ok)).unwrap();
        }
        for bad in ["H0", "H2", "H4", "S2", "S4", "O0", "O3", "G0", "G2"] {
            assert!(!c.is_valid(t(bad)), "{bad}");
            let err = c.check(t(bad)).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TierInvalid, "{bad}");
        }
        // A model that offers 0,2,3,4 (a gap) takes those digits and X.
        let mut gap = gappy();
        gap.o = model("opus", &[0, 2, 3, 4]);
        for ok in ["O0", "O2", "O3", "O4", "OX"] {
            assert!(gap.is_valid(t(ok)), "{ok}");
        }
        assert!(!gap.is_valid(t("O1")));
    }

    /// R2: `<L>X` is valid on EVERY model, also one that offers efforts; a digit only where offered; effort 5 and a
    /// syntax error stay `tier_invalid`.
    #[test]
    fn tier_x_is_valid_on_a_model_with_efforts() {
        let mut c = gappy();
        c.g = model("gemini", &[1, 3]);
        for letter in ["G", "H", "S", "O"] {
            let x = t(&format!("{letter}X"));
            assert!(c.is_valid(x), "{x}");
            c.check(x).unwrap();
        }
        for ok in ["G1", "G3"] {
            assert!(c.is_valid(t(ok)), "{ok}");
        }
        for bad in ["G0", "G2", "G4"] {
            assert_eq!(
                c.check(t(bad)).unwrap_err().code(),
                ErrorCode::TierInvalid,
                "{bad}"
            );
        }
        for bad in ["G5", "X5", "GX5", "5X"] {
            assert_eq!(
                Tier::parse(bad).unwrap_err().code(),
                ErrorCode::TierInvalid,
                "{bad}"
            );
        }
    }

    /// R1: within a letter 0 < X < 1 < 2 < 3 < 4, and a letter ranks above every effort of the one before.
    #[test]
    fn tier_x_orders_between_effort_0_and_1() {
        for letter in ["G", "H", "S", "O"] {
            let (e0, x, e1) = (
                t(&format!("{letter}0")),
                t(&format!("{letter}X")),
                t(&format!("{letter}1")),
            );
            assert!(e0 < x, "{e0} < {x}");
            assert!(x < e1, "{x} < {e1}");
            assert!(x < t(&format!("{letter}4")));
        }
        assert!(t("G4") < t("HX"), "X sits below the next letter");
        assert!(t("H4") < t("SX"));
        assert!(t("S4") < t("OX"));
        assert!(t("GX") > t("G0") && t("HX") > t("H0"));
        let mut tiers: Vec<Tier> = ["G1", "GX", "G0", "H2", "HX", "H0"].map(t).to_vec();
        tiers.sort();
        let sorted: Vec<String> = tiers.iter().map(Tier::to_string).collect();
        assert_eq!(sorted, ["G0", "GX", "G1", "H0", "HX", "H2"]);
    }

    /// R3: X sends no effort, so pi's model argument is the bare model, on a model with efforts and on one with none;
    /// a digit gives `model:level`.
    #[test]
    fn tier_x_model_arg_has_no_level_suffix() {
        let c = gappy();
        // A model with no efforts (O, G) and one with efforts (H: 1,3; S: 0,1,3).
        assert_eq!(c.model_arg(t("OX")).unwrap(), "p/opus");
        assert_eq!(c.model_arg(t("GX")).unwrap(), "p/gemini");
        assert_eq!(c.model_arg(t("HX")).unwrap(), "p/haiku");
        assert_eq!(c.model_arg(t("SX")).unwrap(), "p/sonnet");
        for x in ["OX", "GX", "HX", "SX"] {
            assert!(!c.model_arg(t(x)).unwrap().contains(':'), "{x}");
        }
        assert_eq!(c.model_arg(t("H3")).unwrap(), "p/haiku:high");
        assert_eq!(c.model_arg(t("H1")).unwrap(), "p/haiku:low");
        assert_eq!(c.model_arg(t("S0")).unwrap(), "p/sonnet:off");
        assert_eq!(
            c.model_arg(t("H0")).unwrap_err().code(),
            ErrorCode::TierInvalid
        );
    }

    /// R4: next never lands on X on a model with efforts: from `<L>X` it goes to the lowest offered effort >= 1, from
    /// effort 0 too, and leaving a letter lands on the lowest effort >= 1; a model with no efforts keeps X as its rung.
    #[test]
    fn tier_x_next_climbs_to_the_lowest_effort_from_1() {
        let mut c = gappy();
        c.g = model("gemini", &[0, 1, 3]);
        c.h = model("haiku", &[0, 2, 3]);
        c.s = model("sonnet", &[0, 1, 3]);
        let all = Range {
            floor: t("GX"),
            ceiling: t("OX"),
        };
        assert_eq!(c.next_in(all, t("GX")), Some(t("G1")));
        assert_eq!(c.next_in(all, t("G0")), Some(t("G1")));
        assert_eq!(
            c.next_in(all, t("G3")),
            Some(t("H2")),
            "leaving G: H's lowest effort >= 1"
        );
        assert_eq!(c.next_in(all, t("HX")), Some(t("H2")));
        assert_eq!(c.next_in(all, t("H3")), Some(t("S1")));
        assert_eq!(
            c.next_in(all, t("S3")),
            Some(t("OX")),
            "O offers no efforts: X is its rung"
        );
        // The whole climb: X only where the model has no efforts.
        let mut walk = vec![t("GX")];
        while let Some(next) = c.next_in(all, *walk.last().unwrap()) {
            walk.push(next);
            assert!(walk.len() < 20, "the climb ends");
        }
        let names: Vec<String> = walk.iter().map(Tier::to_string).collect();
        assert_eq!(names, ["GX", "G1", "G3", "H2", "H3", "S1", "S3", "OX"]);
    }

    #[test]
    fn tiers_order_by_the_ladder_g_h_s_o_then_effort_with_x_between_0_and_1() {
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
                "G0", "GX", "G2", "H0", "HX", "H4", "SX", "S1", "S3", "O0", "OX", "O4"
            ]
        );
        assert!(t("H4") < t("SX"), "a higher letter beats any effort");
        assert!(t("S4") < t("OX"));
        assert!(t("G0") < t("GX"), "X is above effort 0 and below effort 1");
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
