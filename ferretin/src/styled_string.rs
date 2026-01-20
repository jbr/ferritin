use ferretin_common::DocRef;
use rustdoc_types::Item;
use std::borrow::Cow;

/// Interactive action that can be attached to a span
#[derive(Debug, Clone)]
pub enum TuiAction<'a> {
    /// Navigate to an already-loaded item (zero-cost since DocRef is Copy)
    Navigate(DocRef<'a, Item>),
    /// Navigate to an item by path (resolves lazily)
    NavigateToPath(String),
    /// Expand a truncated block (identified by index path into document tree)
    ExpandBlock(NodePath),
    /// Open an external URL in browser
    OpenUrl(String),
}

/// Path to a node in the document tree using indices
/// Example: [2, 3, 1] means nodes[2].children[3].children[1]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePath {
    indices: [u16; 8], // 8 levels deep should be enough
    len: u8,
}

impl NodePath {
    pub fn new() -> Self {
        Self {
            indices: [0; 8],
            len: 0,
        }
    }

    pub fn push(&mut self, index: usize) {
        if (self.len as usize) < self.indices.len() {
            self.indices[self.len as usize] = index as u16;
            self.len += 1;
        }
    }

    pub fn indices(&self) -> &[u16] {
        &self.indices[..self.len as usize]
    }
}

/// A semantic content tree for Rust documentation
#[derive(Debug, Clone)]
pub struct Document<'a> {
    pub nodes: Vec<DocumentNode<'a>>,
}

/// A node in the documentation tree
#[derive(Debug, Clone)]
pub enum DocumentNode<'a> {
    /// Inline styled text
    Span(Span<'a>),

    /// Block-level heading
    Heading {
        level: HeadingLevel,
        spans: Vec<Span<'a>>,
    },

    /// Structural section with optional title
    Section {
        title: Option<Vec<Span<'a>>>,
        nodes: Vec<DocumentNode<'a>>,
    },

    /// List of items
    List { items: Vec<ListItem<'a>> },

    /// Code block with syntax highlighting
    CodeBlock {
        lang: Option<Cow<'a, str>>,
        code: Cow<'a, str>,
    },

    /// Hyperlink
    Link {
        url: String,
        text: Vec<Span<'a>>,
        item: Option<DocRef<'a, Item>>,
    },

    /// Horizontal rule/divider
    HorizontalRule,

    /// Block quote
    BlockQuote { nodes: Vec<DocumentNode<'a>> },

    /// Table
    Table {
        header: Option<Vec<TableCell<'a>>>,
        rows: Vec<Vec<TableCell<'a>>>,
    },

    /// Truncated documentation block
    /// Renderers decide how to truncate based on the level hint
    TruncatedBlock {
        nodes: Vec<DocumentNode<'a>>,
        level: TruncationLevel,
    },
}

/// A single cell in a table
#[derive(Debug, Clone)]
pub struct TableCell<'a> {
    pub spans: Vec<Span<'a>>,
}

/// A single item in a list
#[derive(Debug, Clone)]
pub struct ListItem<'a> {
    pub label: Option<Vec<Span<'a>>>,
    pub content: Vec<DocumentNode<'a>>,
}

/// Heading level for semantic structure
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingLevel {
    Title,   // Top-level item name: "Item: Vec"
    Section, // Section header: "Fields:", "Methods:"
}

/// Truncation level hint for renderers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationLevel {
    /// Single-line summary (for listings)
    SingleLine,
    /// Brief paragraph (for secondary items like methods/fields)
    Brief,
    /// Full documentation (for main requested item)
    Full,
}

/// A styled text span with semantic meaning
#[derive(Debug, Clone)]
pub struct Span<'a> {
    pub text: Cow<'a, str>,
    pub style: SpanStyle,
    pub action: Option<TuiAction<'a>>,
}

/// Semantic styling categories for Rust code elements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpanStyle {
    // Rust code semantic elements
    Keyword,      // struct, enum, pub, fn, const, etc.
    TypeName,     // MyStruct, Vec, String, etc.
    FunctionName, // my_function, new, etc.
    FieldName,    // field names in structs
    Lifetime,     // 'a, 'static, etc.
    Generic,      // T, U, generic parameters

    // Structural elements
    Plain,       // unstyled text, whitespace
    Punctuation, // :, {, }, (, ), etc.
    Operator,    // &, *, ->, etc.
    Comment,     // // comments in code output

    // Code content
    InlineRustCode, // Inline code expressions (const values, etc.) - unparsed Rust code
    InlineCode,     // Generic inline code from markdown backticks

    // Markdown semantic styles
    Strong,        // **bold** - semantic emphasis
    Emphasis,      // *italic* - semantic emphasis
    Strikethrough, // ~~strikethrough~~ - from GFM
}

impl<'a> Span<'a> {
    // Rust code element constructors
    pub fn keyword(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Keyword,
            action: None,
        }
    }

    pub fn type_name(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::TypeName,
            action: None,
        }
    }

    pub fn function_name(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::FunctionName,
            action: None,
        }
    }

    pub fn field_name(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::FieldName,
            action: None,
        }
    }

    pub fn lifetime(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Lifetime,
            action: None,
        }
    }

    pub fn generic(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Generic,
            action: None,
        }
    }

    // Structural element constructors
    pub fn plain(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Plain,
            action: None,
        }
    }

    pub fn punctuation(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Punctuation,
            action: None,
        }
    }

    pub fn operator(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Operator,
            action: None,
        }
    }

    pub fn comment(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Comment,
            action: None,
        }
    }

    pub fn inline_rust_code(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::InlineRustCode,
            action: None,
        }
    }

    pub fn inline_code(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::InlineCode,
            action: None,
        }
    }

    pub fn strong(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Strong,
            action: None,
        }
    }

    pub fn emphasis(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Emphasis,
            action: None,
        }
    }

    pub fn strikethrough(text: impl Into<Cow<'a, str>>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Strikethrough,
            action: None,
        }
    }

    /// Chainable method to attach an action to this span
    pub fn with_action(mut self, action: TuiAction<'a>) -> Self {
        self.action = Some(action);
        self
    }

    /// Attach a navigation action for an already-loaded item
    pub fn with_target(mut self, target: Option<DocRef<'a, Item>>) -> Self {
        if let Some(target) = target {
            self.action = Some(TuiAction::Navigate(target));
        }
        self
    }

    /// Attach a navigation action for an item path (resolved lazily)
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.action = Some(TuiAction::NavigateToPath(path.into()));
        self
    }
}

impl<'a> Document<'a> {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn with_nodes(nodes: Vec<DocumentNode<'a>>) -> Self {
        Self { nodes }
    }
}

impl<'a> Default for Document<'a> {
    fn default() -> Self {
        Self::new()
    }
}

// Ergonomic conversions for building Documents from Spans
impl<'a> From<Span<'a>> for DocumentNode<'a> {
    fn from(span: Span<'a>) -> Self {
        DocumentNode::Span(span)
    }
}

impl<'a> FromIterator<Span<'a>> for Document<'a> {
    fn from_iter<T: IntoIterator<Item = Span<'a>>>(iter: T) -> Self {
        Self {
            nodes: iter.into_iter().map(DocumentNode::Span).collect(),
        }
    }
}

// Into<Document> conversions for ergonomic render() calls
impl<'a> From<Vec<Span<'a>>> for Document<'a> {
    fn from(spans: Vec<Span<'a>>) -> Self {
        Self {
            nodes: spans.into_iter().map(DocumentNode::Span).collect(),
        }
    }
}

impl<'a> From<Vec<DocumentNode<'a>>> for Document<'a> {
    fn from(nodes: Vec<DocumentNode<'a>>) -> Self {
        Self { nodes }
    }
}

impl<'a> From<&'a [Span<'a>]> for Document<'a> {
    fn from(spans: &'a [Span<'a>]) -> Self {
        Self {
            nodes: spans.iter().cloned().map(DocumentNode::Span).collect(),
        }
    }
}

impl<'a, 'b> From<&'b Document<'a>> for Document<'a> {
    fn from(doc: &'b Document<'a>) -> Self {
        doc.clone()
    }
}

impl<'a> ListItem<'a> {
    pub fn new(content: Vec<DocumentNode<'a>>) -> Self {
        Self {
            content,
            label: None,
        }
    }

    pub fn from_span(span: Span<'a>) -> Self {
        Self::new(vec![DocumentNode::Span(span)])
    }

    pub fn labeled(label: Vec<Span<'a>>, content: Vec<DocumentNode<'a>>) -> Self {
        Self {
            label: Some(label),
            content,
        }
    }
}

impl<'a> DocumentNode<'a> {
    /// Convenience constructor for a heading
    pub fn heading(level: HeadingLevel, spans: Vec<Span<'a>>) -> Self {
        DocumentNode::Heading { level, spans }
    }

    /// Convenience constructor for a section with title
    pub fn section(title: Vec<Span<'a>>, nodes: Vec<DocumentNode<'a>>) -> Self {
        DocumentNode::Section {
            title: Some(title),
            nodes,
        }
    }

    /// Convenience constructor for a section without title
    pub fn section_untitled(nodes: Vec<DocumentNode<'a>>) -> Self {
        DocumentNode::Section { title: None, nodes }
    }

    /// Convenience constructor for a list
    pub fn list(items: Vec<ListItem<'a>>) -> Self {
        DocumentNode::List { items }
    }

    /// Convenience constructor for a code block
    pub fn code_block(
        lang: Option<impl Into<Cow<'a, str>>>,
        code: impl Into<Cow<'a, str>>,
    ) -> Self {
        DocumentNode::CodeBlock {
            lang: lang.map(Into::into),
            code: code.into(),
        }
    }

    /// Convenience constructor for a link
    pub fn link(url: String, text: Vec<Span<'a>>) -> Self {
        DocumentNode::Link {
            url,
            text,
            item: None,
        }
    }

    /// Convenience constructor for a horizontal rule
    pub fn horizontal_rule() -> Self {
        DocumentNode::HorizontalRule
    }

    /// Convenience constructor for a block quote
    pub fn block_quote(nodes: Vec<DocumentNode<'a>>) -> Self {
        DocumentNode::BlockQuote { nodes }
    }

    /// Convenience constructor for a table
    pub fn table(header: Option<Vec<TableCell<'a>>>, rows: Vec<Vec<TableCell<'a>>>) -> Self {
        DocumentNode::Table { header, rows }
    }

    /// Convenience constructor for a truncated block
    pub fn truncated_block(nodes: Vec<DocumentNode<'a>>, level: TruncationLevel) -> Self {
        DocumentNode::TruncatedBlock { nodes, level }
    }
}

impl<'a> TableCell<'a> {
    /// Create a new table cell from spans
    pub fn new(spans: Vec<Span<'a>>) -> Self {
        Self { spans }
    }

    /// Create a table cell from a single span
    pub fn from_span(span: Span<'a>) -> Self {
        Self { spans: vec![span] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        let span = Span::keyword("struct");
        assert_eq!(span.text, "struct");
        assert!(matches!(span.style, SpanStyle::Keyword));
    }

    #[test]
    fn test_document_creation() {
        let doc = Document::with_nodes(vec![
            DocumentNode::Span(Span::keyword("struct")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::type_name("Foo")),
        ]);
        assert_eq!(doc.nodes.len(), 3);
    }

    #[test]
    fn test_section() {
        let section = DocumentNode::section(
            vec![Span::plain("Fields:")],
            vec![DocumentNode::list(vec![
                ListItem::from_span(Span::field_name("x")),
                ListItem::from_span(Span::field_name("y")),
            ])],
        );

        if let DocumentNode::Section { title, nodes } = section {
            assert!(title.is_some());
            assert_eq!(nodes.len(), 1);
        } else {
            panic!("Expected section node");
        }
    }

    #[test]
    fn test_list_items() {
        let list = DocumentNode::list(vec![
            ListItem::new(vec![
                DocumentNode::Span(Span::field_name("foo")),
                DocumentNode::Span(Span::punctuation(":")),
                DocumentNode::Span(Span::type_name("u32")),
            ]),
            ListItem::from_span(Span::field_name("bar")),
        ]);

        if let DocumentNode::List { items } = list {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].content.len(), 3);
            assert_eq!(items[1].content.len(), 1);
        } else {
            panic!("Expected list node");
        }
    }

    #[test]
    fn test_heading_levels() {
        let title = DocumentNode::heading(HeadingLevel::Title, vec![Span::plain("Item: Vec")]);
        let section = DocumentNode::heading(HeadingLevel::Section, vec![Span::plain("Methods:")]);

        assert!(matches!(title, DocumentNode::Heading { .. }));
        assert!(matches!(section, DocumentNode::Heading { .. }));
    }

    #[test]
    fn test_code_block() {
        let code = DocumentNode::code_block(Some("rust".to_string()), "fn main() {}".to_string());

        if let DocumentNode::CodeBlock { lang, code } = code {
            assert_eq!(lang, Some("rust".into()));
            assert_eq!(code, "fn main() {}");
        } else {
            panic!("Expected code block");
        }
    }

    #[test]
    fn test_link() {
        let link = DocumentNode::link(
            "https://example.com".to_string(),
            vec![Span::plain("Click here")],
        );

        if let DocumentNode::Link { url, text, .. } = link {
            assert_eq!(url, "https://example.com");
            assert_eq!(text.len(), 1);
        } else {
            panic!("Expected link");
        }
    }
}
