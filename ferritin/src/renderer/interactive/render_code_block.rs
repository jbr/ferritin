use ratatui::{
    buffer::Buffer,
    style::{Color, Style},
};
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

use super::state::InteractiveState;

// Code block borders are outdented to the left of content so that the code text
// aligns with surrounding prose, and the border is purely decorative.
const CODE_BLOCK_BORDER_WIDTH: u16 = 2; // "│ " takes 2 columns
const CODE_BLOCK_BORDER_OUTDENT: i16 = -2; // Draw border 2 columns left of content

impl<'a> InteractiveState<'a> {
    /// Render code block with syntax highlighting
    pub(super) fn render_code_block(&mut self, lang: Option<&str>, code: &str, buf: &mut Buffer) {
        let lang_display = match lang {
            Some("no_run") | Some("should_panic") | Some("ignore") | Some("compile_fail")
            | Some("edition2015") | Some("edition2018") | Some("edition2021")
            | Some("edition2024") => "rust",
            Some(l) => l,
            None => "rust",
        };

        // Border is outdented (to the left of content) so code text aligns with surrounding text
        let border_col = self
            .layout
            .indent
            .saturating_add_signed(CODE_BLOCK_BORDER_OUTDENT);
        let content_col = self.layout.indent; // Code content stays at indent

        // Calculate code block dimensions accounting for content position
        let available_width = self.layout.area.width.saturating_sub(content_col);
        let max_line_width = code
            .lines()
            .map(|line| line.len())
            .max()
            .unwrap_or(0)
            .min((available_width.saturating_sub(4)) as usize); // Leave room for border and padding

        // Account for language label in border width: ╭───❬rust❭─╮
        let lang_label = format!("❬{}❭", lang_display);
        // Count actual display width (number of grapheme clusters, not bytes)
        let label_display_width = lang_label.chars().count();
        let min_border_for_label = label_display_width as u16 + 6; // label + some padding
        let border_width = ((max_line_width + 4).max(min_border_for_label as usize))
            .min(available_width as usize) as u16;

        let border_style = self.theme.code_block_border_style;

        // Top border with language label: ╭─────❬rust❭─╮
        if self.layout.pos.y >= self.viewport.scroll_offset
            && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
        {
            self.write_text(
                buf,
                self.layout.pos.y,
                border_col,
                "╭",
                self.layout.area,
                border_style,
            );

            // Calculate position for language label (right side, with one dash before corner)
            // Label ends at border_width - 2 (leaving space for ─╮)
            let label_start =
                border_col + border_width.saturating_sub(label_display_width as u16 + 2);

            // Draw left dashes (up to label)
            for i in 1..label_start.saturating_sub(border_col) {
                self.write_text(
                    buf,
                    self.layout.pos.y,
                    border_col + i,
                    "─",
                    self.layout.area,
                    border_style,
                );
            }

            // Draw language label
            self.write_text(
                buf,
                self.layout.pos.y,
                label_start,
                &lang_label,
                self.layout.area,
                border_style,
            );

            // Draw dashes from end of label to corner
            // The label takes label_display_width columns, so next position is label_start + label_display_width
            let label_end_col = label_start + label_display_width as u16;
            for i in label_end_col..border_col + border_width.saturating_sub(1) {
                self.write_text(
                    buf,
                    self.layout.pos.y,
                    i,
                    "─",
                    self.layout.area,
                    border_style,
                );
            }

            // Draw corner
            self.write_text(
                buf,
                self.layout.pos.y,
                border_col + border_width.saturating_sub(1),
                "╮",
                self.layout.area,
                border_style,
            );
        }
        self.layout.pos.y += 1;

        // Render code content with side borders (no background color)
        if let Some(syntax) = self
            .render_context
            .syntax_set()
            .find_syntax_by_token(lang_display)
        {
            let theme = self.render_context.theme();
            let mut highlighter = HighlightLines::new(syntax, theme);

            for line in LinesWithEndings::from(code) {
                if self.layout.pos.y >= self.viewport.scroll_offset
                    && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
                {
                    // Left border and padding
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        border_col,
                        "│ ",
                        self.layout.area,
                        border_style,
                    );

                    let mut col = content_col;

                    if let Ok(ranges) =
                        highlighter.highlight_line(line, self.render_context.syntax_set())
                    {
                        for (style, text) in ranges {
                            let fg = style.foreground;
                            let ratatui_style = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
                            let text = text.trim_end_matches('\n');

                            self.write_text(
                                buf,
                                self.layout.pos.y,
                                col,
                                text,
                                self.layout.area,
                                ratatui_style,
                            );
                            col += text.len() as u16;
                        }
                    } else {
                        self.write_text(
                            buf,
                            self.layout.pos.y,
                            content_col,
                            line.trim_end_matches('\n'),
                            self.layout.area,
                            Style::default(),
                        );
                    }

                    // Right border and padding
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        border_col + border_width.saturating_sub(2),
                        " │",
                        self.layout.area,
                        border_style,
                    );
                }

                self.layout.pos.y += 1;
            }
        } else {
            for line in code.lines() {
                if self.layout.pos.y >= self.viewport.scroll_offset
                    && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
                {
                    // Left border and padding
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        border_col,
                        "│ ",
                        self.layout.area,
                        border_style,
                    );

                    // Code content
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        content_col,
                        line,
                        self.layout.area,
                        Style::default(),
                    );

                    // Right border and padding
                    self.write_text(
                        buf,
                        self.layout.pos.y,
                        border_col + border_width.saturating_sub(2),
                        " │",
                        self.layout.area,
                        border_style,
                    );
                }
                self.layout.pos.y += 1;
            }
        }

        // Bottom border: ╰─────╯
        if self.layout.pos.y >= self.viewport.scroll_offset
            && self.layout.pos.y < self.viewport.scroll_offset + self.layout.area.height
        {
            self.write_text(
                buf,
                self.layout.pos.y,
                border_col,
                "╰",
                self.layout.area,
                border_style,
            );
            for i in 1..border_width.saturating_sub(1) {
                self.write_text(
                    buf,
                    self.layout.pos.y,
                    border_col + i,
                    "─",
                    self.layout.area,
                    border_style,
                );
            }
            self.write_text(
                buf,
                self.layout.pos.y,
                border_col + border_width.saturating_sub(1),
                "╯",
                self.layout.area,
                border_style,
            );
        }
        self.layout.pos.y += 1;
    }
}
