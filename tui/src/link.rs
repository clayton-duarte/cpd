//! The cockpit's link widget (decision pro-d60): [`Link`] draws a url's cue `󱆭`, or `<text> 󱆭` with the text underlined, as
//! ordinary cells; [`Links`] records where each was drawn so a view can answer a click with [`Links::at`], and [`open_url`]
//! opens the url in the default browser (macOS `open`) through a `Runner`. No cell holds an escape sequence.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::runner::{Call, NO_DEADLINE, Runner};

use crate::{cells, palette};

/// The link cue, drawn where a view links to a page (hs-d191, hs-d235): `nf-md-share_circle` (U+F11AD).
pub const CUE: &str = "\u{F11AD}";

/// A cockpit link: the cue `CUE`, or `<text> CUE`, the whole output the click target (pro-d60).
pub struct Link<'a> {
    url: &'a str,
    text: Option<Line<'a>>,
}

impl<'a> Link<'a> {
    /// The cue only.
    pub fn new(url: &'a str) -> Link<'a> {
        Link { url, text: None }
    }
    /// `<text> CUE`, the text underlined.
    pub fn text(self, text: impl Into<Line<'a>>) -> Link<'a> {
        Link {
            text: Some(text.into()),
            ..self
        }
    }
    /// Cells of the whole output: 1, or the text's width + 2.
    pub fn width(&self) -> u16 {
        match &self.text {
            None => 1,
            Some(line) => u16::try_from(line.width() + 2).unwrap_or(u16::MAX),
        }
    }
}

/// `line` cut to `columns` cells with a final `…`, the way `cells::truncate` cuts a `&str`; spans keep their styles.
fn cut<'a>(line: Line<'a>, columns: usize) -> Vec<Span<'a>> {
    if line.width() <= columns {
        return line.spans;
    }
    let Some(room) = columns.checked_sub(1) else {
        return Vec::new();
    };
    let mut left = room;
    let mut out = Vec::new();
    let mut style = Style::default();
    for span in line.spans {
        style = span.style;
        let taken = cells::take(&span.content, left);
        left -= Span::raw(taken).width();
        let full = taken.len() == span.content.len();
        out.push(Span::styled(taken.to_owned(), span.style));
        if !full {
            break;
        }
    }
    out.push(Span::styled("…", style));
    out
}

impl Widget for Link<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (y, mut x) = (area.y, area.x);
        if let Some(line) = self.text.filter(|_| area.width >= 3) {
            let spans = cut(line, usize::from(area.width - 2));
            for span in spans {
                let span =
                    Span::styled(span.content, span.style.add_modifier(Modifier::UNDERLINED));
                (x, _) = buf.set_span(x, y, &span, area.right() - x);
            }
            // The gap: one plain cell.
            if x < area.right() {
                buf[(x, y)].reset();
                x += 1;
            }
        }
        if x < area.right() {
            buf.set_string(x, y, CUE, palette::clickable_text());
        }
    }
}

/// The (rect, url) of every link drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Links {
    drawn: Vec<(Rect, String)>,
}

impl Links {
    /// Renders `link` into `area` and records the rect it drew with the url.
    pub fn draw(&mut self, link: Link<'_>, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let url = link.url.to_owned();
        let cells = if area.width < 3 {
            1
        } else {
            link.width().min(area.width)
        };
        link.render(area, buf);
        self.drawn.push((Rect::new(area.x, area.y, cells, 1), url));
    }
    /// The url of the link drawn on that cell.
    pub fn at(&self, at: Position) -> Option<&str> {
        self.drawn
            .iter()
            .find(|(rect, _)| rect.contains(at))
            .map(|(_, url)| url.as_str())
    }
}

/// Opens `url` in the default browser (macOS `open`) through `runner`.
pub fn open_url(runner: &dyn Runner, url: &str) -> Result<(), CpdError> {
    let fail =
        |why: String| CpdError::new(ErrorCode::TmuxFailed, format!("open {url} failed: {why}"));
    if !(url.starts_with("http://") || url.starts_with("https://"))
        || !url.bytes().all(|b| (0x21..=0x7e).contains(&b))
    {
        return Err(fail(
            "only http and https urls without spaces or control characters are opened.".to_owned(),
        ));
    }
    let call = Call {
        tool: "open".to_owned(),
        program: "open".to_owned(),
        args: vec![url.to_owned()],
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    match runner.run(&call) {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(fail(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
        Err(e) => Err(fail(format!("{e:?}"))),
    }
}
