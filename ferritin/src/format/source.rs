use super::*;
use crate::styled_string::{DocumentNode, Span as StyledSpan};

/// Format source code
pub(crate) fn format_source_code<'a>(request: &'a Request, span: &Span) -> Vec<DocumentNode<'a>> {
    // Resolve the file path - if it's relative, make it relative to the project root
    let file_path = if span.filename.is_absolute() {
        span.filename.clone()
    } else if let Some(project_root) = request.project_root() {
        project_root.join(&span.filename)
    } else {
        // No project and relative path - can't resolve
        return vec![];
    };

    let Ok(file_content) = fs::read_to_string(&file_path) else {
        return vec![];
    };

    let lines: Vec<&str> = file_content.lines().collect();

    // rustdoc spans are 1-indexed
    let start_line = span.begin.0.saturating_sub(1);
    let end_line = span.end.0.saturating_sub(1);

    if start_line >= lines.len() {
        return vec![];
    }

    let end_line = end_line.min(lines.len().saturating_sub(1));

    // Add a few lines of context around the item
    let context_lines = if end_line - start_line < 10 { 1 } else { 3 };
    let context_start = start_line.saturating_sub(context_lines);
    let context_end = (end_line + context_lines).min(lines.len().saturating_sub(1));

    // Collect source lines
    let code = lines[context_start..=context_end].join("\n");

    // Build document nodes
    vec![
        DocumentNode::paragraph(vec![StyledSpan::plain(format!(
            "Source: {}",
            file_path.display()
        ))]),
        DocumentNode::code_block(Some("rust"), code),
    ]
}
