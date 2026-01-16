use rustdoc_types::ItemKind;

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

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
        &self,
        collected: &mut Vec<FlatItem<'a>>,
        path: Option<String>,
        item: DocRef<'a, Item>,
    ) {
        for child in item.child_items() {
            if let Some(item_name) = child.name() {
                let path = path.as_deref().map_or_else(
                    || item_name.to_string(),
                    |path| format!("{path}::{item_name}"),
                );

                collected.push(FlatItem {
                    path: path.clone(),
                    item: child,
                });

                if self.format_context().is_recursive() {
                    self.collect_flat_items(collected, Some(path), child);
                }
            }
        }
    }

    /// Format collected flat items with grouping by type
    fn format_grouped_flat_items<'a>(&self, items: &[FlatItem<'a>]) -> Vec<DocumentNode<'a>> {
        if items.is_empty() {
            return vec![
                DocumentNode::Span(Span::plain("\n")),
                DocumentNode::Span(Span::plain("No items match the current filters.")),
                DocumentNode::Span(Span::plain("\n")),
            ];
        }

        // Group items by filter type
        let mut groups: HashMap<ItemKind, Vec<&FlatItem>> = HashMap::new();
        for flat_item in items {
            let kind = flat_item.item.kind();
            groups.entry(kind).or_default().push(flat_item);
        }

        let mut doc_nodes = vec![];

        for (kind, group_name) in GROUP_ORDER {
            if let Some(mut group_items) = groups.remove(kind)
                && !group_items.is_empty()
            {
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
        }

        for (kind, mut group_items) in groups {
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
    fn format_flat_item<'a>(&self, flat_item: &FlatItem<'a>) -> ListItem<'a> {
        let mut nodes = vec![];

        // Add brief documentation if available
        if let Some(docs) = self.docs_to_show(flat_item.item, true) {
            nodes.push(DocumentNode::Span(Span::plain("\n")));
            // TODO: Re-add indentation for docs
            nodes.extend(docs);
        }

        ListItem::labeled(vec![Span::type_name(flat_item.path.clone())], nodes)
    }

    /// Format a module
    pub(super) fn format_module<'a>(&self, item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        let mut collected = Vec::new();
        self.collect_flat_items(&mut collected, None, item);
        self.format_grouped_flat_items(&collected)
    }
}
