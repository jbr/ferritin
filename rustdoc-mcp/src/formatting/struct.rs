use super::*;

impl Request {
    pub(super) fn format_struct<'a>(
        &self,
        item: DocRef<'a, Item>,
        r#struct: DocRef<'a, Struct>,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();
        match &r#struct.kind {
            StructKind::Unit => self.format_unit_struct(r#struct, &mut result, item),
            StructKind::Tuple(fields) => {
                self.format_tuple_struct(r#struct, &mut result, item, fields, context)
            }
            StructKind::Plain { fields, .. } => {
                self.format_plain_struct(r#struct, &mut result, item, fields, context)
            }
        }

        result.push_str(&self.format_associated_methods(item, context));

        result
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
        result: &mut String,
        item: DocRef<'a, Item>,
        fields: &[Id],
        context: &FormatContext,
    ) {
        let (visible_fields, hidden_count) = self.categorize_fields(item, fields);
        let struct_name = item.name.as_deref().unwrap_or("<unnamed>");
        let generics_str = if !struct_data.generics.params.is_empty() {
            self.format_generics(&struct_data.generics)
        } else {
            String::new()
        };
        let where_clause = if !struct_data.generics.where_predicates.is_empty() {
            self.format_where_clause(&struct_data.generics.where_predicates)
        } else {
            String::new()
        };
        result.write_fmt(format_args!(
            "\n```rust\nstruct {struct_name}{generics_str}{where_clause} {{\n"
        ));
        for field in &visible_fields {
            let field_name = field.name.as_deref().unwrap_or("<unnamed>");
            if let ItemEnum::StructField(field_type) = &field.inner {
                let type_str = self.format_type(field_type);
                let visibility = match field.visibility {
                    Visibility::Public => "pub ",
                    _ => "",
                };
                result.write_fmt(format_args!("    {visibility}{field_name}: {type_str},\n"));
            }
        }

        if hidden_count > 0 {
            result.write_fmt(format_args!(
                "    // ... {} private field{} hidden\n",
                hidden_count,
                if hidden_count == 1 { "" } else { "s" }
            ));
        }
        result.push_str("}\n```\n\n");

        let fields_to_show = visible_fields
            .iter()
            .filter_map(|field| {
                if let ItemEnum::StructField(field_type) = &field.inner
                    && let Some(name) = &field.name
                    && let Some(docs) = self.docs_to_show(*field, false, context)
                {
                    Some((name, docs, field_type))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if !fields_to_show.is_empty() {
            result.push_str("Fields:\n\n");
        }

        for (name, docs, field_type) in &fields_to_show {
            let type_str = self.format_type(field_type);
            result.write_fmt(format_args!(
                "• {name}: {type_str}\n{}\n",
                Indent::new(docs, 4)
            ));
        }
    }

    fn format_tuple_struct(
        &self,
        struct_data: DocRef<'_, Struct>,
        result: &mut String,
        item: DocRef<'_, Item>,
        fields: &[Option<Id>],
        context: &FormatContext,
    ) {
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

        let struct_name = item.name.as_deref().unwrap_or("<unnamed>");
        let generics_str = if !struct_data.generics.params.is_empty() {
            self.format_generics(&struct_data.generics)
        } else {
            String::new()
        };
        let where_clause = if !struct_data.generics.where_predicates.is_empty() {
            self.format_where_clause(&struct_data.generics.where_predicates)
        } else {
            String::new()
        };
        result.write_fmt(format_args!(
            "\n```rust\nstruct {struct_name}{generics_str}{where_clause}(\n"
        ));
        for (i, field) in &visible_fields {
            if let ItemEnum::StructField(field_type) = &field.inner {
                let type_str = self.format_type(field_type);
                let visibility = match field.visibility {
                    Visibility::Public => "pub ",
                    _ => "",
                };
                result.write_fmt(format_args!("    {visibility}{type_str}, // field {i}\n"));
            }
        }
        if hidden_count > 0 {
            result.write_fmt(format_args!(
                "    // ... {} private field{} hidden\n",
                hidden_count,
                if hidden_count == 1 { "" } else { "s" }
            ));
        }
        result.push_str(");\n```\n");

        let fields_to_show = visible_fields
            .iter()
            .filter_map(|(i, field)| {
                if let ItemEnum::StructField(field_type) = field.inner()
                    && let Some(docs) = self.docs_to_show(*field, false, context)
                {
                    Some((i, field_type, docs))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if !fields_to_show.is_empty() {
            result.push_str("Fields:\n");
        }
        for (i, field_type, docs) in fields_to_show {
            let type_str = self.format_type(field_type);
            result.write_fmt(format_args!(
                "• Field {i}: {type_str}\n {}\n",
                Indent::new(&docs, 4)
            ));
        }
    }

    fn format_unit_struct(
        &self,
        struct_data: DocRef<'_, Struct>,
        result: &mut String,
        item: DocRef<'_, Item>,
    ) {
        let struct_name = item.name.as_deref().unwrap_or("<unnamed>");
        let generics_str = if !struct_data.generics.params.is_empty() {
            self.format_generics(&struct_data.generics)
        } else {
            String::new()
        };
        let where_clause = if !struct_data.generics.where_predicates.is_empty() {
            self.format_where_clause(&struct_data.generics.where_predicates)
        } else {
            String::new()
        };
        result.write_fmt(format_args!(
            "\n```rust\nstruct {struct_name}{generics_str}{where_clause};\n```\n"
        ));
    }
}
