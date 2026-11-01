//! The layout rows of the dialog: the frame of each of the four kinds, and the input box nested in it (its wall rect is the
//! dialog's body rect). Widths are the popup's, in a client wider by two margins.

use crate::layout_tests::{Case, PILL_LEFT, PILL_RIGHT, Sample, check, dump, sym};

const WIDTHS: [u16; 7] = [44, 20, 14, 10, 8, 6, 4];

/// What a kind draws: the cases to loop over, the popup's height and the labels of its buttons, the accepting one first.
struct Kind {
    subject: &'static str,
    cases: &'static [Case],
    height: u16,
    title: &'static str,
    buttons: &'static [&'static str],
}

/// The pills of the buttons row `y` between the walls, left to right; the column of the first pill that is cut or
/// overlapped is the error.
fn pills(sample: &Sample, y: u16) -> Result<Vec<String>, u16> {
    let frame = sample.boxes[0].rect;
    let (mut found, mut open) = (Vec::new(), None::<String>);
    for x in frame.x + 1..frame.right() - 1 {
        match (sym(&sample.buf, x, y), open.take()) {
            (PILL_LEFT, None) => open = Some(String::new()),
            (PILL_RIGHT, Some(label)) => found.push(label),
            (PILL_LEFT, Some(_)) | (PILL_RIGHT, None) => return Err(x),
            (s, Some(mut label)) => {
                label.push_str(s);
                open = Some(label);
            }
            (_, None) => {}
        }
    }
    if open.is_some() {
        Err(frame.right() - 1)
    } else {
        Ok(found)
    }
}

fn rows(kind: &Kind) {
    for width in WIDTHS {
        for &case in kind.cases {
            let subject = format!("{} {case:?}", kind.subject);
            let sample = crate::dialog::layout_sample(width, kind.height, case);
            check(&subject, width, &sample);
            let frame = sample.boxes[0].rect;
            let top = frame.y;
            // the title is a pill after `╭─` when it fits (the room is the width less `╭─`, a `─` and `╮`, and the pill's caps)
            if usize::from(width) >= kind.title.chars().count() + 6
                && sym(&sample.buf, frame.x + 2, top) != PILL_LEFT
            {
                panic!(
                    "{subject} at width {width}: the title {:?} fits and is not a pill at ({}, {top})\n{}",
                    kind.title,
                    frame.x + 2,
                    dump(&sample.buf)
                );
            }
            // the buttons: whole or absent, never overlapping; both when they fit, else only the accepting one (A13)
            let y = frame.bottom() - 2;
            let found = pills(&sample, y).unwrap_or_else(|x| {
                panic!(
                    "{subject} at width {width}: a pill is cut or overlapped at ({x}, {y})\n{}",
                    dump(&sample.buf)
                )
            });
            let need = |labels: &[&str]| {
                4 + labels
                    .iter()
                    .map(|l| l.chars().count() as u16 + 2)
                    .sum::<u16>()
                    + labels.len() as u16
                    - 1
            };
            let expected: Vec<&str> = if width >= need(kind.buttons) {
                kind.buttons.to_vec()
            } else if width >= need(&kind.buttons[..1]) {
                kind.buttons[..1].to_vec()
            } else {
                Vec::new()
            };
            assert_eq!(
                found,
                expected,
                "{subject} at width {width}: the buttons row\n{}",
                dump(&sample.buf)
            );
        }
    }
}

#[test]
fn layout_dialog_confirm() {
    rows(&Kind {
        subject: "confirm dialog",
        cases: &[Case::Confirm],
        height: 8,
        title: "Archive",
        buttons: &["Archive", "Cancel"],
    });
}

#[test]
fn layout_dialog_input() {
    rows(&Kind {
        subject: "input dialog",
        cases: &[Case::InputEmpty, Case::InputOne, Case::InputWrapped],
        height: 12,
        title: "New Plan",
        buttons: &["OK", "Cancel"],
    });
}

#[test]
fn layout_dialog_text() {
    rows(&Kind {
        subject: "text dialog",
        cases: &[Case::Text],
        height: 8,
        title: "Legend",
        buttons: &["Close"],
    });
}

#[test]
fn layout_dialog_checklist() {
    rows(&Kind {
        subject: "checklist dialog",
        cases: &[Case::Checklist],
        height: 12,
        title: "Retire records of cpd",
        buttons: &["Approve", "Later"],
    });
}
