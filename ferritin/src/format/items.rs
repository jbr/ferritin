use super::*;
use crate::styled_string::{DocumentNode, Span};

impl Request {
    /// Format a type alias
    pub(crate) fn format_type_alias<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        type_alias: DocRef<'a, TypeAlias>,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");

        let mut spans = vec![
            Span::keyword("type"),
            Span::plain(" "),
            Span::type_name(name),
            Span::plain(" "),
            Span::operator("="),
            Span::plain(" "),
        ];

        // Add type spans
        spans.extend(self.format_type(item, &type_alias.item().type_));

        spans.push(Span::punctuation(";"));

        vec![DocumentNode::generated_code(spans)]
    }

    /// Format a union
    pub(crate) fn format_union<'a>(
        &'a self,
        _item: DocRef<'a, Item>,
        _union: DocRef<'a, Union>,
    ) -> Vec<DocumentNode<'a>> {
        // TODO: Implement union formatting
        vec![DocumentNode::paragraph(vec![Span::plain(
            "[Union formatting not yet implemented]",
        )])]
    }

    /// Format a constant
    pub(crate) fn format_constant<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        const_: &'a Constant,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");

        let mut spans = vec![
            Span::keyword("const"),
            Span::plain(" "),
            Span::plain(name),
            Span::punctuation(":"),
            Span::plain(" "),
        ];

        // Add type spans
        spans.extend(self.format_type(item, type_));

        if let Some(value) = &const_.value {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("="));
            spans.push(Span::plain(" "));
            spans.push(Span::inline_code(value));
        }

        spans.push(Span::punctuation(";"));

        vec![DocumentNode::generated_code(spans)]
    }

    /// Format a static
    pub(crate) fn format_static<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        static_item: &'a Static,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");

        let mut spans = vec![
            Span::keyword("static"),
            Span::plain(" "),
            Span::plain(name),
            Span::punctuation(":"),
            Span::plain(" "),
        ];

        // Add type spans
        spans.extend(self.format_type(item, &static_item.type_));

        spans.push(Span::plain(" "));
        spans.push(Span::operator("="));
        spans.push(Span::plain(" "));
        spans.push(Span::inline_code(&static_item.expr));
        spans.push(Span::punctuation(";"));

        vec![DocumentNode::generated_code(spans)]
    }
}
