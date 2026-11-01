//! Red tests of the box widget's geometry and rendering.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::box_widget::{BoxWidget, Walls, geometry, spacing};
use crate::palette;

/// A body: a row of `x` over its whole area.
struct Xs;

impl Widget for Xs {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                buf[(x, y)].set_symbol("x");
            }
        }
    }
}

fn sym(buf: &Buffer, x: u16, y: u16) -> String {
    buf[(x, y)].symbol().to_string()
}

fn row(buf: &Buffer, y: u16, x0: u16, x1: u16) -> String {
    (x0..x1).map(|x| sym(buf, x, y)).collect()
}

/// A cell painted as `sel`: its fg and the REVERSED modifier (`bg` is `Reset` either way).
fn lit(buf: &Buffer, x: u16, y: u16, sel: Style) -> bool {
    let st = buf[(x, y)].style();
    st.fg == sel.fg && st.add_modifier.contains(Modifier::REVERSED)
}

fn full() -> Walls<'static> {
    Walls::Full
}

fn rail() -> Walls<'static> {
    Walls::Rail {
        closing: Line::from(Span::styled("done", palette::muted_text())),
    }
}

/// Draw into a buffer one cell larger than `area` all round.
fn draw(widget: BoxWidget<'_, Xs>, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(
        0,
        0,
        area.x + area.width + 1,
        area.y + area.height + 1,
    ));
    widget.render(area, &mut buf);
    buf
}

#[test]
fn box_geometry_full_has_a_wall_a_pad_column_and_no_vertical_padding() {
    for w in [5u16, 6, 44] {
        for h in [2u16, 3, 9] {
            let area = Rect::new(3, 2, w, h);
            let g = geometry(&full(), area);
            assert_eq!(g.wall, area, "{w}x{h}");
            assert_eq!(g.title_row, 2, "{w}x{h}");
            assert_eq!(g.closing_row, None, "{w}x{h}");
            assert_eq!(
                g.inner,
                Rect::new(3 + 1 + spacing::PAD_X, 3, w - 4, h - 2),
                "{w}x{h}"
            );
        }
    }
}

#[test]
fn box_geometry_rail_has_blank_rows_a_title_row_and_a_closing_row_and_ends_content_four_columns_short()
 {
    let area = Rect::new(3, 2, 40, 9);
    let g = geometry(&rail(), area);
    assert_eq!(g.wall, area);
    assert_eq!(g.title_row, 3);
    assert_eq!(g.closing_row, Some(2 + 9 - 2));
    assert_eq!(g.inner, Rect::new(5, 4, 36, 5));
    assert_eq!(spacing::RIGHT_PAD, 2);
    // Content ends at column W-4 of a pane the container passed through `contain`.
    let pane = Rect::new(0, 0, 40, 9);
    let g = geometry(&rail(), spacing::contain(pane));
    assert_eq!(
        g.inner.x + g.inner.width,
        pane.width - spacing::RIGHT_PAD - spacing::MARGIN_X
    );
}

#[test]
fn box_geometry_below_the_minimum_width_has_an_empty_inner() {
    assert_eq!(spacing::MIN_WIDTH, 5);
    assert_eq!(geometry(&full(), Rect::new(1, 1, 4, 9)).inner, Rect::ZERO);
    assert_eq!(geometry(&rail(), Rect::new(1, 1, 4, 9)).inner, Rect::ZERO);
    assert_eq!(geometry(&full(), Rect::new(1, 1, 20, 1)).inner, Rect::ZERO);
    assert_eq!(geometry(&rail(), Rect::new(1, 1, 20, 3)).inner, Rect::ZERO);
    assert_ne!(geometry(&full(), Rect::new(1, 1, 5, 3)).inner, Rect::ZERO);
    assert_ne!(geometry(&rail(), Rect::new(1, 1, 5, 5)).inner, Rect::ZERO);
}

#[test]
fn box_full_draws_rounded_walls_the_title_after_the_corner_and_a_line_and_the_body_inside_the_padding()
 {
    let border = palette::input_border(true);
    let title = Line::from(vec![
        Span::raw("ab"),
        Span::styled("cd", palette::muted_text()),
    ]);
    let area = Rect::new(1, 1, 12, 4);
    let buf = draw(BoxWidget::new(full(), border, Xs).title(title), area);
    assert_eq!(row(&buf, 1, 1, 13), "╭─abcd─────╮");
    assert_eq!(buf[(1, 1)].style().fg, border.fg);
    assert_eq!(buf[(5, 1)].style().fg, palette::muted_text().fg);
    for y in [2u16, 3] {
        assert_eq!(sym(&buf, 1, y), "│");
        assert_eq!(sym(&buf, 12, y), "│");
        assert_eq!(sym(&buf, 2, y), " ");
        assert_eq!(sym(&buf, 11, y), " ");
        assert_eq!(row(&buf, y, 3, 11), "xxxxxxxx");
        assert_eq!(buf[(1, y)].style().fg, border.fg);
    }
    assert_eq!(row(&buf, 4, 1, 13), "╰──────────╯");
}

#[test]
fn box_rail_draws_the_corner_the_rail_the_closing_row_and_leaves_the_right_wall_column_blank() {
    let border = palette::input_border(true);
    let area = Rect::new(1, 1, 12, 6);
    let title = Line::from("head");
    let buf = draw(BoxWidget::new(rail(), border, Xs).title(title), area);
    // Blank row above, title row, two body rows, closing row, blank row below.
    assert_eq!(row(&buf, 1, 1, 13).trim(), "");
    assert_eq!(row(&buf, 2, 1, 6), "╭head");
    assert_eq!(sym(&buf, 12, 2), " ");
    for y in [3u16, 4] {
        assert_eq!(sym(&buf, 1, y), "│");
        assert_eq!(buf[(1, y)].style().fg, border.fg);
        assert_eq!(row(&buf, y, 3, 11), "xxxxxxxx");
        assert_eq!(sym(&buf, 11, y), " ");
        assert_eq!(sym(&buf, 12, y), " ");
    }
    assert_eq!(row(&buf, 5, 1, 6), "╰done");
    assert_eq!(buf[(2, 5)].style().fg, palette::muted_text().fg);
    assert_eq!(sym(&buf, 12, 5), " ");
    assert_eq!(row(&buf, 6, 1, 13).trim(), "");
}

#[test]
fn box_highlight_paints_the_body_rows_between_the_walls_padding_included_and_a_rails_whole_row() {
    let border = palette::input_border(true);
    let sel = palette::selected();
    // Full: body row 1 (the second) only, between the walls.
    let buf = draw(
        BoxWidget::new(full(), border, Xs).highlight(1..2),
        Rect::new(1, 1, 10, 5),
    );
    for x in 2..10 {
        assert!(lit(&buf, x, 3, sel), "x={x}");
        assert!(!lit(&buf, x, 2, sel), "x={x}");
        assert!(!lit(&buf, x, 4, sel), "x={x}");
    }
    assert!(!lit(&buf, 1, 3, sel));
    assert!(!lit(&buf, 10, 3, sel));
    // Rail: the whole row of the area, rail column included.
    let area = Rect::new(1, 1, 10, 6);
    let buf = draw(BoxWidget::new(rail(), border, Xs).highlight(0..1), area);
    for x in 1..11 {
        assert!(lit(&buf, x, 3, sel), "x={x}");
        assert!(!lit(&buf, x, 4, sel), "x={x}");
    }
}

#[test]
fn box_below_the_minimum_width_writes_nothing() {
    let area = Rect::new(1, 1, 4, 6);
    for walls in [full(), rail()] {
        let buf = draw(
            BoxWidget::new(walls, Style::default(), Xs).title(Line::from("t")),
            area,
        );
        assert_eq!(buf, Buffer::empty(buf.area));
    }
}

#[test]
fn box_never_writes_outside_its_area() {
    let area = Rect::new(2, 2, 9, 6);
    for walls in [full(), rail()] {
        let buf = draw(
            BoxWidget::new(walls, palette::input_border(true), Xs)
                .title(Line::from("a title far too long for the room"))
                .highlight(0..9),
            area,
        );
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let inside = x >= area.x
                    && x < area.x + area.width
                    && y >= area.y
                    && y < area.y + area.height;
                if !inside {
                    assert_eq!(
                        buf[(x, y)],
                        ratatui::buffer::Cell::default(),
                        "stray write at {x},{y}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_box_nested_in_a_box_inner_sits_one_padding_column_from_the_parents_wall() {
    let parent = Rect::new(0, 0, 30, 8);
    let outer = geometry(&full(), parent);
    let child = geometry(&full(), outer.inner);
    assert_eq!(child.wall.x - (parent.x + 1), spacing::PAD_X);
    assert_eq!(
        (parent.x + parent.width - 1) - (child.wall.x + child.wall.width - 1) - 1,
        spacing::PAD_X
    );
    let buf = draw(BoxWidget::new(full(), Style::default(), Xs), parent);
    assert_eq!(sym(&buf, 0, 2), "│");
    assert_eq!(sym(&buf, 1, 2), " ");
}

#[test]
fn spacing_contain_takes_the_margin_off_each_side() {
    assert_eq!(
        spacing::contain(Rect::new(0, 0, 10, 4)),
        Rect::new(1, 0, 8, 4)
    );
}
