use rustdoc_types::ItemKind;

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

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
    /// Tracks visited items during recursive descent so cyclic re-export
    /// chains (e.g. a nested module glob-importing its parent) can't send the
    /// traversal into an infinite loop.
    // DocRef hashes by crate name + item id; the interior mutability lives in
    // Navigator's connection pool and doesn't affect identity.
    #[allow(clippy::mutable_key_type)]
    fn collect_flat_items(
        &mut self,
        collected: &mut Vec<FlatItem<'a>>,
        visited: &mut std::collections::HashSet<DocRef<'a, Item>>,
        path: Option<String>,
        item: DocRef<'a, Item>,
    ) {
        for child in self.children(item) {
            if self.hidden_by_visibility(child) {
                continue;
            }
            if let Some(item_name) = child.name() {
                let path = path.as_deref().map_or_else(
                    || item_name.to_string(),
                    |path| format!("{path}::{item_name}"),
                );

                collected.push(FlatItem {
                    path: path.clone(),
                    item: child,
                });

                if self.format_context().is_recursive() && visited.insert(child) {
                    self.collect_flat_items(collected, visited, Some(path), child);
                }
            }
        }
    }

    /// Format collected flat items with grouping by type
    fn format_grouped_flat_items(&mut self, items: &[FlatItem<'a>]) -> Vec<DocumentNode<'a>> {
        if items.is_empty() {
            return vec![DocumentNode::paragraph(vec![Span::plain(
                "No items match the current filters.",
            )])];
        }

        // Group items by filter type
        let mut groups: HashMap<ItemKind, Vec<&FlatItem>> = HashMap::new();
        for flat_item in items {
            let kind = flat_item.item.kind();
            groups.entry(kind).or_default().push(flat_item);
        }

        let mut doc_nodes = vec![];

        for (group_name, kinds) in GROUP_ORDER {
            let mut group_items: Vec<&FlatItem> = kinds
                .iter()
                .filter_map(|kind| groups.remove(kind))
                .flatten()
                .collect();

            if group_items.is_empty() {
                continue;
            }

            group_items.sort_by_key(|a| &a.path);

            let list_items: Vec<ListItem> = group_items
                .iter()
                .map(|flat_item| self.format_flat_item(flat_item))
                .collect();

            let section = DocumentNode::section(
                vec![Span::plain(*group_name)],
                vec![DocumentNode::list(list_items)],
            );
            doc_nodes.push(section);
        }

        // Sort remaining (unrecognized) kinds alphabetically by their debug name
        // so output is stable across runs — HashMap iteration order is not.
        let mut remaining: Vec<_> = groups.into_iter().collect();
        remaining.sort_by_key(|(kind, _)| format!("{kind:?}"));

        for (kind, mut group_items) in remaining {
            group_items.sort_by_key(|a| &a.path);

            let list_items: Vec<ListItem> = group_items
                .iter()
                .map(|flat_item| self.format_flat_item(flat_item))
                .collect();

            let section = DocumentNode::section(
                vec![Span::plain(format!("{kind:?}"))],
                vec![DocumentNode::list(list_items)],
            );
            doc_nodes.push(section);
        }

        doc_nodes
    }

    /// Format a single flat item as a ListItem
    fn format_flat_item(&mut self, flat_item: &FlatItem<'a>) -> ListItem<'a> {
        // Prepend item name as a paragraph
        let mut content = vec![DocumentNode::paragraph(vec![
            Span::type_name(flat_item.path.clone()).with_target(Some(flat_item.item)),
            Span::plain(" "),
        ])];

        // Add brief documentation if available
        if let Some(docs) = self.docs_to_show(flat_item.item, TruncationLevel::SingleLine) {
            content.extend(docs);
        }

        ListItem::new(content)
    }

    /// Format a module
    #[allow(clippy::mutable_key_type)] // see collect_flat_items
    pub(super) fn format_module(&mut self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut collected = Vec::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(item);
        self.collect_flat_items(&mut collected, &mut visited, None, item);
        self.format_grouped_flat_items(&collected)
    }
}
