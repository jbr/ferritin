use super::*;

#[test]
fn test_tokenize() {
    assert_eq!(
        tokenize("Hello, world! This is a test. CamelCase hyphenate-word snake_word"),
        vec![
            "Hello",
            "world",
            "This",
            "test",
            "Camel",
            "Case",
            "CamelCase",
            "hyphenate",
            "word",
            "hyphenate-word",
            "snake",
            "word",
            "snake_word"
        ]
    );
}

#[test]
fn test_hash_term() {
    // Should be case insensitive
    assert_eq!(hash_term("Hello"), hash_term("HELLO"));
    assert_eq!(hash_term("Hello"), hash_term("hello"));
}

#[test]
fn test_prose_slices_basic() {
    let text = "Some prose\n```rust\nlet x = 1;\n```\nMore prose";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Some prose\n", "More prose"]);
}

#[test]
fn test_prose_slices_blank_lines_in_code() {
    let text = "Prose\n```rust\nlet x = 1;\n\nassert!(x > 0);\n```\nMore";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Prose\n", "More"]);
}

#[test]
fn test_prose_slices_language_tags() {
    let text = "Before\n```rust,no_run\ncode\n```\nAfter";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Before\n", "After"]);
}

#[test]
fn test_prose_slices_no_fences() {
    let text = "Just some prose\nwith multiple lines";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Just some prose\nwith multiple lines"]);
}

#[test]
fn test_prose_slices_conservative_start() {
    // ``` with other content on line should not start a fence
    let text = "Prose\n``` not a fence\nmore prose";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Prose\n``` not a fence\nmore prose"]);
}

#[test]
fn test_prose_slices_eager_end() {
    // ``` anywhere on line should end fence
    let text = "Prose\n```\ncode\nmore code ```\nAfter";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["Prose\n", "After"]);
}

#[test]
fn test_prose_slices_multiple_fences() {
    let text = "First\n```\ncode1\n```\nMiddle\n```\ncode2\n```\nLast";
    let slices: Vec<_> = prose_slices(text).collect();
    assert_eq!(slices, vec!["First\n", "Middle\n", "Last"]);
}

/// Test that our fence detection produces the same tokens as pulldown-cmark
#[test]
fn test_prose_slices_matches_pulldown_cmark() {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let test_cases = vec![
        "Simple\n```rust\nlet x = vec![];\n```\nProse",
        "```rust\nlet x = 1;\n\nassert!(x > 0);\n```\nAfter",
        "Before\n```rust,no_run\ncode\n```\nAfter\n```\nmore\n```\nEnd",
        "No fences here",
        "Inline `code` is fine\n```\nblock code\n```\nmore",
        "Multiple\n```\nfirst\n```\nblocks\n```rust\nsecond\n```\nhere",
    ];

    for text in test_cases {
        // Tokenize our prose slices
        let our_tokens: Vec<&str> = prose_slices(text).flat_map(tokenize).collect();

        // Extract non-code content from pulldown-cmark and tokenize
        let mut cmark_prose = String::new();
        let mut in_code_block = false;

        for event in Parser::new(text) {
            match event {
                Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
                Event::End(TagEnd::CodeBlock) => in_code_block = false,
                Event::Text(t) if !in_code_block => {
                    cmark_prose.push(' ');
                    cmark_prose.push_str(&t);
                }
                Event::Code(c) => {
                    cmark_prose.push(' ');
                    cmark_prose.push_str(&c);
                }
                _ => {}
            }
        }

        let cmark_tokens = tokenize(&cmark_prose);

        assert_eq!(
            our_tokens, cmark_tokens,
            "Token mismatch for input: {:?}",
            text
        );
    }
}
