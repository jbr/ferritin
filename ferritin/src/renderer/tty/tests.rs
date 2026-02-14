use crate::renderer::OutputMode;

use super::*;

#[test]
fn test_render_paragraph() {
    let doc = Document::from(vec![
        Span::keyword("struct"),
        Span::plain(" "),
        Span::type_name("Foo"),
    ]);
    let mut output = String::new();
    let render_context = RenderContext::new().with_output_mode(OutputMode::Tty);
    render(&doc, &render_context, &mut output).unwrap();
    // Should contain ANSI codes
    assert!(output.contains("\x1b"));
    // Should contain the actual text
    assert!(output.contains("struct"));
    assert!(output.contains("Foo"));
}

#[test]
fn test_render_heading() {
    let doc = Document::from(DocumentNode::heading(
        HeadingLevel::Title,
        vec![Span::plain("Test")],
    ));

    let mut output = String::new();
    let render_context = RenderContext::new()
        .with_output_mode(OutputMode::Tty)
        .with_terminal_width(10);

    render(&doc, &render_context, &mut output).unwrap();
    assert!(output.contains("Test"));
    // Should have decorative underline
    assert!(output.contains("=========="));
}
