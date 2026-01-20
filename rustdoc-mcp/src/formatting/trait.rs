use super::*;

impl Request {
    /// Format a trait
    pub(super) fn format_trait<'a>(
        &self,
        item: DocRef<'a, Item>,
        trait_data: DocRef<'a, Trait>,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();
        let trait_name = item.name.as_deref().unwrap_or("<unnamed>");
        let generics_str = if !trait_data.generics.params.is_empty() {
            self.format_generics(&trait_data.generics)
        } else {
            String::new()
        };
        let where_clause = if !trait_data.generics.where_predicates.is_empty() {
            self.format_where_clause(&trait_data.generics.where_predicates)
        } else {
            String::new()
        };

        result.push_str(&format!(
            "\n```rust\ntrait {trait_name}{generics_str}{where_clause} {{\n"
        ));

        for trait_item in item.id_iter(&trait_data.items) {
            if let Some(docs) = self.docs_to_show(trait_item, false, context) {
                result.push_str(&format!("    /// {docs}\n"));
            }

            match &trait_item.inner {
                ItemEnum::Function(f) => self.format_trait_function(&mut result, f, &trait_item),
                ItemEnum::AssocType {
                    generics,
                    bounds,
                    type_,
                } => self.format_assoc_type(&mut result, generics, bounds, type_, &trait_item),
                ItemEnum::AssocConst { type_, value } => {
                    self.format_assoc_const(&mut result, type_, value, &trait_item)
                }
                _ => {
                    let item_name = trait_item.name.as_deref().unwrap_or("<unnamed>");
                    result.push_str(&format!("    // {}: {:?}\n", item_name, trait_item.inner));
                }
            }
        }

        result.push_str("}\n```\n");
        result
    }

    fn format_assoc_const(
        &self,
        result: &mut String,
        type_: &Type,
        value: &Option<String>,
        trait_item: &Item,
    ) {
        let const_name = trait_item.name.as_deref().unwrap_or("<unnamed>");
        let type_str = self.format_type(type_);
        result.push_str(&format!("    const {const_name}: {type_str}"));
        if let Some(default_val) = value {
            result.push_str(&format!(" = {default_val}"));
        }
        result.push_str(";\n");
    }

    fn format_assoc_type(
        &self,
        result: &mut String,
        generics: &Generics,
        bounds: &[GenericBound],
        type_: &Option<Type>,
        trait_item: &Item,
    ) {
        let type_name = trait_item.name.as_deref().unwrap_or("<unnamed>");
        result.push_str(&format!("    type {type_name}"));
        if !generics.params.is_empty() {
            result.push_str(&self.format_generics(generics));
        }
        if !bounds.is_empty() {
            result.push_str(": ");
            result.push_str(&self.format_generic_bounds(bounds));
        }
        if let Some(default_type) = type_ {
            result.push_str(" = ");
            result.push_str(&self.format_type(default_type));
        }
        result.push_str(";\n");
    }

    fn format_trait_function(&self, result: &mut String, f: &Function, trait_item: &Item) {
        let method_name = trait_item.name.as_deref().unwrap_or("<unnamed>");
        let signature = self.format_function_signature(method_name, f);
        let has_default = f.has_body;
        result.push_str(&format!("    {signature}"));
        if has_default {
            result.push_str(" { ... }\n");
        } else {
            result.push_str(";\n");
        }
    }
}
