//! The one window copy: a rectangle of one `Buffer` into another at a position, clamped once (audit-plan F3).
//! ratatui has no blit; `Buffer::merge` re-indexes all of `self` on every call, so it is worse here.

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

/// Copy the cells of `src_rect` (clamped to `src`) into `dst` at `at` (clamped to `dst`).
pub(crate) fn copy_cells(src: &Buffer, src_rect: Rect, dst: &mut Buffer, at: Position) {
    let src_rect = src_rect.intersection(src.area);
    let width = src_rect.width.min(dst.area.right().saturating_sub(at.x));
    let height = src_rect.height.min(dst.area.bottom().saturating_sub(at.y));
    for y in 0..height {
        for x in 0..width {
            dst[(at.x + x, at.y + y)] = src[(src_rect.x + x, src_rect.y + y)].clone();
        }
    }
}
