//! Shared table layout: column-width allocation and span-aware word-wrap.
//!
//! All three text renderers (plain, tty, interactive) use the same layout pass
//! to decide column widths and break long cells across multiple lines while
//! preserving span styling and link actions.

use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::styled_string::{Span, SpanStyle, TableCell, TuiAction};

/// Default minimum width when shrinking columns to fit. A column may end up
/// narrower than this if its natural width is already smaller.
const MIN_COL_WIDTH: usize = 8;

/// Default fallback width for renderers (like plain) that don't have a
/// terminal width to consult.
pub(super) const DEFAULT_FALLBACK_WIDTH: usize = 100;

/// A cell after layout: a stack of styled lines, each padded conceptually to
/// the column's display width (renderers handle the padding themselves).
#[derive(Debug)]
pub(super) struct LaidOutCell<'a> {
    pub lines: Vec<Vec<Span<'a>>>,
}

/// Result of laying out a table for a given available width.
#[derive(Debug)]
pub(super) struct TableLayout<'a> {
    pub col_widths: Vec<usize>,
    pub header: Option<Vec<LaidOutCell<'a>>>,
    pub rows: Vec<Vec<LaidOutCell<'a>>>,
}

impl<'a> TableLayout<'a> {
    pub fn num_cols(&self) -> usize {
        self.col_widths.len()
    }

    /// True if any cell in the header or body wrapped to more than one line.
    /// Renderers use this to decide whether to insert blank separator rows
    /// between body rows so wrapped cells don't visually run together.
    pub fn any_wrapped(&self) -> bool {
        let header = self
            .header
            .iter()
            .flat_map(|cells| cells.iter())
            .any(|c| c.lines.len() > 1);
        let rows = self
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .any(|c| c.lines.len() > 1);
        header || rows
    }
}

/// Display width of a span's text (treats control chars as 0).
pub(super) fn span_display_width(span: &Span) -> usize {
    UnicodeWidthStr::width(span.text.as_ref())
}

/// Sum of display widths of every span in a cell (single-line view).
fn cell_natural_width(cell: &TableCell) -> usize {
    cell.spans.iter().map(span_display_width).sum()
}

/// Lay out a table for the given available terminal width (in display columns).
///
/// `available_width` is the total horizontal space, including border characters.
/// If the table's natural widths fit, columns get their natural widths. Otherwise
/// the widest column is shrunk repeatedly until the table fits or columns hit
/// the minimum width.
pub(super) fn lay_out<'a>(
    header: Option<&[TableCell<'a>]>,
    rows: &[Vec<TableCell<'a>>],
    available_width: usize,
) -> TableLayout<'a> {
    let num_cols = header
        .map(<[_]>::len)
        .or_else(|| rows.first().map(Vec::len))
        .unwrap_or(0);

    if num_cols == 0 {
        return TableLayout {
            col_widths: Vec::new(),
            header: None,
            rows: Vec::new(),
        };
    }

    let mut natural = vec![0usize; num_cols];
    if let Some(header_cells) = header {
        for (col_idx, cell) in header_cells.iter().enumerate().take(num_cols) {
            natural[col_idx] = natural[col_idx].max(cell_natural_width(cell));
        }
    }
    for row in rows {
        for (col_idx, cell) in row.iter().enumerate().take(num_cols) {
            natural[col_idx] = natural[col_idx].max(cell_natural_width(cell));
        }
    }

    let col_widths = compute_column_widths(&natural, available_width);

    let header_laid_out = header.map(|cells| {
        cells
            .iter()
            .enumerate()
            .map(|(col_idx, cell)| lay_out_cell(cell, col_widths[col_idx]))
            .collect()
    });

    let rows_laid_out = rows
        .iter()
        .map(|row| {
            (0..num_cols)
                .map(|col_idx| match row.get(col_idx) {
                    Some(cell) => lay_out_cell(cell, col_widths[col_idx]),
                    None => LaidOutCell {
                        lines: vec![vec![]],
                    },
                })
                .collect()
        })
        .collect();

    TableLayout {
        col_widths,
        header: header_laid_out,
        rows: rows_laid_out,
    }
}

/// Choose column widths that fit in `available_width` (which includes
/// `num_cols + 1` border characters). When natural widths overflow, the widest
/// column is reduced one column at a time until the budget is met or columns
/// reach `MIN_COL_WIDTH`.
fn compute_column_widths(natural: &[usize], available_width: usize) -> Vec<usize> {
    let num_cols = natural.len();
    if num_cols == 0 {
        return Vec::new();
    }

    let border_chars = num_cols + 1;
    let budget = available_width.saturating_sub(border_chars);

    let total_natural: usize = natural.iter().sum();
    if total_natural <= budget {
        return natural.to_vec();
    }

    let mut widths: Vec<usize> = natural.to_vec();
    loop {
        let total: usize = widths.iter().sum();
        if total <= budget {
            break;
        }
        // Find widest column above MIN_COL_WIDTH.
        let target = widths
            .iter()
            .enumerate()
            .filter(|&(_, &w)| w > MIN_COL_WIDTH)
            .max_by_key(|&(_, &w)| w)
            .map(|(idx, _)| idx);
        match target {
            Some(idx) => widths[idx] -= 1,
            None => break, // every column already at the floor
        }
    }
    widths
}

/// Wrap a single cell's spans to fit `width` display columns. Returns at least
/// one line (an empty line for empty cells).
fn lay_out_cell<'a>(cell: &TableCell<'a>, width: usize) -> LaidOutCell<'a> {
    if width == 0 {
        return LaidOutCell {
            lines: vec![vec![]],
        };
    }
    let tokens = tokenize(&cell.spans);
    let mut lines = wrap_tokens(tokens, width);
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    LaidOutCell { lines }
}

#[derive(Debug)]
enum TokenKind {
    Word,
    Space,
    LineBreak,
}

struct Token<'a> {
    text: Cow<'a, str>,
    style: SpanStyle,
    action: Option<TuiAction<'a>>,
    kind: TokenKind,
    width: usize,
}

/// Split spans into a flat token stream of words, inline-whitespace runs, and
/// hard line breaks. Style and action are inherited from the source span.
fn tokenize<'a>(spans: &[Span<'a>]) -> Vec<Token<'a>> {
    let mut tokens = Vec::new();
    for span in spans {
        let style = span.style;
        let action = span.action.clone();
        let text: &str = span.text.as_ref();
        let mut iter = text.char_indices().peekable();

        while let Some((start, ch)) = iter.next() {
            if ch == '\n' {
                tokens.push(Token {
                    text: Cow::Borrowed(""),
                    style,
                    action: action.clone(),
                    kind: TokenKind::LineBreak,
                    width: 0,
                });
                continue;
            }

            let is_space = ch.is_whitespace();
            let mut end = start + ch.len_utf8();
            while let Some(&(i, c2)) = iter.peek() {
                let same_kind = if is_space {
                    c2.is_whitespace() && c2 != '\n'
                } else {
                    !c2.is_whitespace()
                };
                if !same_kind {
                    break;
                }
                end = i + c2.len_utf8();
                iter.next();
            }

            let slice = &text[start..end];
            let chunk = Cow::Owned(slice.to_string());
            let kind = if is_space {
                TokenKind::Space
            } else {
                TokenKind::Word
            };
            tokens.push(Token {
                width: UnicodeWidthStr::width(slice),
                text: chunk,
                style,
                action: action.clone(),
                kind,
            });
        }
    }
    tokens
}

/// Greedy word wrap. Tokens are placed on the current line as long as they
/// fit; oversized words are hard-broken at the column boundary.
fn wrap_tokens<'a>(tokens: Vec<Token<'a>>, width: usize) -> Vec<Vec<Span<'a>>> {
    let mut lines: Vec<Vec<Span<'a>>> = Vec::new();
    let mut current: Vec<Span<'a>> = Vec::new();
    let mut current_width = 0usize;

    for token in tokens {
        match token.kind {
            TokenKind::LineBreak => {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            TokenKind::Space => {
                if current.is_empty() {
                    // Drop leading whitespace at the start of a line.
                    continue;
                }
                if current_width + token.width > width {
                    // The space is the natural break point — flush and skip it.
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                    continue;
                }
                current.push(Span {
                    text: token.text,
                    style: token.style,
                    action: token.action,
                });
                current_width += token.width;
            }
            TokenKind::Word => {
                if current_width + token.width <= width {
                    current.push(Span {
                        text: token.text,
                        style: token.style,
                        action: token.action,
                    });
                    current_width += token.width;
                    continue;
                }
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                if token.width <= width {
                    current.push(Span {
                        text: token.text,
                        style: token.style,
                        action: token.action,
                    });
                    current_width = token.width;
                } else {
                    hard_break_word(
                        token.text.as_ref(),
                        token.style,
                        token.action,
                        width,
                        &mut lines,
                        &mut current,
                        &mut current_width,
                    );
                }
            }
        }
    }

    lines.push(current);
    lines
}

/// Break a single word that's longer than the column width, emitting full-width
/// chunks on their own lines and leaving any short remainder on `current`.
fn hard_break_word<'a>(
    word: &str,
    style: SpanStyle,
    action: Option<TuiAction<'a>>,
    width: usize,
    lines: &mut Vec<Vec<Span<'a>>>,
    current: &mut Vec<Span<'a>>,
    current_width: &mut usize,
) {
    let mut remaining = word;
    while !remaining.is_empty() {
        let mut end_byte = 0;
        let mut acc_w = 0usize;
        for (i, ch) in remaining.char_indices() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if acc_w + cw > width {
                break;
            }
            acc_w += cw;
            end_byte = i + ch.len_utf8();
        }
        // Defensive: a zero-width char with width 0 budget; force progress.
        if end_byte == 0 {
            end_byte = remaining
                .char_indices()
                .nth(1)
                .map_or(remaining.len(), |(i, _)| i);
            acc_w = UnicodeWidthStr::width(&remaining[..end_byte]);
        }
        let chunk = remaining[..end_byte].to_string();
        let span = Span {
            text: Cow::Owned(chunk),
            style,
            action: action.clone(),
        };
        if acc_w >= width {
            lines.push(vec![span]);
            *current_width = 0;
        } else {
            current.push(span);
            *current_width = acc_w;
        }
        remaining = &remaining[end_byte..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_cell(text: &'static str) -> TableCell<'static> {
        TableCell {
            spans: vec![Span::plain(text)],
        }
    }

    fn multi_span_cell(parts: &[&'static str]) -> TableCell<'static> {
        TableCell {
            spans: parts.iter().map(|p| Span::plain(*p)).collect(),
        }
    }

    fn line_text(line: &[Span]) -> String {
        line.iter().map(|s| s.text.as_ref()).collect()
    }

    #[test]
    fn natural_widths_when_table_fits() {
        let header = vec![plain_cell("a"), plain_cell("b")];
        let rows = vec![vec![plain_cell("hi"), plain_cell("there")]];
        let layout = lay_out(Some(&header), &rows, 80);
        assert_eq!(layout.col_widths, vec![2, 5]);
    }

    #[test]
    fn shrinks_widest_column_when_overflowing() {
        let header = vec![plain_cell("a"), plain_cell("b")];
        let rows = vec![vec![
            plain_cell("short"),
            plain_cell("a much longer chunk of prose that wants to wrap"),
        ]];
        let layout = lay_out(Some(&header), &rows, 30);
        // budget = 30 - 3 borders = 27 columns; shorter col stays at natural.
        assert!(layout.col_widths[0] <= 5);
        assert!(layout.col_widths[1] >= 8);
        assert!(layout.col_widths.iter().sum::<usize>() <= 27);
    }

    #[test]
    fn wraps_long_cell_across_multiple_lines() {
        let rows = vec![vec![plain_cell(
            "Skip the Alt-Svc cache and dial QUIC directly.",
        )]];
        let layout = lay_out(None, &rows, 25);
        let cell = &layout.rows[0][0];
        assert!(cell.lines.len() > 1, "expected wrap, got {:?}", cell.lines);
        for line in &cell.lines {
            let w: usize = line.iter().map(span_display_width).sum();
            assert!(
                w <= layout.col_widths[0],
                "line {w} > {}",
                layout.col_widths[0]
            );
        }
        let joined: String = cell
            .lines
            .iter()
            .map(|l| line_text(l))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("Skip the Alt-Svc"));
        assert!(joined.contains("directly."));
    }

    #[test]
    fn multi_span_cell_keeps_bytes_intact() {
        // Cell has three spans like a markdown link: "Falls back to " + "Client::new" + "."
        // The bug was that each span was independently truncated to col_width,
        // letting the cell exceed col_width. With wrapping, total width per
        // line stays within col_width.
        let cell = multi_span_cell(&["Falls back to ", "Client::new_with_quic", "."]);
        let layout = lay_out(None, &[vec![cell]], 25);
        let laid = &layout.rows[0][0];
        for line in &laid.lines {
            let w: usize = line.iter().map(span_display_width).sum();
            assert!(
                w <= layout.col_widths[0],
                "line width {w} > col {}",
                layout.col_widths[0]
            );
        }
        let joined: String = laid
            .lines
            .iter()
            .map(|l| line_text(l))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("Falls back to"));
        assert!(joined.contains("Client::new_with_quic"));
    }

    #[test]
    fn handles_unicode_emdash_correctly() {
        // The em-dash is 3 bytes / 1 column; byte-based truncation would have
        // miscounted width and could split it.
        let rows = vec![vec![plain_cell(
            "No fallback — a non-h2-speaking server surfaces an error.",
        )]];
        let layout = lay_out(None, &rows, 25);
        for line in &layout.rows[0][0].lines {
            let w: usize = line.iter().map(span_display_width).sum();
            assert!(w <= layout.col_widths[0]);
            // No partial UTF-8 sequences anywhere.
            for span in line {
                assert!(span.text.is_char_boundary(0));
                assert!(span.text.is_char_boundary(span.text.len()));
            }
        }
    }

    #[test]
    fn hard_break_word_longer_than_column() {
        let rows = vec![vec![plain_cell("supercalifragilisticexpialidocious")]];
        let layout = lay_out(None, &rows, 14); // budget 14 - 2 borders = 12
        let cell = &layout.rows[0][0];
        assert!(cell.lines.len() >= 2);
        for line in &cell.lines {
            let w: usize = line.iter().map(span_display_width).sum();
            assert!(w <= layout.col_widths[0]);
        }
        let joined: String = cell
            .lines
            .iter()
            .flat_map(|l| l.iter().map(|s| s.text.as_ref()))
            .collect();
        assert_eq!(joined, "supercalifragilisticexpialidocious");
    }

    #[test]
    fn explicit_newline_forces_break() {
        let rows = vec![vec![plain_cell("line one\nline two")]];
        let layout = lay_out(None, &rows, 80);
        let cell = &layout.rows[0][0];
        assert_eq!(cell.lines.len(), 2);
        assert_eq!(line_text(&cell.lines[0]), "line one");
        assert_eq!(line_text(&cell.lines[1]), "line two");
    }

    #[test]
    fn empty_cell_yields_one_empty_line() {
        let rows = vec![vec![plain_cell("")]];
        let layout = lay_out(None, &rows, 80);
        assert_eq!(layout.rows[0][0].lines.len(), 1);
        assert!(layout.rows[0][0].lines[0].is_empty());
    }
}
