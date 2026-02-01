use super::*;
use crate::styled_string::{Document, DocumentNode, Span, SpanStyle};
use ratatui::{Terminal, backend::TestBackend, style::Style};
use std::sync::mpsc::channel;
use syntect::{highlighting::ThemeSet, parsing::SyntaxSet};

/// Helper to create a minimal test state
fn create_test_state<'a>() -> InteractiveState<'a> {
    let (cmd_tx, _cmd_rx) = channel();
    let (_resp_tx, resp_rx) = channel();

    let document = Document {
        nodes: vec![DocumentNode::Span(Span {
            text: "Test document".into(),
            style: SpanStyle::Plain,
            action: None,
        })],
    };

    // Create minimal ui_config and theme for testing
    let theme_set = ThemeSet::load_defaults();
    let ui_config = UiRenderConfig {
        syntax_theme: theme_set.themes.get("base16-ocean.dark").unwrap().clone(),
        syntax_set: SyntaxSet::load_defaults_newlines(),
        color_scheme: crate::color_scheme::ColorScheme::default(),
    };

    let theme = InteractiveTheme {
        breadcrumb_style: Style::default(),
        breadcrumb_current_style: Style::default(),
        breadcrumb_hover_style: Style::default(),
        status_style: Style::default(),
        status_hint_style: Style::default(),
        help_bg_style: Style::default(),
        help_title_style: Style::default(),
        help_key_style: Style::default(),
        help_desc_style: Style::default(),
        muted_style: Style::default(),
        document_bg_style: Style::default(),
        code_block_border_style: Style::default(),
    };

    InteractiveState::new(document, None, cmd_tx, resp_rx, ui_config, theme)
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
    state.document.history.push(HistoryEntry::List);
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
