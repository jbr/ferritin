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

/// Semantic model of a `module` item: a flat collection of its child items in
/// traversal order. Grouping by kind (the terminal's sections) and a JSON
/// client's own grouping are both *downstream* of this flat list — the model
/// ships structure, not presentation, so grouping stays a consumer concern.
pub(crate) struct ModuleDoc<'a> {
    pub(crate) items: Vec<ModuleItem<'a>>,
}

/// A single child of a module: its qualified path (relative to the module, or
/// fully qualified under `--recursive`), the kind used for grouping, the
/// resolved navigation target, and brief docs.
pub(crate) struct ModuleItem<'a> {
    /// Path as listed: the bare name for a direct child, `a::b::c` when reached
    /// recursively through nested modules.
    pub(crate) path: String,
    /// Item kind, used to group the listing (terminal) and exposed verbatim to
    /// JSON clients (which may group differently).
    pub(crate) kind: ItemKind,
    /// The child item itself — the navigation target. Carried so the lowering
    /// can attach the nav action and the JSON output can derive the `url`.
    pub(crate) target: DocRef<'a, Item>,
    /// Single-line docs for the listing, if the item has any.
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

impl<'a> Request<'a> {
    /// Collect all items in a module hierarchy as flat [`ModuleItem`]s.
    ///
    /// Tracks visited items during recursive descent so cyclic re-export
    /// chains (e.g. a nested module glob-importing its parent) can't send the
    /// traversal into an infinite loop.
    // DocRef hashes by crate name + item id; the interior mutability lives in
    // Navigator's connection pool and doesn't affect identity.
    #[allow(clippy::mutable_key_type)]
    fn collect_module_items(
        &mut self,
        collected: &mut Vec<ModuleItem<'a>>,
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

                // Filter what's *collected*, not what's *descended into*: a
                // `--kind fn` listing should still recurse through modules to
                // reach nested functions, just without listing the modules.
                if self.format_context().should_display(child) {
                    let docs = self.docs_to_show(child, TruncationLevel::SingleLine);
                    collected.push(ModuleItem {
                        path: path.clone(),
                        kind: child.kind(),
                        target: child,
                        docs,
                    });
                }

                if self.format_context().is_recursive() && visited.insert(child) {
                    self.collect_module_items(collected, visited, Some(path), child);
                }
            }
        }
    }

    /// Resolve a module item into its semantic [`ModuleDoc`] model — the
    /// resolution half of the old `format_module`, with the grouping and span
    /// assembly moved to [`lower_module`].
    #[allow(clippy::mutable_key_type)] // see collect_module_items
    pub(super) fn model_module(&mut self, item: DocRef<'a, Item>) -> ModuleDoc<'a> {
        let mut items = Vec::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(item);
        self.collect_module_items(&mut items, &mut visited, None, item);
        ModuleDoc { items }
    }
}

/// Lower a [`ModuleDoc`] to presentation [`DocumentNode`]s, reproducing the old
/// `format_module` output: items grouped by kind under [`GROUP_ORDER`]
/// headings, sorted by path within each group, with unrecognized kinds appended
/// alphabetically. insta snapshots are the guardrail for byte-identity.
pub(super) fn lower_module(model: ModuleDoc<'_>) -> Vec<DocumentNode<'_>> {
    let ModuleDoc { items } = model;

    if items.is_empty() {
        return vec![DocumentNode::paragraph(vec![Span::plain(
            "No items match the current filters.",
        )])];
    }

    // Group items by kind.
    let mut groups: HashMap<ItemKind, Vec<ModuleItem>> = HashMap::new();
    for item in items {
        groups.entry(item.kind).or_default().push(item);
    }

    let mut doc_nodes = vec![];

    for (group_name, kinds) in GROUP_ORDER {
        let mut group_items: Vec<ModuleItem> = kinds
            .iter()
            .filter_map(|kind| groups.remove(kind))
            .flatten()
            .collect();

        if group_items.is_empty() {
            continue;
        }

        group_items.sort_by(|a, b| a.path.cmp(&b.path));

        let list_items: Vec<ListItem> = group_items.into_iter().map(lower_module_item).collect();

        doc_nodes.push(DocumentNode::section(
            vec![Span::plain(*group_name)],
            vec![DocumentNode::list(list_items)],
        ));
    }

    // Sort remaining (unrecognized) kinds alphabetically by their debug name
    // so output is stable across runs — HashMap iteration order is not.
    let mut remaining: Vec<_> = groups.into_iter().collect();
    remaining.sort_by_key(|(kind, _)| format!("{kind:?}"));

    for (kind, mut group_items) in remaining {
        group_items.sort_by(|a, b| a.path.cmp(&b.path));

        let list_items: Vec<ListItem> = group_items.into_iter().map(lower_module_item).collect();

        doc_nodes.push(DocumentNode::section(
            vec![Span::plain(format!("{kind:?}"))],
            vec![DocumentNode::list(list_items)],
        ));
    }

    doc_nodes
}

/// Lower a single [`ModuleItem`] to its list entry: a nav-targeted path
/// paragraph followed by any brief docs.
fn lower_module_item(item: ModuleItem<'_>) -> ListItem<'_> {
    let ModuleItem {
        path, target, docs, ..
    } = item;

    let mut content = vec![DocumentNode::paragraph(vec![
        Span::type_name(path).with_target(Some(target)),
        Span::plain(" "),
    ])];

    if let Some(docs) = docs {
        content.extend(docs);
    }

    ListItem::new(content)
}
