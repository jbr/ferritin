use rustdoc_types::ItemKind;

use super::*;

// Define display order for groups
const GROUP_ORDER: &[(ItemKind, &str)] = &[
    (ItemKind::Module, "Modules"),
    (ItemKind::Struct, "Structs"),
    (ItemKind::Enum, "Enums"),
    (ItemKind::Trait, "Traits"),
    (ItemKind::Union, "Unions"),
    (ItemKind::TypeAlias, "Type Aliases"),
    (ItemKind::Function, "Functions"),
    (ItemKind::Constant, "Constants"),
    (ItemKind::Static, "Statics"),
    (ItemKind::Macro, "Macros"),
    (ItemKind::Variant, "Variants"),
];

#[derive(Debug)]
struct FlatItem<'a> {
    path: String,
    item: DocRef<'a, Item>,
}

impl Request {
    /// Collect all items in a module hierarchy as flat qualified paths
    fn collect_flat_items<'a>(
        collected: &mut Vec<FlatItem<'a>>,
        path: Option<String>,
        item: DocRef<'a, Item>,
        context: &FormatContext,
    ) {
        for child in item.child_items() {
            if let Some(item_name) = child.name()
                && context.filter_match_kind(child.kind())
            {
                let path = path.as_deref().map_or_else(
                    || item_name.to_string(),
                    |path| format!("{path}::{item_name}"),
                );

                collected.push(FlatItem {
                    path: path.clone(),
                    item: child,
                });

                if context.is_recursive() {
                    Self::collect_flat_items(collected, Some(path), child, context);
                }
            }
        }
    }

    /// Format collected flat items with grouping by type
    fn format_grouped_flat_items(&self, items: &[FlatItem], context: &FormatContext) -> String {
        if items.is_empty() {
            return "\nNo items match the current filters.\n".to_string();
        }

        // Group items by filter type
        let mut groups: HashMap<ItemKind, Vec<&FlatItem>> = HashMap::new();
        for flat_item in items {
            let kind = flat_item.item.kind();
            groups.entry(kind).or_default().push(flat_item);
        }

        let mut result = String::new();

        for (kind, group_name) in GROUP_ORDER {
            if context.filter_match_kind(*kind)
                && let Some(mut group_items) = groups.remove(kind)
                && !group_items.is_empty()
            {
                result.write_fmt(format_args!("\n{group_name}:\n"));

                group_items.sort_by_key(|a| &a.path);

                for flat_item in group_items {
                    result.push_str(&self.format_flat_item_line(flat_item, context));
                }
            }
        }

        for (kind, mut group_items) in groups {
            result.write_fmt(format_args!("\n{kind:?}:\n"));

            group_items.sort_by_key(|a| &a.path);

            for flat_item in group_items {
                result.push_str(&self.format_flat_item_line(flat_item, context));
            }
        }

        result
    }

    /// Format a single flat item line
    fn format_flat_item_line(&self, flat_item: &FlatItem, context: &FormatContext) -> String {
        let mut line = flat_item.path.to_string();

        // Add brief documentation if available
        if let Some(docs) = self.docs_to_show(flat_item.item, true, context) {
            line.push_str(" // ");
            line.push_str(&docs);
        }

        line.push('\n');
        line
    }

    /// Format a module
    pub(super) fn format_module(&self, item: DocRef<'_, Item>, context: &FormatContext) -> String {
        let mut result = String::new();

        let mut collected = Vec::new();
        Self::collect_flat_items(&mut collected, None, item, context);
        result.push_str(&self.format_grouped_flat_items(&collected, context));

        result
    }
}
