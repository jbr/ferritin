//! Server-side syntax highlighting of fenced code blocks into CSS-class-tagged
//! spans for the JSON / web path.
//!
//! The terminal renderers highlight fenced blocks at *render* time (syntect →
//! theme RGB). The web client instead needs theme-agnostic, class-based spans it
//! can color with its own light/dark CSS — the same way it already renders the
//! semantic [`SpanStyle`](crate::styled_string::SpanStyle) spans of generated
//! signatures. So here we run syntect's parser over the block, collapse each
//! token's TextMate scope stack into a small fixed *lexical* vocabulary
//! (`keyword`, `type`, `string`, …), and emit spans that tile the source exactly
//! (concatenating their `text` reconstructs the code).
//!
//! This vocabulary is deliberately distinct from `SpanStyle`: those spans are
//! semantic and navigable (a `TypeName` carries a resolve link), while these are
//! purely lexical guesses over opaque example text. On the client they share a
//! color palette, not a meaning.
//!
//! The scope → class mapping mirrors the fallback chains
//! [`ColorScheme`](crate::color_scheme) uses in the other direction (semantic
//! style → representative scope → theme color), so the two stay consistent.

use std::sync::LazyLock;
use syntect::{
    easy::ScopeRegionIterator,
    parsing::{ParseState, Scope, ScopeStack, SyntaxSet},
    util::LinesWithEndings,
};

/// Loaded once for the process; read-only and `Sync`. The JSON path (the CLI and
/// the server's rayon workers) shares this rather than reparsing the syntax dump
/// per request.
static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// One highlighted region of a code block: a slice of the source and its lexical
/// class. `class` is `None` for text syntect didn't scope (punctuation, plain
/// identifiers) or for any block whose language has no grammar.
pub struct CodeSpan<'a> {
    pub text: &'a str,
    pub class: Option<&'static str>,
}

/// Highlight `code` as `lang`, returning class-tagged spans that tile the source.
///
/// Falls back to the Rust grammar when `lang` is absent (matching the terminal's
/// default), and to a single unclassed span when no grammar matches — so callers
/// always get a faithful, gap-free representation of the text.
pub fn highlight<'a>(lang: Option<&str>, code: &'a str) -> Vec<CodeSpan<'a>> {
    let syntax = lang
        .and_then(|l| SYNTAX_SET.find_syntax_by_token(l))
        .or_else(|| SYNTAX_SET.find_syntax_by_token("rust"));
    let Some(syntax) = syntax else {
        return vec![CodeSpan {
            text: code,
            class: None,
        }];
    };

    let base = code.as_ptr() as usize;
    let mut parse_state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut spans: Vec<CodeSpan<'a>> = Vec::new();
    // The currently-open run: (start byte offset into `code`, its class). Adjacent
    // regions of equal class merge into one run so the wire carries runs, not
    // per-token fragments.
    let mut run: Option<(usize, Option<&'static str>)> = None;

    for line in LinesWithEndings::from(code) {
        let line_off = line.as_ptr() as usize - base;
        let ops = match parse_state.parse_line(line, &SYNTAX_SET) {
            Ok(ops) => ops,
            // Best-effort: a line we can't scope is emitted verbatim so the spans
            // still tile the source exactly.
            Err(_) => {
                if let Some((start, class)) = run.take() {
                    spans.push(CodeSpan {
                        text: &code[start..line_off],
                        class,
                    });
                }
                spans.push(CodeSpan {
                    text: line,
                    class: None,
                });
                continue;
            }
        };
        for (text, op) in ScopeRegionIterator::new(&ops, line) {
            let _ = stack.apply(op);
            if text.is_empty() {
                continue;
            }
            let off = text.as_ptr() as usize - base;
            let class = classify(stack.as_slice());
            match run {
                Some((_, open)) if open == class => {} // extend the open run
                Some((start, open)) => {
                    spans.push(CodeSpan {
                        text: &code[start..off],
                        class: open,
                    });
                    run = Some((off, class));
                }
                None => run = Some((off, class)),
            }
        }
    }

    if let Some((start, class)) = run {
        spans.push(CodeSpan {
            text: &code[start..],
            class,
        });
    }
    if spans.is_empty() {
        spans.push(CodeSpan {
            text: code,
            class: None,
        });
    }
    spans
}

/// Reduce a token's scope stack to a single lexical class.
///
/// A `comment` or `string` anywhere in the stack colors the whole token — those
/// are containers whose nested `punctuation.definition.*` delimiters (`//`, the
/// quotes) belong to them, not to generic punctuation. Otherwise we prefer the
/// most specific (innermost) recognized scope.
fn classify(stack: &[Scope]) -> Option<&'static str> {
    let scopes: Vec<String> = stack.iter().map(|s| s.build_string()).collect();
    if scopes.iter().any(|s| s.starts_with("comment")) {
        return Some("comment");
    }
    if scopes.iter().any(|s| s.starts_with("string")) {
        return Some("string");
    }
    scopes.iter().rev().find_map(|s| class_for_scope(s))
}

/// Map one TextMate scope (e.g. `keyword.control.rust`) to a lexical class, or
/// `None` if it isn't one we color. The vocabulary — `keyword`, `operator`,
/// `type`, `function`, `string`, `number`, `constant`, `comment`, `variable`,
/// `punctuation` — is language-agnostic (it keys on the standardized leading
/// scope atoms), so bash / toml / json blocks decorate too.
fn class_for_scope(scope: &str) -> Option<&'static str> {
    let mut atoms = scope.split('.');
    match atoms.next()? {
        "comment" => Some("comment"),
        "string" => Some("string"),
        "keyword" => match atoms.next() {
            Some("operator") => Some("operator"),
            _ => Some("keyword"),
        },
        // In Rust's grammar `storage.type` is the declaration keywords (`let`,
        // `const`, `fn`, `struct`) *and* the primitive types (`i32`, `bool`) —
        // one scope, indistinguishable — while `storage.modifier` is `pub`/`mut`.
        // Rust is the primary content, so we color the whole family as keywords
        // (declaration keywords are the frequent case; primitives read fine as
        // keyword-colored). Concrete type *names* still come via `support.type`
        // and `entity.name.type` below.
        "storage" => Some("keyword"),
        "constant" => match atoms.next() {
            Some("numeric") => Some("number"),
            _ => Some("constant"), // constant.language (true/false), .character, .other
        },
        "entity" => {
            if scope.starts_with("entity.name.function") {
                Some("function")
            } else if scope.starts_with("entity.name.type")
                || scope.starts_with("entity.name.class")
                || scope.starts_with("entity.name.namespace")
            {
                Some("type")
            } else {
                None
            }
        }
        "support" => {
            if scope.starts_with("support.function") {
                Some("function")
            } else if scope.starts_with("support.constant") {
                Some("constant")
            } else {
                Some("type") // support.type / support.class / support.other — stdlib types
            }
        }
        "variable" => Some("variable"),
        "punctuation" => Some("punctuation"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spans must always reconstruct the input exactly (they tile it), so the
    /// client can recover raw text by concatenation.
    fn assert_tiles(lang: Option<&str>, code: &str) {
        let joined: String = highlight(lang, code).iter().map(|s| s.text).collect();
        assert_eq!(joined, code);
    }

    #[test]
    fn tiles_rust() {
        assert_tiles(Some("rust"), "let x = \"hi\";\nfn main() {}\n");
    }

    #[test]
    fn tiles_unknown_language() {
        assert_tiles(Some("brainfuck-that-has-no-grammar"), "+++[.]\n");
    }

    #[test]
    fn tiles_empty() {
        assert_tiles(Some("rust"), "");
    }

    #[test]
    fn classifies_rust_tokens() {
        let spans = highlight(Some("rust"), "let n = 42; // note\nlet v = Vec::new();\n");
        let class_of = |needle: &str| {
            spans
                .iter()
                .find(|s| s.text.contains(needle))
                .and_then(|s| s.class)
        };
        assert_eq!(class_of("let"), Some("keyword"));
        assert_eq!(class_of("42"), Some("number"));
        assert_eq!(class_of("// note"), Some("comment"));
        assert_eq!(class_of("Vec"), Some("type"));
    }
}
