use super::*;

/// Format source code
pub(crate) fn format_source_code(request: &Request, span: &Span) -> String {
    // Resolve the file path - if it's relative, make it relative to the project root
    let file_path = if span.filename.is_absolute() {
        span.filename.clone()
    } else {
        request.project.project_root().join(&span.filename)
    };

    let Ok(file_content) = fs::read_to_string(&file_path) else {
        return String::new();
    };

    let lines: Vec<&str> = file_content.lines().collect();

    // rustdoc spans are 1-indexed
    let start_line = span.begin.0.saturating_sub(1);
    let end_line = span.end.0.saturating_sub(1);

    if start_line >= lines.len() {
        return String::new();
    }

    let end_line = end_line.min(lines.len().saturating_sub(1));

    // Add a few lines of context around the item
    let context_lines = if end_line - start_line < 10 { 1 } else { 3 };
    let context_start = start_line.saturating_sub(context_lines);
    let context_end = (end_line + context_lines).min(lines.len().saturating_sub(1));

    let mut result = String::new();
    result.write_fmt(format_args!("\nSource: {}\n", file_path.display()));
    result.push_str("```rust\n");

    for line in lines[context_start..=context_end].iter() {
        result.write_fmt(format_args!("{line}\n"));
    }

    result.push_str("```\n");

    result
}
