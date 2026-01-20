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

        let mut doc_nodes = vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::Span(Span::keyword("type")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::type_name(name)),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::operator("=")),
            DocumentNode::Span(Span::plain(" ")),
        ];

        // Add type spans
        doc_nodes.extend(
            self.format_type(item, &type_alias.item().type_)
                .into_iter()
                .map(DocumentNode::Span),
        );

        doc_nodes.push(DocumentNode::Span(Span::punctuation(";")));
        doc_nodes.push(DocumentNode::Span(Span::plain("\n")));

        doc_nodes
    }

    /// Format a union
    pub(crate) fn format_union<'a>(
        &'a self,
        _item: DocRef<'a, Item>,
        _union: DocRef<'a, Union>,
    ) -> Vec<DocumentNode<'a>> {
        // TODO: Implement union formatting
        vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::Span(Span::plain("[Union formatting not yet implemented]")),
            DocumentNode::Span(Span::plain("\n")),
        ]
    }

    /// Format a constant
    pub(crate) fn format_constant<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        const_: &'a Constant,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");

        let mut doc_nodes = vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::Span(Span::keyword("const")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::plain(name)),
            DocumentNode::Span(Span::punctuation(":")),
            DocumentNode::Span(Span::plain(" ")),
        ];

        // Add type spans
        doc_nodes.extend(
            self.format_type(item, type_)
                .into_iter()
                .map(DocumentNode::Span),
        );

        if let Some(value) = &const_.value {
            doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
            doc_nodes.push(DocumentNode::Span(Span::operator("=")));
            doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
            doc_nodes.push(DocumentNode::Span(Span::inline_code(value)));
        }

        doc_nodes.push(DocumentNode::Span(Span::punctuation(";")));
        doc_nodes.push(DocumentNode::Span(Span::plain("\n")));

        doc_nodes
    }

    /// Format a static
    pub(crate) fn format_static<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        static_item: &'a Static,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");

        let mut doc_nodes = vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::Span(Span::keyword("static")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::plain(name)),
            DocumentNode::Span(Span::punctuation(":")),
            DocumentNode::Span(Span::plain(" ")),
        ];

        // Add type spans
        doc_nodes.extend(
            self.format_type(item, &static_item.type_)
                .into_iter()
                .map(DocumentNode::Span),
        );

        doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
        doc_nodes.push(DocumentNode::Span(Span::operator("=")));
        doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
        doc_nodes.push(DocumentNode::Span(Span::inline_code(&static_item.expr)));
        doc_nodes.push(DocumentNode::Span(Span::punctuation(";")));
        doc_nodes.push(DocumentNode::Span(Span::plain("\n")));

        doc_nodes
    }
}
