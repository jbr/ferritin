use ferretin_common::iterators::IdIter;

use super::*;

impl Request {
    /// Format an enum
    pub(super) fn format_enum(
        &self,
        item: DocRef<'_, Item>,
        enum_data: DocRef<'_, Enum>,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();
        let enum_name = item.name.as_deref().unwrap_or("<unnamed>");
        let generics_str = if !enum_data.generics.params.is_empty() {
            self.format_generics(&enum_data.generics)
        } else {
            String::new()
        };
        let where_clause = if !enum_data.generics.where_predicates.is_empty() {
            self.format_where_clause(&enum_data.generics.where_predicates)
        } else {
            String::new()
        };

        result.push_str(&format!(
            "\n```rust\nenum {enum_name}{generics_str}{where_clause} {{\n"
        ));

        for variant in item.id_iter(&enum_data.variants) {
            if let ItemEnum::Variant(variant_enum) = &variant.inner {
                if let Some(docs) = &variant.docs {
                    result.push_str(&format!("    /// {docs}\n"));
                }

                let variant_name = variant.name.as_deref().unwrap_or("<unnamed>");

                match &variant_enum.kind {
                    VariantKind::Plain => {
                        result.push_str(&format!("    {variant_name},\n"));
                    }
                    VariantKind::Tuple(fields) => {
                        self.format_tuple_enum(enum_data, &mut result, variant_name, fields)
                    }
                    VariantKind::Struct { fields, .. } => {
                        self.format_struct_enum(&mut result, variant_name, item.id_iter(fields))
                    }
                }
            }
        }

        result.push_str("}\n```\n");

        result.push_str(&self.format_associated_methods(item, context));

        result
    }

    fn format_struct_enum<T>(
        &self,
        result: &mut String,
        variant_name: &str,
        fields: IdIter<'_, T>,
    ) {
        result.push_str(&format!("    {variant_name} {{\n"));
        for field in fields {
            if let ItemEnum::StructField(field_type) = &field.inner {
                let field_name = field.name.as_deref().unwrap_or("<unnamed>");
                let type_str = self.format_type(field_type);
                result.push_str(&format!("        {field_name}: {type_str},\n"));
            }
        }
        result.push_str("    },\n");
    }

    fn format_tuple_enum(
        &self,
        enum_data: DocRef<'_, Enum>,
        result: &mut String,
        variant_name: &str,
        fields: &[Option<Id>],
    ) {
        result.push_str(&format!("    {variant_name}("));
        let mut field_types = vec![];
        for field_id in fields.iter().copied().flatten() {
            if let Some(field) = enum_data.get(&field_id)
                && let ItemEnum::StructField(field_type) = &field.inner
            {
                field_types.push(self.format_type(field_type));
            }
        }
        result.push_str(&field_types.join(", "));
        result.push_str("),\n");
    }
}
