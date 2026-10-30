//! Red tests for the engine-fed status texts of contract §6.1.1 (hs-d740, hs-d741): `workers Q·W·N/M`, queued in colour 1, waiting in
//! colour 3 and running in colour 2, each a coloured number on the pill's colour-8 block; the rest is drawn as the pill draws its text.

use super::workers::Counts;

#[test]
fn workers_texts_are_the_traffic_light_queued_waiting_running() {
    let a = Counts { running: 1, limit: 5, waiting: 0, queued: 5 };
    assert_eq!(a.long(), "workers #[noreverse]#[fg=colour1]#[bg=colour8]5#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour3]#[bg=colour8]0#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour2]#[bg=colour8]1#[reverse]#[fg=colour8]#[bg=default]/5");
    assert_eq!(a.short(), "#[noreverse]#[fg=colour1]#[bg=colour8]5#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour3]#[bg=colour8]0#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour2]#[bg=colour8]1#[reverse]#[fg=colour8]#[bg=default]/5");
    let b = Counts { running: 12, limit: 16, waiting: 103, queued: 1000 };
    assert_eq!(b.long(), "workers #[noreverse]#[fg=colour1]#[bg=colour8]1000#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour3]#[bg=colour8]103#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour2]#[bg=colour8]12#[reverse]#[fg=colour8]#[bg=default]/16");
    assert_eq!(b.short(), "#[noreverse]#[fg=colour1]#[bg=colour8]1000#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour3]#[bg=colour8]103#[reverse]#[fg=colour8]#[bg=default]·#[noreverse]#[fg=colour2]#[bg=colour8]12#[reverse]#[fg=colour8]#[bg=default]/16");
}

/// Splits `text` into its `#[…]` directives (`Err`) and plain runs (`Ok`), in order.
fn parts(text: &str) -> Vec<Result<&str, &str>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("#[") {
        if at > 0 {
            out.push(Ok(&rest[..at]));
        }
        let end = rest[at..].find(']').expect("a style closes") + at;
        out.push(Err(&rest[at..=end]));
        rest = &rest[end + 1..];
    }
    if !rest.is_empty() {
        out.push(Ok(rest));
    }
    out
}

#[test]
fn each_workers_number_is_its_own_base_colour_and_the_rest_keeps_the_pills_style() {
    let counts = Counts { running: 12, limit: 16, waiting: 0, queued: 1000 };
    for (what, text, label) in [("long", counts.long(), "workers "), ("short", counts.short(), "")] {
        for bad in ["#[default]", "dim", "colour7", "colour9", "colour10", "colour11", "colour12", "colour13", "colour14", "colour15"] {
            assert!(!text.contains(bad), "{what}: {text:?} contains {bad}");
        }
        let mut expected: Vec<Result<String, String>> = Vec::new();
        if !label.is_empty() {
            expected.push(Ok(label.to_string()));
        }
        for (i, (colour, number)) in [(1, "1000"), (3, "0"), (2, "12")].into_iter().enumerate() {
            if i > 0 {
                expected.push(Ok("·".to_string()));
            }
            expected.push(Err("#[noreverse]".to_string()));
            expected.push(Err(format!("#[fg=colour{colour}]")));
            expected.push(Err("#[bg=colour8]".to_string()));
            expected.push(Ok(number.to_string()));
            expected.push(Err("#[reverse]".to_string()));
            expected.push(Err("#[fg=colour8]".to_string()));
            expected.push(Err("#[bg=default]".to_string()));
        }
        expected.push(Ok("/16".to_string()));
        let got: Vec<Result<String, String>> = parts(&text).into_iter().map(|p| p.map(str::to_string).map_err(str::to_string)).collect();
        assert_eq!(got, expected, "{what}: {text:?}");
    }
}

/// Display columns as tmux's format_width counts them: a closed `#[…]` is 0 cells, everything else one cell per char.
fn columns(text: &str) -> usize {
    parts(text).into_iter().map(|p| p.map_or(0, |run| run.chars().count())).sum()
}

#[test]
fn workers_texts_count_display_columns_with_no_cell_for_a_style() {
    let wide = Counts { running: 6, limit: 6, waiting: 4, queued: 9 };
    assert_eq!((columns(&wide.long()), columns(&wide.short())), (15, 7));
    let zero = Counts { running: 0, limit: 3, waiting: 0, queued: 1 };
    assert_eq!((columns(&zero.long()), columns(&zero.short())), (15, 7));
}
