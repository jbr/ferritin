use super::state::InteractiveState;
use crate::{
    renderer::table_layout::{self, LaidOutCell, span_display_width},
    styled_string::{Span, TableCell},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

impl<'a> InteractiveState<'a> {
    /// Render a table with unicode borders. Long cells wrap onto multiple lines
    /// so each cell stays inside its column.
    pub(super) fn render_table(
        &mut self,
        header: Option<&[TableCell<'a>]>,
        rows: &[Vec<TableCell<'a>>],
        buf: &mut Buffer,
    ) {
        if rows.is_empty() && header.is_none() {
            return;
        }

        let available = (self.layout.area.width as usize)
            .saturating_sub(self.layout.indent as usize)
            .max(20);
        let layout = table_layout::lay_out(header, rows, available);
        if layout.num_cols() == 0 {
            return;
        }

        let border_style = self.theme.muted_style;

        self.draw_border_line(buf, &layout.col_widths, '┌', '┬', '┐', border_style);
        self.layout.pos.y += 1;

        if let Some(header_cells) = layout.header.as_deref() {
            self.draw_row(buf, header_cells, &layout.col_widths, true);
            self.draw_border_line(buf, &layout.col_widths, '├', '┼', '┤', border_style);
            self.layout.pos.y += 1;
        }

        let separate_rows = layout.any_wrapped();
        let last = layout.rows.len().saturating_sub(1);
        for (idx, row) in layout.rows.iter().enumerate() {
            self.draw_row(buf, row, &layout.col_widths, false);
            if separate_rows && idx < last {
                self.draw_blank_row(buf, &layout.col_widths);
            }
        }

        self.draw_border_line(buf, &layout.col_widths, '└', '┴', '┘', border_style);
        // Caller (render_node) advances pos.y past the bottom border.
    }

    /// Draw a single horizontal border line at the current `pos.y` (does not
    /// advance `pos.y` — caller decides).
    fn draw_border_line(
        &self,
        buf: &mut Buffer,
        col_widths: &[usize],
        left: char,
        mid: char,
        right: char,
        border_style: Style,
    ) {
        let y = self.layout.pos.y;
        if y < self.viewport.scroll_offset
            || y >= self.viewport.scroll_offset + self.layout.area.height
        {
            return;
        }
        let mut buf_str = String::new();
        buf_str.push(left);
        for (idx, &w) in col_widths.iter().enumerate() {
            for _ in 0..w {
                buf_str.push('─');
            }
            if idx < col_widths.len() - 1 {
                buf_str.push(mid);
            }
        }
        buf_str.push(right);
        self.write_text(
            buf,
            y,
            self.layout.indent,
            &buf_str,
            self.layout.area,
            border_style,
        );
    }

    /// Draw an empty content row that preserves column borders. Advances `pos.y`.
    fn draw_blank_row(&mut self, buf: &mut Buffer, col_widths: &[usize]) {
        let y = self.layout.pos.y;
        let in_view = y >= self.viewport.scroll_offset
            && y < self.viewport.scroll_offset + self.layout.area.height;
        let border_style = self.theme.muted_style;

        let mut col_pos = self.layout.indent;
        if in_view {
            self.write_text(buf, y, col_pos, "│", self.layout.area, border_style);
        }
        col_pos += 1;
        for &w in col_widths {
            if in_view {
                let pad: String = " ".repeat(w);
                self.write_text(buf, y, col_pos, &pad, self.layout.area, Style::default());
            }
            col_pos += w as u16;
            if in_view {
                self.write_text(buf, y, col_pos, "│", self.layout.area, border_style);
            }
            col_pos += 1;
        }
        self.layout.pos.y += 1;
    }

    /// Draw one logical row of cells across as many physical lines as the
    /// tallest wrapped cell. Advances `pos.y` past the row.
    fn draw_row(
        &mut self,
        buf: &mut Buffer,
        cells: &[LaidOutCell<'a>],
        col_widths: &[usize],
        bold: bool,
    ) {
        let row_height = cells
            .iter()
            .map(|c| c.lines.len().max(1))
            .max()
            .unwrap_or(1);
        let border_style = self.theme.muted_style;

        for line_idx in 0..row_height {
            let y = self.layout.pos.y;
            let in_view = y >= self.viewport.scroll_offset
                && y < self.viewport.scroll_offset + self.layout.area.height;

            // Always traverse to register actions; skip text writes if off-screen.
            let mut col_pos = self.layout.indent;
            if in_view {
                self.write_text(buf, y, col_pos, "│", self.layout.area, border_style);
            }
            col_pos += 1;

            for (col_idx, &width) in col_widths.iter().enumerate() {
                let line: &[Span<'a>] = cells
                    .get(col_idx)
                    .and_then(|c| c.lines.get(line_idx).map(Vec::as_slice))
                    .unwrap_or(&[]);

                let cell_start = col_pos;
                let mut written: usize = 0;
                for span in line {
                    let span_w = span_display_width(span);
                    if in_view {
                        let mut style = self.style(span.style);
                        if bold {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        self.write_text(
                            buf,
                            y,
                            col_pos,
                            span.text.as_ref(),
                            self.layout.area,
                            style,
                        );
                    }
                    if let Some(action) = &span.action {
                        let rect = Rect::new(col_pos, y, span_w as u16, 1);
                        self.render_cache.actions.push((rect, action.clone()));
                    }
                    col_pos += span_w as u16;
                    written += span_w;
                }

                // Pad remainder to column width.
                if in_view && written < width {
                    let pad: String = " ".repeat(width - written);
                    self.write_text(buf, y, col_pos, &pad, self.layout.area, Style::default());
                }
                col_pos = cell_start + width as u16;

                if in_view {
                    self.write_text(buf, y, col_pos, "│", self.layout.area, border_style);
                }
                col_pos += 1;
            }

            self.layout.pos.y += 1;
        }
    }
}
