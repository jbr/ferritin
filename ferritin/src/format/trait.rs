use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

impl Request {
    /// Format a trait
    pub(super) fn format_trait<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        trait_data: DocRef<'a, Trait>,
    ) -> Vec<DocumentNode<'a>> {
        let trait_name = item.name().unwrap_or("<unnamed>");

        // Build concise trait signature
        let mut signature_spans = vec![
            Span::keyword("trait"),
            Span::plain(" "),
            Span::type_name(trait_name),
        ];

        if !trait_data.generics.params.is_empty() {
            signature_spans.extend(self.format_generics(item, &trait_data.item().generics));
        }

        if !trait_data.generics.where_predicates.is_empty() {
            signature_spans.extend(
                self.format_where_clause(item, &trait_data.item().generics.where_predicates),
            );
        }

        signature_spans.push(Span::plain(" "));
        signature_spans.push(Span::punctuation("{"));
        signature_spans.push(Span::plain(" ... "));
        signature_spans.push(Span::punctuation("}"));

        let mut nodes: Vec<DocumentNode> = vec![DocumentNode::generated_code(signature_spans)];

        // Build list of trait members
        let mut member_items = vec![];

        for trait_item in item.id_iter(&trait_data.item().items) {
            let item_name = trait_item.name().unwrap_or("<unnamed>");

            let signature_spans = match &trait_item.item().inner {
                ItemEnum::Function(f) => {
                    self.format_trait_method_signature(trait_item, f, item_name)
                }
                ItemEnum::AssocType {
                    generics,
                    bounds,
                    type_,
                } => self.format_trait_assoc_type_signature(
                    item,
                    generics,
                    bounds,
                    type_.as_ref(),
                    item_name,
                ),
                ItemEnum::AssocConst { type_, value } => {
                    self.format_trait_assoc_const_signature(item, type_, value, item_name)
                }
                _ => {
                    // Fallback for unknown item types
                    vec![Span::comment(format!(
                        "// {}: {:?}",
                        item_name, trait_item.inner
                    ))]
                }
            };

            // Prepend signature as a paragraph
            let mut item_content = vec![DocumentNode::paragraph({
                let mut sig = signature_spans;
                sig.push(Span::plain(" "));
                sig
            })];

            // Add docs if available
            if let Some(docs) = self.docs_to_show(trait_item, TruncationLevel::SingleLine) {
                item_content.extend(docs);
            }

            member_items.push(ListItem::new(item_content));
        }

        if !member_items.is_empty() {
            nodes.push(DocumentNode::list(member_items));
        }

        nodes
    }

    fn format_trait_assoc_const_signature<'a>(
        &self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        value: &'a Option<String>,
        const_name: &'a str,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![
            Span::keyword("const"),
            Span::plain(" "),
            Span::plain(const_name),
            Span::punctuation(":"),
            Span::plain(" "),
        ];

        spans.extend(self.format_type(item, type_));

        if let Some(default_val) = value {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("="));
            spans.push(Span::plain(" "));
            spans.push(Span::inline_rust_code(default_val));
        }

        spans.push(Span::punctuation(";"));
        spans
    }

    fn format_trait_assoc_type_signature<'a>(
        &self,
        item: DocRef<'a, Item>,
        generics: &'a Generics,
        bounds: &'a [GenericBound],
        type_: Option<&'a Type>,
        type_name: &'a str,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![
            Span::keyword("type"),
            Span::plain(" "),
            Span::type_name(type_name),
        ];

        if !generics.params.is_empty() {
            spans.extend(self.format_generics(item, generics));
        }

        if !bounds.is_empty() {
            spans.push(Span::punctuation(":"));
            spans.push(Span::plain(" "));
            spans.extend(self.format_generic_bounds(item, bounds));
        }

        if let Some(default_type) = type_ {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("="));
            spans.push(Span::plain(" "));
            spans.extend(self.format_type(item, default_type));
        }

        spans.push(Span::punctuation(";"));
        spans
    }

    fn format_trait_method_signature<'a>(
        &self,
        item: DocRef<'a, Item>,
        f: &'a Function,
        method_name: &'a str,
    ) -> Vec<Span<'a>> {
        let has_default = f.has_body;

        let mut spans = self.format_function_signature(item, method_name, f);

        if has_default {
            spans.push(Span::plain(" "));
            spans.push(Span::punctuation("{"));
            spans.push(Span::plain(" ... "));
            spans.push(Span::punctuation("}"));
        } else {
            spans.push(Span::punctuation(";"));
        }

        spans
    }
}
