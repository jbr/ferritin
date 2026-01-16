use super::*;
use crate::styled_string::{DocumentNode, Span};

impl Request {
    /// Format a trait
    pub(super) fn format_trait<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        trait_data: DocRef<'a, Trait>,
    ) -> Vec<DocumentNode<'a>> {
        let trait_name = item.name().unwrap_or("<unnamed>");

        // Build trait signature
        let mut nodes = vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::Span(Span::keyword("trait")),
            DocumentNode::Span(Span::plain(" ")),
            DocumentNode::Span(Span::type_name(trait_name)),
        ];

        if !trait_data.generics.params.is_empty() {
            nodes.extend(
                self.format_generics(&trait_data.item().generics)
                    .into_iter()
                    .map(DocumentNode::Span),
            );
        }

        if !trait_data.generics.where_predicates.is_empty() {
            nodes.extend(
                self.format_where_clause(&trait_data.item().generics.where_predicates)
                    .into_iter()
                    .map(DocumentNode::Span),
            );
        }

        nodes.push(DocumentNode::Span(Span::plain(" ")));
        nodes.push(DocumentNode::Span(Span::punctuation("{")));
        nodes.push(DocumentNode::Span(Span::plain("\n")));

        // Add trait members
        for trait_item in item.id_iter(&trait_data.item().items) {
            // Add documentation
            if let Some(docs) = self.docs_to_show(trait_item, false) {
                nodes.push(DocumentNode::Span(Span::plain("    ")));
                nodes.extend(docs);
                nodes.push(DocumentNode::Span(Span::plain("\n")));
            }

            let item_name = trait_item.name().unwrap_or("<unnamed>");

            match &trait_item.item().inner {
                ItemEnum::Function(f) => self.format_trait_function(&mut nodes, f, item_name),
                ItemEnum::AssocType {
                    generics,
                    bounds,
                    type_,
                } => self.format_assoc_type(&mut nodes, generics, bounds, type_, item_name),
                ItemEnum::AssocConst { type_, value } => {
                    self.format_assoc_const(&mut nodes, type_, value, item_name)
                }
                _ => {
                    nodes.push(DocumentNode::Span(Span::plain("    ")));
                    nodes.push(DocumentNode::Span(Span::comment(format!(
                        "// {}: {:?}",
                        item_name, trait_item.inner
                    ))));
                    nodes.push(DocumentNode::Span(Span::plain("\n")));
                }
            }
        }

        nodes.push(DocumentNode::Span(Span::punctuation("}")));
        nodes.push(DocumentNode::Span(Span::plain("\n")));

        nodes
    }

    fn format_assoc_const<'a>(
        &self,
        nodes: &mut Vec<DocumentNode<'a>>,
        type_: &'a Type,
        value: &'a Option<String>,
        const_name: &'a str,
    ) {
        nodes.push(DocumentNode::Span(Span::plain("    ")));
        nodes.push(DocumentNode::Span(Span::keyword("const")));
        nodes.push(DocumentNode::Span(Span::plain(" ")));
        nodes.push(DocumentNode::Span(Span::plain(const_name)));
        nodes.push(DocumentNode::Span(Span::punctuation(":")));
        nodes.push(DocumentNode::Span(Span::plain(" ")));
        nodes.extend(self.format_type(type_).into_iter().map(DocumentNode::Span));

        if let Some(default_val) = value {
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.push(DocumentNode::Span(Span::operator("=")));
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.push(DocumentNode::Span(Span::inline_rust_code(default_val)));
        }

        nodes.push(DocumentNode::Span(Span::punctuation(";")));
        nodes.push(DocumentNode::Span(Span::plain("\n")));
    }

    fn format_assoc_type<'a>(
        &self,
        nodes: &mut Vec<DocumentNode<'a>>,
        generics: &'a Generics,
        bounds: &'a [GenericBound],
        type_: &'a Option<Type>,
        type_name: &'a str,
    ) {
        nodes.push(DocumentNode::Span(Span::plain("    ")));
        nodes.push(DocumentNode::Span(Span::keyword("type")));
        nodes.push(DocumentNode::Span(Span::plain(" ")));
        nodes.push(DocumentNode::Span(Span::type_name(type_name)));

        if !generics.params.is_empty() {
            nodes.extend(
                self.format_generics(generics)
                    .into_iter()
                    .map(DocumentNode::Span),
            );
        }

        if !bounds.is_empty() {
            nodes.push(DocumentNode::Span(Span::punctuation(":")));
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.extend(
                self.format_generic_bounds(bounds)
                    .into_iter()
                    .map(DocumentNode::Span),
            );
        }

        if let Some(default_type) = type_ {
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.push(DocumentNode::Span(Span::operator("=")));
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.extend(
                self.format_type(default_type)
                    .into_iter()
                    .map(DocumentNode::Span),
            );
        }

        nodes.push(DocumentNode::Span(Span::punctuation(";")));
        nodes.push(DocumentNode::Span(Span::plain("\n")));
    }

    fn format_trait_function<'a>(
        &self,
        nodes: &mut Vec<DocumentNode<'a>>,
        f: &'a Function,
        method_name: &'a str,
    ) {
        let has_default = f.has_body;

        nodes.push(DocumentNode::Span(Span::plain("    ")));
        nodes.extend(
            self.format_function_signature(method_name, f)
                .into_iter()
                .map(DocumentNode::Span),
        );

        if has_default {
            nodes.push(DocumentNode::Span(Span::plain(" ")));
            nodes.push(DocumentNode::Span(Span::punctuation("{")));
            nodes.push(DocumentNode::Span(Span::plain(" ... ")));
            nodes.push(DocumentNode::Span(Span::punctuation("}")));
        } else {
            nodes.push(DocumentNode::Span(Span::punctuation(";")));
        }

        nodes.push(DocumentNode::Span(Span::plain("\n")));
    }
}
