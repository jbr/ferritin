use super::*;
use crate::{
    logging::StatusLogBackend,
    styled_string::{Document, DocumentNode, Span, SpanStyle},
};
use crossbeam_channel::unbounded as channel;
use ratatui::{Terminal, backend::TestBackend};

/// Helper to create a minimal test state
fn create_test_state<'a>() -> InteractiveState<'a> {
    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    let document = Document {
        nodes: vec![DocumentNode::paragraph(vec![Span {
            text: "Test document".into(),
            style: SpanStyle::Plain,
            action: None,
        }])],
    };
    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    )
}

#[test]
fn test_initial_state_is_normal_mode() {
    let state = create_test_state();
    assert!(matches!(state.ui_mode, UiMode::Normal));
}

#[test]
fn test_mode_transitions_via_state() {
    let mut state = create_test_state();

    // Start in Normal mode
    assert!(matches!(state.ui_mode, UiMode::Normal));

    // Transition to Help
    state.ui_mode = UiMode::Help;
    assert!(matches!(state.ui_mode, UiMode::Help));

    // Back to Normal
    state.ui_mode = UiMode::Normal;
    assert!(matches!(state.ui_mode, UiMode::Normal));

    // Transition to GoTo
    state.ui_mode = UiMode::Input(InputMode::GoTo {
        buffer: String::new(),
    });
    assert!(matches!(
        state.ui_mode,
        UiMode::Input(InputMode::GoTo { .. })
    ));

    // Transition to Search
    state.ui_mode = UiMode::Input(InputMode::Search {
        buffer: String::new(),
        all_crates: false,
    });
    assert!(matches!(
        state.ui_mode,
        UiMode::Input(InputMode::Search {
            all_crates: false,
            ..
        })
    ));
}

#[test]
fn test_input_mode_buffer_manipulation() {
    let mut state = create_test_state();

    // Enter GoTo mode
    state.ui_mode = UiMode::Input(InputMode::GoTo {
        buffer: String::from("test"),
    });

    // Modify buffer
    if let UiMode::Input(InputMode::GoTo { buffer }) = &mut state.ui_mode {
        buffer.push_str("_path");
        assert_eq!(buffer, "test_path");
    }

    // Enter Search mode
    state.ui_mode = UiMode::Input(InputMode::Search {
        buffer: String::from("query"),
        all_crates: false,
    });

    // Toggle all_crates
    if let UiMode::Input(InputMode::Search { buffer, all_crates }) = &mut state.ui_mode {
        assert_eq!(buffer, "query");
        assert!(!*all_crates);
        *all_crates = true;
        assert!(*all_crates);
    }
}

#[test]
fn test_history_navigation() {
    let mut state = create_test_state();

    // Initially no history, can't go back or forward
    assert!(!state.document.history.can_go_back());
    assert!(!state.document.history.can_go_forward());

    // Add first entry
    state.document.history.push(HistoryEntry::List {
        default_crate: None,
    });
    // Still can't go back (only one entry, at index 0)
    assert!(!state.document.history.can_go_back());
    assert!(!state.document.history.can_go_forward());

    // Add second entry
    state.document.history.push(HistoryEntry::Search {
        query: "test".to_string(),
        crate_name: None,
    });
    // Now we can go back (two entries, at index 1)
    assert!(state.document.history.can_go_back());
    assert!(!state.document.history.can_go_forward());

    // Go back
    state.document.history.go_back();
    // Now we can go forward but not back (at index 0)
    assert!(!state.document.history.can_go_back());
    assert!(state.document.history.can_go_forward());

    // Go forward
    state.document.history.go_forward();
    // Back at the end (index 1)
    assert!(state.document.history.can_go_back());
    assert!(!state.document.history.can_go_forward());
}

#[test]
fn test_rendering_to_test_backend() {
    let mut state = create_test_state();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Render the state
    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    // Get the buffer and verify some content
    let buffer = terminal.backend().buffer();

    // The test document has "Test document" text, should appear in the buffer
    let buffer_str = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        buffer_str.contains("Test document"),
        "Rendered buffer should contain document text"
    );
}

#[test]
fn test_brief_truncation_with_code_block() {
    use crate::styled_string::TruncationLevel;

    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    // Create a document with a Brief truncated block containing text and a code block
    let document = Document {
        nodes: vec![DocumentNode::TruncatedBlock {
            level: TruncationLevel::Brief,
            nodes: vec![
                DocumentNode::paragraph(vec![Span::plain("First paragraph with some text.")]),
                DocumentNode::paragraph(vec![Span::plain("Second paragraph with more text.")]),
                DocumentNode::CodeBlock {
                    lang: Some("rust".into()),
                    code: "fn example() {\n    println!(\"Hello\");\n    let x = 42;\n    let y = 100;\n    let z = x + y;\n}\n".into(),
                },
                DocumentNode::paragraph(vec![Span::plain("Third paragraph after code.")]),
            ],
        }],
    };

    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    let mut state = InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Render the state
    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    // Get the buffer and inspect what was rendered
    let buffer = terminal.backend().buffer();

    // Convert buffer to line-by-line representation for easier debugging
    let mut lines = Vec::new();
    for y in 0..24 {
        let line: String = (0..80)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect();
        lines.push(line);
    }

    // Print the rendered output for debugging
    println!("\n=== Rendered output ===");
    for (i, line) in lines.iter().enumerate() {
        println!("{:2}: |{}|", i, line.trim_end());
    }
    println!("=== End output ===\n");

    // Check for issues:
    // 1. Count code block borders
    let top_borders = lines.iter().filter(|l| l.contains("╭")).count();
    let bottom_borders = lines.iter().filter(|l| l.contains("╯")).count();

    println!("Top borders (╭): {}", top_borders);
    println!("Bottom borders (╯): {}", bottom_borders);

    // Look for the truncated block's closing border
    let truncation_indicators = lines.iter().filter(|l| l.contains("╰─[...]")).count();
    println!("Truncation indicators (╰─[...]): {}", truncation_indicators);

    // If a code block is started but not finished, we'd see an imbalance
    // This test documents the current behavior - it may show the bug
}

#[test]
fn test_brief_with_short_code_block() {
    use crate::styled_string::TruncationLevel;

    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    // Create a simpler case: just one line of text and a small code block
    let document = Document {
        nodes: vec![DocumentNode::TruncatedBlock {
            level: TruncationLevel::Brief,
            nodes: vec![
                DocumentNode::paragraph(vec![Span::plain("Some text before code.")]),
                DocumentNode::CodeBlock {
                    lang: Some("rust".into()),
                    code: "let x = 42;".into(),
                },
            ],
        }],
    };

    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    let mut state = InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..10 {
        let line: String = (0..80)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect();
        lines.push(line);
    }

    println!("\n=== Short code block test ===");
    for (i, line) in lines.iter().enumerate() {
        println!("{:2}: |{}|", i, line.trim_end());
    }

    let top_borders = lines.iter().filter(|l| l.contains("╭")).count();
    let bottom_borders = lines.iter().filter(|l| l.contains("╯")).count();

    println!("Top code block borders: {}", top_borders);
    println!("Bottom code block borders: {}", bottom_borders);

    // If code block renders, borders should match
    if top_borders > 0 || bottom_borders > 0 {
        assert_eq!(
            top_borders, bottom_borders,
            "Code block should have matching top and bottom borders, or not render at all"
        );
    }
}

#[test]
fn test_truncated_block_border_on_wrapped_lines() {
    use crate::styled_string::TruncationLevel;

    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    // Create a document with a Brief truncated block containing a very long line that will wrap
    // Brief mode has an 8-line limit, so we need enough content to exceed that and trigger truncation
    let long_text = "This is a very long line of text that should wrap across multiple lines when rendered in a narrow terminal window and we want to make sure the border appears on all wrapped lines not just the last one.";

    let document = Document {
        nodes: vec![DocumentNode::TruncatedBlock {
            level: TruncationLevel::Brief,
            nodes: vec![
                DocumentNode::paragraph(vec![Span::plain(long_text)]),
                DocumentNode::paragraph(vec![Span::plain(
                    "Second paragraph with additional content.",
                )]),
                DocumentNode::paragraph(vec![Span::plain(
                    "Third paragraph to ensure we exceed the 8-line Brief limit.",
                )]),
                DocumentNode::paragraph(vec![Span::plain(
                    "Fourth paragraph - this should be truncated.",
                )]),
                DocumentNode::paragraph(vec![Span::plain("Fifth paragraph - also truncated.")]),
            ],
        }],
    };

    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    let mut state = InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(60, 24); // Narrow width to force wrapping
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..10 {
        let line: String = (0..60)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect();
        lines.push(line);
    }

    println!("\n=== Wrapped line border test ===");
    for (i, line) in lines.iter().enumerate() {
        println!("{:2}: |{}|", i, line.trim_end());
    }

    // Count how many lines have the border character │
    let border_lines: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains('│'))
        .map(|(i, _)| i)
        .collect();

    println!("Lines with borders: {:?}", border_lines);

    // The border should appear on all lines with content, including wrapped lines
    // With a 60-char width, the long text should wrap to at least 3 lines
    assert!(
        border_lines.len() >= 3,
        "Expected borders on at least 3 wrapped lines, found on {} lines",
        border_lines.len()
    );
}

#[test]
#[ignore] // Run with --ignored to update snapshot
fn test_std_module_spacing() {
    use crate::styled_string::{DocumentNode, ListItem, Span};

    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    // Simulate the structure from std's markdown: paragraph, list, paragraph, list
    let document = Document {
        nodes: vec![
            // First paragraph
            DocumentNode::paragraph(vec![Span::plain(
                "The standard library exposes three common ways:",
            )]),
            // First list
            DocumentNode::List {
                items: vec![
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "Vec<T> - A heap-allocated vector",
                    )])]),
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "[T; N] - An inline array",
                    )])]),
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "[T] - A dynamically sized slice",
                    )])]),
                ],
            },
            // Second paragraph
            DocumentNode::paragraph(vec![Span::plain(
                "Slices can only be handled through pointers:",
            )]),
            // Second list
            DocumentNode::List {
                items: vec![
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "&[T] - shared slice",
                    )])]),
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "&mut [T] - mutable slice",
                    )])]),
                    ListItem::new(vec![DocumentNode::paragraph(vec![Span::plain(
                        "Box<[T]> - owned slice",
                    )])]),
                ],
            },
            // Final paragraph
            DocumentNode::paragraph(vec![Span::plain(
                "str, a UTF-8 string slice, is a primitive type.",
            )]),
        ],
    };

    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    let mut state = InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..25 {
        let line: String = (0..80)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect();
        output.push_str(&format!("{}\n", line.trim_end()));
    }

    println!("\n=== Current std-like spacing ===");
    println!("{}", output);
    println!("=== End ===\n");

    // TODO: Once we fix spacing, add assertions about blank line counts
}

#[test]
#[ignore] // Run with --ignored to update snapshot
fn test_code_block_spacing() {
    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    // Simulate paragraph followed by code block (like alloc module docs)
    let document = Document {
        nodes: vec![
            DocumentNode::paragraph(vec![Span::plain("Here's an example:")]),
            DocumentNode::CodeBlock {
                lang: Some("rust".into()),
                code: "let x = vec![1, 2, 3];".into(),
            },
            DocumentNode::paragraph(vec![Span::plain("More content after the code block.")]),
        ],
    };

    let render_context = RenderContext::new();
    let theme = InteractiveTheme::from_render_context(&render_context);
    let (_, log_reader) = StatusLogBackend::new(100);

    let mut state = InteractiveState::new(
        document,
        None,
        cmd_tx,
        resp_rx,
        render_context,
        theme,
        log_reader,
    );
    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| state.render_frame(frame)).unwrap();

    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..15 {
        let line: String = (0..60)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect();
        output.push_str(&format!("{}\n", line.trim_end()));
    }

    println!("\n=== Current code block spacing ===");
    println!("{}", output);
    println!("=== End ===\n");

    // Count blank lines
    let blank_lines_before_code = output
        .lines()
        .skip(1) // Skip "Here's an example:"
        .take_while(|l| l.trim().is_empty())
        .count();

    println!("Blank lines before code block: {}", blank_lines_before_code);
    println!("Expected: 1 blank line between paragraph and code block");

    // TODO: Once we fix spacing, assert blank_lines_before_code == 1
}
