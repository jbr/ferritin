use rustdoc_types::ItemKind;

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};
use ferretin_common::project::RUST_CRATES;
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TraitCategory {
    CrateLocal, // From current crate/workspace (most relevant)
    External,   // Third-party crates
    Std,        // std/core/alloc (least relevant, usually noise)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TraitImpl {
    name: String,
    category: TraitCategory,
    full_path: String,
}

impl Request {
    /// Add associated methods for a struct or enum
    pub(super) fn format_associated_methods<'a>(
        &'a self,
        item: DocRef<'a, Item>,
    ) -> Vec<DocumentNode<'a>> {
        let mut doc_nodes = vec![];

        let inherent_methods = item.methods().collect::<Vec<_>>();
        // Show inherent methods first
        if !inherent_methods.is_empty() {
            doc_nodes.extend(self.format_item_list(inherent_methods, "Associated Types"));
        }

        let trait_impls = item.traits().collect::<Vec<_>>();
        // Show trait implementations
        if !trait_impls.is_empty() {
            doc_nodes.extend(self.format_trait_implementations(&trait_impls));
        }

        doc_nodes
    }

    fn format_item_list<'a>(
        &'a self,
        mut items: Vec<DocRef<'a, Item>>,
        title: &'a str,
    ) -> Vec<DocumentNode<'a>> {
        items.sort_by(|a, b| {
            match (&a.span, &b.span) {
                (Some(span_a), Some(span_b)) => {
                    // Primary sort by filename
                    let filename_cmp = span_a.filename.cmp(&span_b.filename);
                    if filename_cmp != Ordering::Equal {
                        filename_cmp
                    } else {
                        // Secondary sort by start line
                        let line_cmp = span_a.begin.0.cmp(&span_b.begin.0);
                        if line_cmp != Ordering::Equal {
                            line_cmp
                        } else {
                            // Tertiary sort by start column
                            span_a.begin.1.cmp(&span_b.begin.1)
                        }
                    }
                }
                (Some(_), None) => Ordering::Less, // Items with spans come before items without
                (None, Some(_)) => Ordering::Greater, // Items with spans come before items without
                (None, None) => {
                    // Both without spans, sort by name (lexicographical)
                    a.name.cmp(&b.name)
                }
            }
        });

        let list_items: Vec<ListItem> = items
            .iter()
            .map(|item| {
                let mut item_nodes = vec![];

                // Add visibility
                match &item.item().visibility {
                    Visibility::Public => {
                        item_nodes.push(DocumentNode::Span(Span::keyword("pub")));
                        item_nodes.push(DocumentNode::Span(Span::plain(" ")));
                    }
                    Visibility::Crate => {
                        item_nodes.push(DocumentNode::Span(Span::keyword("pub")));
                        item_nodes.push(DocumentNode::Span(Span::punctuation("(")));
                        item_nodes.push(DocumentNode::Span(Span::keyword("crate")));
                        item_nodes.push(DocumentNode::Span(Span::punctuation(")")));
                        item_nodes.push(DocumentNode::Span(Span::plain(" ")));
                    }
                    Visibility::Restricted { path, .. } => {
                        item_nodes.push(DocumentNode::Span(Span::keyword("pub")));
                        item_nodes.push(DocumentNode::Span(Span::punctuation("(")));
                        item_nodes.push(DocumentNode::Span(Span::plain(path)));
                        item_nodes.push(DocumentNode::Span(Span::punctuation(")")));
                        item_nodes.push(DocumentNode::Span(Span::plain(" ")));
                    }
                    Visibility::Default => {}
                }

                let name = item.name().unwrap_or("<unnamed>");
                let kind = item.kind();

                // For functions, show the signature inline
                if let ItemEnum::Function(inner) = &item.item().inner {
                    let signature_spans: Vec<DocumentNode> = self
                        .format_function_signature(*item, name, inner)
                        .into_iter()
                        .map(DocumentNode::Span)
                        .collect();
                    item_nodes.extend(signature_spans);
                } else {
                    // For other items, show kind + name
                    let kind_str = match kind {
                        ItemKind::AssocConst => "const",
                        ItemKind::AssocType => "type",
                        _ => "",
                    };

                    if !kind_str.is_empty() {
                        item_nodes.push(DocumentNode::Span(Span::keyword(kind_str)));
                        item_nodes.push(DocumentNode::Span(Span::plain(" ")));
                    }

                    item_nodes.push(DocumentNode::Span(Span::plain(name)));
                }

                // Add brief doc preview
                if let Some(docs) = self.docs_to_show(*item, TruncationLevel::SingleLine) {
                    item_nodes.push(DocumentNode::Span(Span::plain("\n")));
                    // TODO: Re-add indentation for docs
                    item_nodes.extend(docs);
                }

                item_nodes.push(DocumentNode::Span(Span::plain("\n")));

                ListItem::new(item_nodes)
            })
            .collect();

        vec![
            DocumentNode::Span(Span::plain("\n")),
            DocumentNode::section(
                vec![Span::plain(title)],
                vec![
                    DocumentNode::Span(Span::plain("\n")),
                    DocumentNode::list(list_items),
                ],
            ),
        ]
    }

    /// Format trait implementations with explicit category groups
    fn format_trait_implementations<'a>(
        &self,
        trait_impls: &[DocRef<'a, Item>],
    ) -> Vec<DocumentNode<'a>> {
        let mut crate_local = Vec::new();
        let mut external = Vec::new();
        let mut std_traits = Vec::new();

        // Extract trait implementations
        for impl_block in trait_impls {
            if let ItemEnum::Impl(impl_item) = &impl_block.inner
                && let Some(trait_path) = &impl_item.trait_
            {
                let full_path = impl_block
                    .crate_docs()
                    .path(&trait_path.id)
                    .map(|path| path.to_string())
                    .unwrap_or(trait_path.path.clone());

                // Use the simple path name for display (generics not needed in trait lists)
                let display_name = trait_path.path.clone();

                let impl_ = self.categorize_trait(full_path, display_name);

                match impl_.category {
                    TraitCategory::CrateLocal => crate_local.push(impl_),
                    TraitCategory::External => external.push(impl_),
                    TraitCategory::Std => std_traits.push(impl_),
                }
            }
        }

        // Sort each category alphabetically for stable output
        crate_local.sort();
        external.sort();
        std_traits.sort();

        let mut doc_nodes = vec![];

        // Add crate-local and external traits (most relevant)
        let mut primary_traits = Vec::new();
        primary_traits.extend(crate_local);
        primary_traits.extend(external);

        if !primary_traits.is_empty() {
            doc_nodes.push(DocumentNode::Span(Span::plain("Trait Implementations:\n")));
            for t in primary_traits {
                doc_nodes.push(DocumentNode::Span(
                    Span::plain(t.name).with_path(t.full_path),
                ));
                doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
            }
            doc_nodes.push(DocumentNode::Span(Span::plain("\n")));
        }

        // Add std traits separately with truncation
        if !std_traits.is_empty() {
            doc_nodes.push(DocumentNode::Span(Span::plain("std traits: ")));
            for t in std_traits {
                doc_nodes.push(DocumentNode::Span(
                    Span::plain(t.name).with_path(t.full_path),
                ));
                doc_nodes.push(DocumentNode::Span(Span::plain(" ")));
            }

            doc_nodes.push(DocumentNode::Span(Span::plain("\n")));
        }

        if !doc_nodes.is_empty() {
            doc_nodes.insert(0, DocumentNode::Span(Span::plain("\n")));
        }

        doc_nodes
    }

    fn categorize_trait(&self, full_path: String, rendered_path: String) -> TraitImpl {
        // Check by explicit crate prefix (like std::fmt::Display)
        let crate_prefix = full_path.split("::").next().unwrap_or("");
        // Check if it's from std crates by prefix
        if !crate_prefix.is_empty()
            && let Some(normalized) = self.project.normalize_crate_name(crate_prefix)
        {
            if RUST_CRATES.contains(&normalized) {
                return TraitImpl {
                    category: TraitCategory::Std,
                    name: rendered_path.to_string(),
                    full_path,
                };
            }

            // Check if it's from current workspace
            if self.project.is_workspace_package(normalized) {
                return TraitImpl {
                    category: TraitCategory::CrateLocal,
                    name: rendered_path.to_string(),
                    full_path,
                };
            }
        }

        TraitImpl {
            category: TraitCategory::External,
            name: full_path.to_string(),
            full_path,
        }
    }
}
