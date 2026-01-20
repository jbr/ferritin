use super::*;

impl Request {
    /// Format a type alias
    pub(crate) fn format_type_alias(
        &self,
        item: DocRef<'_, Item>,
        type_alias: DocRef<'_, TypeAlias>,
        _context: &FormatContext,
    ) -> String {
        let type_str = self.format_type(&type_alias.type_);
        format!(
            "\n```rust\ntype {} = {type_str};\n```\n",
            item.name().unwrap_or("<unnamed>")
        )
    }

    /// Format a union
    pub(crate) fn format_union(
        &self,
        _item: DocRef<'_, Item>,
        _union: DocRef<'_, Union>,
        _context: &FormatContext,
    ) -> String {
        // TODO: Implement union formatting
        "\n[Union formatting not yet implemented]\n".to_string()
    }

    /// Format a constant
    pub(crate) fn format_constant(
        &self,
        item: DocRef<'_, Item>,
        type_: &Type,
        const_: &Constant,
        _context: &FormatContext,
    ) -> String {
        let name = item.name().unwrap_or("<unnamed>");
        let type_str = self.format_type(type_);
        let mut result = format!("\n```rust\nconst {name}: {type_str}");
        if let Some(value) = &const_.value {
            result.write_fmt(format_args!(" = {value}"));
        }
        result.push_str(";\n```\n");
        result
    }

    /// Format a static
    pub(crate) fn format_static(
        &self,
        item: DocRef<'_, Item>,
        static_item: &Static,
        _context: &FormatContext,
    ) -> String {
        let type_str = self.format_type(&static_item.type_);
        let result = format!(
            "\n```rust\nstatic {}: {type_str} = {};\n```\n",
            item.name().unwrap_or("<unnamed>"),
            &static_item.expr
        );
        result
    }
}
