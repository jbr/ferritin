use super::*;
use crate::styled_string::DocumentNode;

impl Request {
    /// Format an enum
    pub(super) fn format_enum<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        enum_data: DocRef<'a, Enum>,
    ) -> Vec<DocumentNode<'a>> {
        use crate::styled_string::{DocumentNode, ListItem, Span};
        let enum_name = item.name().unwrap_or("<unnamed>");

        // Build signature spans
        let mut code_spans = vec![
            Span::keyword("enum"),
            Span::plain(" "),
            Span::type_name(enum_name),
        ];

        if !enum_data.generics.params.is_empty() {
            code_spans.extend(self.format_generics(&enum_data.item().generics));
        }

        if !enum_data.generics.where_predicates.is_empty() {
            code_spans
                .extend(self.format_where_clause(&enum_data.item().generics.where_predicates));
        }

        code_spans.push(Span::plain(" "));
        code_spans.push(Span::punctuation("{"));
        code_spans.push(Span::plain("\n"));

        // Format variants
        for variant in item.id_iter(&enum_data.item().variants) {
            if let ItemEnum::Variant(variant_enum) = &variant.item().inner {
                let variant_name = variant.name().unwrap_or("<unnamed>");

                match &variant_enum.kind {
                    VariantKind::Plain => {
                        code_spans.push(Span::plain("    "));
                        code_spans.push(Span::type_name(variant_name));
                        code_spans.push(Span::punctuation(","));
                        code_spans.push(Span::plain("\n"));
                    }
                    VariantKind::Tuple(fields) => {
                        code_spans.push(Span::plain("    "));
                        code_spans.push(Span::type_name(variant_name));
                        code_spans.push(Span::punctuation("("));

                        let mut first = true;
                        for field_id in fields.iter().copied().flatten() {
                            if let Some(field) = enum_data.get(&field_id)
                                && let ItemEnum::StructField(field_type) = &field.item().inner
                            {
                                if !first {
                                    code_spans.push(Span::punctuation(","));
                                    code_spans.push(Span::plain(" "));
                                }
                                first = false;
                                code_spans.extend(self.format_type(field_type));
                            }
                        }

                        code_spans.push(Span::punctuation(")"));
                        code_spans.push(Span::punctuation(","));
                        code_spans.push(Span::plain("\n"));
                    }
                    VariantKind::Struct { fields, .. } => {
                        code_spans.push(Span::plain("    "));
                        code_spans.push(Span::type_name(variant_name));
                        code_spans.push(Span::plain(" "));
                        code_spans.push(Span::punctuation("{"));
                        code_spans.push(Span::plain("\n"));

                        for field in item.id_iter(fields) {
                            if let ItemEnum::StructField(field_type) = &field.item().inner {
                                let field_name = field.name().unwrap_or("<unnamed>");
                                code_spans.push(Span::plain("        "));
                                code_spans.push(Span::field_name(field_name));
                                code_spans.push(Span::punctuation(":"));
                                code_spans.push(Span::plain(" "));
                                code_spans.extend(self.format_type(field_type));
                                code_spans.push(Span::punctuation(","));
                                code_spans.push(Span::plain("\n"));
                            }
                        }

                        code_spans.push(Span::plain("    "));
                        code_spans.push(Span::punctuation("}"));
                        code_spans.push(Span::punctuation(","));
                        code_spans.push(Span::plain("\n"));
                    }
                }
            }
        }

        code_spans.push(Span::punctuation("}"));

        // Build document nodes
        let mut doc_nodes = vec![];

        // Add signature as spans
        for span in code_spans {
            doc_nodes.push(DocumentNode::Span(span));
        }
        doc_nodes.push(DocumentNode::Span(Span::plain("\n\n")));

        // Build variants section with List (collect documented variants)
        let variant_items: Vec<ListItem> = item
            .id_iter(&enum_data.item().variants)
            .filter_map(|variant| {
                if let ItemEnum::Variant(_) = &variant.inner
                    && let Some(docs) = self.docs_to_show(variant, false)
                {
                    let variant_name = variant.name().unwrap_or("<unnamed>");
                    let mut item_nodes = vec![
                        DocumentNode::Span(Span::type_name(variant_name)),
                        DocumentNode::Span(Span::plain("\n")),
                    ];
                    // TODO: Re-add indentation for docs
                    item_nodes.extend(docs);
                    return Some(ListItem::new(item_nodes));
                }
                None
            })
            .collect();

        if !variant_items.is_empty() {
            let variants_section = DocumentNode::section(
                vec![Span::plain("Variants:")],
                vec![DocumentNode::list(variant_items)],
            );
            doc_nodes.push(variants_section);
        }

        doc_nodes.extend(self.format_associated_methods(item));

        doc_nodes
    }
}
