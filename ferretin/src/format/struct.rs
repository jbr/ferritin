use super::*;
use crate::styled_string::DocumentNode;

impl Request {
    pub(super) fn format_struct<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        r#struct: DocRef<'a, Struct>,
    ) -> Vec<DocumentNode<'a>> {
        let mut doc_nodes = match &r#struct.kind {
            StructKind::Unit => self.format_unit_struct(r#struct, item),
            StructKind::Tuple(fields) => self.format_tuple_struct(r#struct, item, fields),
            StructKind::Plain { fields, .. } => self.format_plain_struct(r#struct, item, fields),
        };

        doc_nodes.extend(self.format_associated_methods(item));

        doc_nodes
    }

    /// Categorize struct fields into visible and hidden counts
    fn categorize_fields<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        fields: &[Id],
    ) -> (Vec<DocRef<'a, Item>>, usize) {
        let mut visible_fields = Vec::new();
        let mut hidden_count = 0;

        for field_id in fields {
            if let Some(field) = item.get(field_id) {
                visible_fields.push(field);
            } else {
                hidden_count += 1;
            }
        }

        (visible_fields, hidden_count)
    }

    fn format_plain_struct<'a>(
        &'a self,
        struct_data: DocRef<'a, Struct>,
        item: DocRef<'a, Item>,
        fields: &[Id],
    ) -> Vec<DocumentNode<'a>> {
        use crate::styled_string::{DocumentNode, ListItem, Span};

        let (visible_fields, hidden_count) = self.categorize_fields(item, fields);
        let struct_name = item.name().unwrap_or("<unnamed>");

        let mut code_spans = vec![
            Span::keyword("struct"),
            Span::plain(" "),
            Span::type_name(struct_name),
        ];

        if !struct_data.generics.params.is_empty() {
            code_spans.extend(self.format_generics(&struct_data.item().generics));
        }

        if !struct_data.generics.where_predicates.is_empty() {
            code_spans
                .extend(self.format_where_clause(&struct_data.item().generics.where_predicates));
        }

        code_spans.push(Span::plain(" "));
        code_spans.push(Span::punctuation("{"));
        code_spans.push(Span::plain("\n"));

        for field in &visible_fields {
            let field_name = field.name().unwrap_or("<unnamed>");
            if let ItemEnum::StructField(field_type) = &field.item().inner {
                let visibility = match field.item().visibility {
                    Visibility::Public => "pub ",
                    _ => "",
                };
                code_spans.push(Span::plain("    "));
                if !visibility.is_empty() {
                    code_spans.push(Span::keyword(visibility.trim()));
                    code_spans.push(Span::plain(" "));
                }
                code_spans.push(Span::field_name(field_name));
                code_spans.push(Span::punctuation(":"));
                code_spans.push(Span::plain(" "));
                code_spans.extend(self.format_type(field_type));
                code_spans.push(Span::punctuation(","));
                code_spans.push(Span::plain("\n"));
            }
        }

        if hidden_count > 0 {
            code_spans.push(Span::plain("    "));
            code_spans.push(Span::comment(format!(
                "// ... {} private field{} hidden",
                hidden_count,
                if hidden_count == 1 { "" } else { "s" }
            )));
            code_spans.push(Span::plain("\n"));
        }

        code_spans.push(Span::punctuation("}"));

        // Build document nodes
        let mut doc_nodes = vec![];

        // Add signature as spans
        for span in code_spans {
            doc_nodes.push(DocumentNode::Span(span));
        }
        doc_nodes.push(DocumentNode::Span(Span::plain("\n\n")));

        // Build fields section with List
        let field_items: Vec<ListItem> = visible_fields
            .iter()
            .filter_map(|field| {
                if let ItemEnum::StructField(field_type) = &field.item().inner
                    && let Some(name) = field.name()
                    && let Some(docs) = self.docs_to_show(*field, false)
                {
                    let mut item_nodes = vec![
                        DocumentNode::Span(Span::field_name(name)),
                        DocumentNode::Span(Span::punctuation(":")),
                        DocumentNode::Span(Span::plain(" ")),
                    ];
                    // Convert Vec<Span> to Vec<DocumentNode>
                    let type_spans: Vec<DocumentNode> = self
                        .format_type(field_type)
                        .into_iter()
                        .map(DocumentNode::Span)
                        .collect();
                    item_nodes.extend(type_spans);
                    item_nodes.push(DocumentNode::Span(Span::plain("\n")));
                    // TODO: Re-add indentation for docs
                    item_nodes.extend(docs);
                    Some(ListItem::new(item_nodes))
                } else {
                    None
                }
            })
            .collect();

        if !field_items.is_empty() {
            let fields_section = DocumentNode::section(
                vec![Span::plain("Fields:")],
                vec![DocumentNode::list(field_items)],
            );
            doc_nodes.push(fields_section);
        }

        doc_nodes
    }

    fn format_tuple_struct<'a>(
        &'a self,
        struct_data: DocRef<'a, Struct>,
        item: DocRef<'a, Item>,
        fields: &[Option<Id>],
    ) -> Vec<DocumentNode<'a>> {
        use crate::styled_string::{DocumentNode, ListItem, Span};

        let mut visible_fields = Vec::new();
        let mut hidden_count = 0;
        for (i, field_id_opt) in fields.iter().enumerate() {
            if let Some(field_id) = field_id_opt
                && let Some(field) = struct_data.get(field_id)
            {
                visible_fields.push((i, field));
            } else {
                hidden_count += 1;
            }
        }

        let struct_name = item.name().unwrap_or("<unnamed>");

        let mut code_spans = vec![
            Span::keyword("struct"),
            Span::plain(" "),
            Span::type_name(struct_name),
        ];

        if !struct_data.generics.params.is_empty() {
            code_spans.extend(self.format_generics(&struct_data.item().generics));
        }

        if !struct_data.generics.where_predicates.is_empty() {
            code_spans
                .extend(self.format_where_clause(&struct_data.item().generics.where_predicates));
        }

        code_spans.push(Span::punctuation("("));
        code_spans.push(Span::plain("\n"));

        for (i, field) in &visible_fields {
            if let ItemEnum::StructField(field_type) = &field.item().inner {
                let visibility = match field.visibility {
                    Visibility::Public => "pub ",
                    _ => "",
                };
                code_spans.push(Span::plain("    "));
                if !visibility.is_empty() {
                    code_spans.push(Span::keyword(visibility.trim()));
                    code_spans.push(Span::plain(" "));
                }
                code_spans.extend(self.format_type(field_type));
                code_spans.push(Span::punctuation(","));
                code_spans.push(Span::plain(" "));
                code_spans.push(Span::comment(format!("// field {i}")));
                code_spans.push(Span::plain("\n"));
            }
        }

        if hidden_count > 0 {
            code_spans.push(Span::plain("    "));
            code_spans.push(Span::comment(format!(
                "// ... {} private field{} hidden",
                hidden_count,
                if hidden_count == 1 { "" } else { "s" }
            )));
            code_spans.push(Span::plain("\n"));
        }

        code_spans.push(Span::punctuation(")"));

        // Build document nodes
        let mut doc_nodes = vec![];

        // Add signature as spans
        for span in code_spans {
            doc_nodes.push(DocumentNode::Span(span));
        }
        doc_nodes.push(DocumentNode::Span(Span::plain("\n\n")));

        // Build fields section with List
        let field_items: Vec<ListItem> = visible_fields
            .iter()
            .filter_map(|(i, field)| {
                if let ItemEnum::StructField(field_type) = field.inner()
                    && let Some(docs) = self.docs_to_show(*field, false)
                {
                    let mut item_nodes =
                        vec![DocumentNode::Span(Span::plain(format!("Field {}: ", i)))];
                    // Convert Vec<Span> to Vec<DocumentNode>
                    let type_spans: Vec<DocumentNode> = self
                        .format_type(field_type)
                        .into_iter()
                        .map(DocumentNode::Span)
                        .collect();
                    item_nodes.extend(type_spans);
                    item_nodes.push(DocumentNode::Span(Span::plain("\n")));
                    // TODO: Re-add indentation for docs
                    item_nodes.extend(docs);
                    Some(ListItem::new(item_nodes))
                } else {
                    None
                }
            })
            .collect();

        if !field_items.is_empty() {
            let fields_section = DocumentNode::section(
                vec![Span::plain("Fields:")],
                vec![DocumentNode::list(field_items)],
            );
            doc_nodes.push(fields_section);
        }

        doc_nodes
    }

    fn format_unit_struct<'a>(
        &'a self,
        struct_data: DocRef<'a, Struct>,
        item: DocRef<'a, Item>,
    ) -> Vec<DocumentNode<'a>> {
        use crate::styled_string::{DocumentNode, Span};

        let struct_name = item.name().unwrap_or("<unnamed>");

        let mut code_spans = vec![
            Span::keyword("struct"),
            Span::plain(" "),
            Span::type_name(struct_name),
        ];

        if !struct_data.generics.params.is_empty() {
            code_spans.extend(self.format_generics(&struct_data.item().generics));
        }

        if !struct_data.generics.where_predicates.is_empty() {
            code_spans
                .extend(self.format_where_clause(&struct_data.item().generics.where_predicates));
        }

        code_spans.push(Span::punctuation(";"));

        code_spans.into_iter().map(DocumentNode::Span).collect()
    }
}
