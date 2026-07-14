use super::*;
use rustdoc_types::ItemKind;

// Display order for groups. Each group has a label and the set of ItemKinds it
// collects — so we can merge e.g. the three macro-flavored kinds under a single
// "Macros" heading rather than exposing rustdoc's internal distinction.
const GROUP_ORDER: &[(&str, &[ItemKind])] = &[
    ("Modules", &[ItemKind::Module]),
    ("Structs", &[ItemKind::Struct]),
    ("Enums", &[ItemKind::Enum]),
    ("Traits", &[ItemKind::Trait]),
    ("Unions", &[ItemKind::Union]),
    ("Type Aliases", &[ItemKind::TypeAlias]),
    ("Functions", &[ItemKind::Function]),
    ("Constants", &[ItemKind::Constant]),
    ("Statics", &[ItemKind::Static]),
    (
        "Macros",
        &[
            ItemKind::Macro,
            ItemKind::ProcAttribute,
            ItemKind::ProcDerive,
        ],
    ),
    ("Primitives", &[ItemKind::Primitive]),
    ("Variants", &[ItemKind::Variant]),
];

#[derive(Debug)]
struct FlatItem<'a> {
    path: String,
    item: DocRef<'a, Item>,
}

impl<'a> Request<'a> {
    /// Collect all items in a module hierarchy as flat qualified paths.
    ///
    /// Tracks visited items so cyclic re-export chains (e.g. a nested module
    /// glob-importing its parent) can't send the recursion into an infinite
    /// loop.
    // DocRef hashes by crate name + item id; the interior mutability lives in
    // Navigator's connection pool and doesn't affect identity.
    #[allow(clippy::mutable_key_type)]
    fn collect_flat_items(
        &mut self,
        collected: &mut Vec<FlatItem<'a>>,
        visited: &mut std::collections::HashSet<DocRef<'a, Item>>,
        path: Option<String>,
        item: DocRef<'a, Item>,
        context: &FormatContext,
    ) {
        for child in self.children(item) {
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

                if context.is_recursive() && visited.insert(child) {
                    self.collect_flat_items(collected, visited, Some(path), child, context);
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

        for (group_name, kinds) in GROUP_ORDER {
            let mut group_items: Vec<&FlatItem> = kinds
                .iter()
                .filter_map(|kind| groups.remove(kind))
                .flatten()
                .collect();

            if group_items.is_empty() {
                continue;
            }

            result.write_fmt(format_args!("\n{group_name}:\n"));

            group_items.sort_by_key(|a| &a.path);

            for flat_item in group_items {
                result.push_str(&self.format_flat_item_line(flat_item, context));
            }
        }

        // Sort remaining (unrecognized) kinds alphabetically by their debug name
        // so output is stable across runs — HashMap iteration order is not.
        let mut remaining: Vec<_> = groups.into_iter().collect();
        remaining.sort_by_key(|(kind, _)| format!("{kind:?}"));

        for (kind, mut group_items) in remaining {
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
    #[allow(clippy::mutable_key_type)] // see collect_flat_items
    pub(super) fn format_module(
        &mut self,
        item: DocRef<'a, Item>,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();

        let mut collected = Vec::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(item);
        self.collect_flat_items(&mut collected, &mut visited, None, item, context);
        result.push_str(&self.format_grouped_flat_items(&collected, context));

        result
    }
}
