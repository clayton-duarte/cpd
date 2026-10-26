# PR links: do the cockpit's bytes leave a link on the wrong PR?

**VERDICT: bytes correct.** Every frame sequence tried leaves the OSC 8 link of every PR row's cue cell (`link::CUE`, U+F11AD, last column of the PRs section) on exactly that row's PR url, and no other cell of the column carries a link. Candidates (a), (b) and (c) are not reproduced; the defect, if it is in what the User sees, is not in the bytes `widgets::column::draw` + ratatui's diff + `link::link` emit. (Candidate (d), tmux, is another Job's.)

Run: 2026-10-26, `timeout 1500 scripts/cargo.sh nextest run -p cpd-tui pr_links_probe` at base `d573afa9`: 6 passed. Unverified: anything about a real terminal (Ghostty, tmux), which this Job does not use.

## Method

The REAL `widgets::column::draw` (not a copy) drawn into one `ratatui::Terminal<CrosstermBackend<Sink>>` (`Viewport::Fixed`) per sequence, several `draw` calls on the same terminal so the diff between frames is real. ALL bytes of each frame, in order, are replayed through a small terminal model (grid of cells; CUP, ED 2, SGR ignored, OSC 8 open/close, each symbol printed at the cursor and advancing by its width; any other escape panics). After each frame, for every `Drawn.rows` entry (every PR on screen, with its `url`): the model's cell (width-1, row.y) is `CUE` and its link is that url; every other cell of the grid has no link; and the model's visible symbols equal ratatui's drawn buffer. Width 30 × height 16 unless stated.

Negative control: replacing `&pr.url` in column.rs by `&shown[0].1.url` (temporarily, reverted) fails 4 of the 6 probes with `cue cell (29,y) is linked wrong`, so the probe can fail. (The two that still pass draw one PR, or the 2-cell form that does not call `draw`.)

## Sequences run (all hold)

| probe | frames (PR numbers top to bottom; scroll; frame = spinner) |
|---|---|
| `reorder_appear_disappear` | 1,2,3 → 3,1,2 → 2,1,3 → 2,9,1,3 (appear in the middle) → 9,1,3 (first gone) → 9,3 (middle gone) → none → 5,6 |
| `scroll_offsets` | 8 PRs, scroll 0,1,2,3,99 (clamped),2,0,1; then 7 PRs (first left) at scroll 1 |
| `active_row_and_spinner_and_clean` | plain; `active` on row 0, 1, 2; all `lead_working` with spinner frame 0..n; #2 `clean` (merge icon); #2 `merging` frames 0-3; all `clean` reversed; active + clean |
| `width_change` | 30 wide → 40 → 9 (`Terminal::resize`, emits a clear) → 30×9 with 5 PRs at scroll 1 |
| `the_same_cue_symbol_with_other_url_is_redrawn` | one row whose text is unchanged but whose url changes #1 → #2: the cue cell is rewritten with the new url |

## Why (b) cannot happen

ratatui-core 0.1.2 `src/buffer/diff.rs`: a `ForcedWidth` cell is emitted when `current != previous`, and `Cell::eq` (cell.rs) compares the full symbol. The link piece's symbol is `ESC]8;;<url>BEL<glyph>ESC]8;;BEL`, so the url is part of the symbol: a cell with the same glyph and another url differs and is redrawn. The piece closes its own link, so a cell written after it never inherits it. The cue's piece is 1 cell wide (`link()` pairs a second cell only when `x + width < end`, and the area is 1 cell), so (c) (a join onto a neighbour) cannot arise in the Deploy column.

## Why (a) cannot happen in `draw`

`draw` takes one `shown` list (`open.iter().skip(win.first).take(win.shown)` zipped with the rows' y) and uses it for the text (`pr_lines`), `rows`/`hits` and the final `link::link` loop (column.rs `for (y, pr) in &shown`). One list, one pairing, one frame; scroll and `active` do not enter it. A wrong link would need `open` itself to hold a url that does not match the row's text, or an `open` that is reordered between the draw and the click (the click handler uses the previous `Drawn`), which are not bytes. Source of the order: `open_pr_rows` in tui/src/read/widgets.rs, `ORDER BY q.created_at, q.repo, q.number`, deterministic. Not probed: the read path (a `url` column that disagrees with `repo`/`number`).

## Plan view and Inbox rows (2-cell pieces)

Same helper, so same verdict, in a smaller form: `two_cell_pieces_titles_rows` links whole text runs (`#1 open`, odd and even lengths, so 2-cell pieces and a 1-cell tail) over 3 rows of one terminal for: 3 rows → reordered → all three rows the same text with other urls → one row blank → all the same url. After each frame, every cell of a row has exactly its row's url and nothing past the text has any. Holds. This calls `link::link` directly (not `plan/boxes.rs` or `tree/inbox.rs`), so it shows the helper is correct for 2-cell pieces, not that those views pair rows with the right url (unverified).

## Not covered

- Real terminal behaviour: how Ghostty or tmux treat OSC 8 on a private-use glyph, or a link on a cell next to one redrawn by the terminal itself (candidate (d)).
- A click handler/opener that resolves the url from a stale `Drawn` or from a different source than the drawn one.
- The data: whether `pull_requests.url` is right for the row.

## Appendix: the probe

Scratch, reverted from the worktree. Put it at `tui/src/widgets/pr_links_probe.rs` with `#[cfg(test)] mod pr_links_probe;` in `tui/src/widgets/mod.rs`; to turn it into a failing test, add the sequence that fails to a `#[test]`.

```rust
//! SCRATCH probe (diagnose-docs-a1): replays every byte the REAL `column::draw` emits through a small terminal model.
use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use cpd_core::ids::SessionId;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::column::{Drawn, Machine, draw};
use super::host::Usage;
use crate::link;
use crate::read::{Checks, OpenPr, ReviewDecision};

#[derive(Clone, Default)]
struct Sink(Rc<RefCell<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The model: a grid of (symbol, link). A wide symbol's second cell is ("", same link).
struct Model {
    w: usize,
    h: usize,
    grid: Vec<(String, Option<String>)>,
}

impl Model {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, grid: vec![(" ".into(), None); w * h] }
    }
    fn resize(&mut self, w: usize, h: usize) {
        *self = Self::new(w, h);
    }
    fn replay(&mut self, bytes: &[u8]) {
        let text = std::str::from_utf8(bytes).unwrap();
        let (mut row, mut col) = (0usize, 0usize);
        let mut link: Option<String> = None;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                match chars.next() {
                    Some('[') => {
                        let mut p = String::new();
                        let last = loop {
                            let c = chars.next().unwrap();
                            if c.is_ascii_alphabetic() {
                                break c;
                            }
                            p.push(c);
                        };
                        match last {
                            'H' => {
                                let mut it = p.split(';');
                                row = it.next().unwrap().parse::<usize>().unwrap() - 1;
                                col = it.next().unwrap().parse::<usize>().unwrap() - 1;
                            }
                            'J' if p == "2" => self.grid.iter_mut().for_each(|c| *c = (" ".into(), None)),
                            'J' => panic!("unmodelled ED {p}"),
                            'K' => panic!("unmodelled EL {p}"),
                            'm' | 'l' | 'h' => {}
                            other => panic!("unmodelled CSI {other} {p}"),
                        }
                    }
                    Some(']') => {
                        let mut s = String::new();
                        loop {
                            match chars.next().unwrap() {
                                '\x07' => break,
                                c => s.push(c),
                            }
                        }
                        let rest = s.strip_prefix("8;;").unwrap_or_else(|| panic!("unmodelled OSC {s}"));
                        link = (!rest.is_empty()).then(|| rest.to_owned());
                    }
                    other => panic!("unmodelled ESC {other:?}"),
                }
            } else {
                let wd = crate::cells::width(&c.to_string());
                if wd == 0 {
                    continue;
                }
                assert!(row < self.h && col < self.w, "print off the grid at ({col},{row})");
                self.grid[row * self.w + col] = (c.to_string(), link.clone());
                for k in 1..wd {
                    if col + k < self.w {
                        self.grid[row * self.w + col + k] = (String::new(), link.clone());
                    }
                }
                col += wd;
            }
        }
    }
    fn link_at(&self, x: usize, y: usize) -> &Option<String> {
        &self.grid[y * self.w + x].1
    }
}

struct Rig {
    terminal: Terminal<CrosstermBackend<Sink>>,
    sink: Sink,
    model: Model,
    w: u16,
    h: u16,
}

impl Rig {
    fn new(w: u16, h: u16) -> Self {
        let sink = Sink::default();
        let terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, h)) },
        )
        .unwrap();
        Self { terminal, sink, model: Model::new(w.into(), h.into()), w, h }
    }
    fn flush(&mut self) -> Vec<u8> {
        let bytes = std::mem::take(&mut *self.sink.0.borrow_mut());
        self.model.replay(&bytes);
        bytes
    }
    fn resize(&mut self, w: u16, h: u16) {
        self.terminal.resize(Rect::new(0, 0, w, h)).unwrap();
        self.model.resize(w.into(), h.into());
        self.w = w;
        self.h = h;
        self.flush(); // the clear bytes; the model starts blank
    }
    /// Draws one frame, replays its bytes, asserts every link; returns the frame's bytes.
    fn frame(&mut self, label: &str, open: &[OpenPr], scroll: usize, spin: usize) -> Vec<u8> {
        let machine = Machine { name: "homeserver".into(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
        let mut out: Option<Drawn> = None;
        let buf = self.terminal.draw(|f| out = Some(draw(f, f.area(), open, &machine, scroll, spin))).unwrap().buffer.clone();
        let bytes = self.flush();
        let d = out.unwrap();
        let x = usize::from(self.w) - 1;
        let mut want: Vec<(usize, String)> = vec![];
        for r in &d.rows {
            // rows carries the url the drawn row pairs with; the number is the pull's
            want.push((r.area.y as usize, r.url.clone()));
        }
        for (y, url) in &want {
            assert_eq!(self.model.link_at(x, *y).as_deref(), Some(url.as_str()), "{label}: cue cell ({x},{y}) is linked wrong; bytes {:?}", String::from_utf8_lossy(&bytes));
            assert_eq!(self.model.grid[y * usize::from(self.w) + x].0, link::CUE, "{label}: cue glyph at ({x},{y})");
        }
        for y in 0..usize::from(self.h) {
            for cx in 0..usize::from(self.w) {
                let allowed = cx == x && want.iter().any(|(wy, _)| *wy == y);
                if !allowed {
                    assert_eq!(self.model.link_at(cx, y), &None, "{label}: stray link at ({cx},{y})");
                }
            }
        }
        // the model's visible text equals the ratatui buffer's, ignoring link wrappers
        for y in 0..self.h {
            for cx in 0..self.w {
                let sym = &buf[(cx, y)];
                let stripped: String = strip(sym.symbol());
                let m = &self.model.grid[usize::from(y) * usize::from(self.w) + usize::from(cx)].0;
                if !m.is_empty() && !stripped.is_empty() {
                    let first = stripped.chars().next().unwrap().to_string();
                    assert_eq!(m, &first, "{label}: text differs at ({cx},{y})");
                }
            }
        }
        bytes
    }
}

fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\x1b]8;;") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 5..];
        rest = &after[after.find('\x07').unwrap() + 1..];
    }
    out.push_str(rest);
    out
}

fn pr(n: i64) -> OpenPr {
    OpenPr {
        repo: "acme/repo".into(),
        number: n,
        url: format!("https://github.com/acme/repo/pull/{n}"),
        title: "A title".into(),
        draft: false,
        checks: Checks::Passed,
        review_decision: ReviewDecision::None,
        conflicting: false,
        update: None,
        clean: false,
        active: false,
        lead: Some(SessionId::parse(&format!("l-{n}")).unwrap()),
        lead_working: false,
        plan_failed: false,
        plan_phase: None,
        merging: false,
    }
}

fn prs(ns: &[i64]) -> Vec<OpenPr> {
    ns.iter().map(|n| pr(*n)).collect()
}

const W: u16 = 30;
const H: u16 = 16;

#[test]
fn reorder_appear_disappear() {
    let mut r = Rig::new(W, H);
    r.frame("initial 1,2,3", &prs(&[1, 2, 3]), 0, 0);
    r.frame("reorder 3,1,2", &prs(&[3, 1, 2]), 0, 0);
    r.frame("reverse 2,1,3", &prs(&[2, 1, 3]), 0, 0);
    r.frame("appear middle 2,9,1,3", &prs(&[2, 9, 1, 3]), 0, 0);
    r.frame("disappear first 9,1,3", &prs(&[9, 1, 3]), 0, 0);
    r.frame("disappear middle 9,3", &prs(&[9, 3]), 0, 0);
    r.frame("empty", &[], 0, 0);
    r.frame("back 5,6", &prs(&[5, 6]), 0, 0);
}

#[test]
fn scroll_offsets() {
    let mut r = Rig::new(W, H);
    let all = prs(&[1, 2, 3, 4, 5, 6, 7, 8]);
    for s in [0, 1, 2, 3, 99, 2, 0, 1] {
        r.frame(&format!("scroll {s}"), &all, s, 0);
    }
    let some = prs(&[2, 3, 4, 5, 6, 7, 8]);
    r.frame("scroll 1 after the first left", &some, 1, 0);
}

#[test]
fn active_row_and_spinner_and_clean() {
    let mut r = Rig::new(W, H);
    let mut v = prs(&[1, 2, 3]);
    r.frame("plain", &v, 0, 0);
    for a in 0..3 {
        v.iter_mut().enumerate().for_each(|(i, p)| p.active = i == a);
        r.frame(&format!("active {a}"), &v, 0, 0);
    }
    v.iter_mut().for_each(|p| p.active = false);
    v.iter_mut().for_each(|p| p.lead_working = true);
    for f in 0..crate::palette::SPINNER.len() {
        r.frame(&format!("spin {f}"), &v, 0, f);
    }
    v[1].clean = true;
    r.frame("clean #2", &v, 0, 3);
    v[1].merging = true;
    for f in 0..4 {
        r.frame(&format!("merging {f}"), &v, 0, f);
    }
    v[0].clean = true;
    v[2].clean = true;
    v.reverse();
    r.frame("all clean reversed", &v, 0, 5);
    v[0].active = true;
    r.frame("active+clean", &v, 0, 5);
}

#[test]
fn width_change() {
    let mut r = Rig::new(W, H);
    let v = prs(&[1, 2, 3]);
    r.frame("w30", &v, 0, 0);
    r.resize(40, H);
    r.frame("w40", &v, 0, 0);
    r.resize(9, H);
    r.frame("w9", &v, 0, 0);
    r.resize(W, 9);
    r.frame("30x9 scrolled", &prs(&[1, 2, 3, 4, 5]), 1, 0);
}

#[test]
fn the_same_cue_symbol_with_other_url_is_redrawn() {
    // candidate (b): a row whose cell text is unchanged but whose PR changes
    let mut r = Rig::new(W, H);
    let mut a = prs(&[1]);
    r.frame("pr 1", &a, 0, 0);
    a[0].url = "https://github.com/acme/repo/pull/2".into();
    let bytes = r.frame("same row text, other url", &a, 0, 0);
    assert!(String::from_utf8_lossy(&bytes).contains("pull/2"));
}

/// Plan boxes / Inbox form: 2-cell pieces of a title over a row, the same text with other urls.
#[test]
fn two_cell_pieces_titles_rows() {
    let w = 24u16;
    let sink = Sink::default();
    let mut t = Terminal::with_options(CrosstermBackend::new(sink.clone()), TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, w, 3)) }).unwrap();
    let mut m = Model::new(w.into(), 3);
    let url = |n: u32| format!("https://github.com/acme/repo/pull/{n}");
    let frames: Vec<Vec<(&str, u32)>> = vec![
        vec![("#1 open", 1), ("#2 open", 2), ("#3 open", 3)],
        vec![("#3 open", 3), ("#1 open", 1), ("#2 open", 2)],
        vec![("#1 open", 9), ("#1 open", 8), ("#1 open", 7)], // same text, other urls
        vec![("#1 open", 7), ("", 0), ("#1 open", 9)],        // one row disappears
        vec![("#1 open", 7), ("#1 open", 7), ("#1 open", 7)],
    ];
    for (i, f) in frames.iter().enumerate() {
        t.draw(|fr| {
            let b = fr.buffer_mut();
            for (y, (text, n)) in f.iter().enumerate() {
                b.set_string(0, y as u16, text, Style::default());
                if !text.is_empty() {
                    // an odd start and a wide-ish run so pieces pair 2 cells: link over the whole text
                    link::link(b, Rect::new(0, y as u16, text.chars().count() as u16, 1), &url(*n));
                }
            }
        })
        .unwrap();
        let bytes = std::mem::take(&mut *sink.0.borrow_mut());
        m.replay(&bytes);
        for (y, (text, n)) in f.iter().enumerate() {
            for x in 0..usize::from(w) {
                let want = (x < text.chars().count()).then(|| url(*n));
                assert_eq!(m.link_at(x, y), &want, "frame {i} ({x},{y})");
            }
        }
    }
}
```
