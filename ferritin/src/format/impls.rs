use ferritin_common::CrateProvenance;
use rustdoc_types::ItemKind;

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};
use semver::VersionReq;
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
                let mut signature_spans = vec![];

                // Add visibility
                match &item.item().visibility {
                    Visibility::Public => {
                        signature_spans.push(Span::keyword("pub"));
                        signature_spans.push(Span::plain(" "));
                    }
                    Visibility::Crate => {
                        signature_spans.push(Span::keyword("pub"));
                        signature_spans.push(Span::punctuation("("));
                        signature_spans.push(Span::keyword("crate"));
                        signature_spans.push(Span::punctuation(")"));
                        signature_spans.push(Span::plain(" "));
                    }
                    Visibility::Restricted { path, .. } => {
                        signature_spans.push(Span::keyword("pub"));
                        signature_spans.push(Span::punctuation("("));
                        signature_spans.push(Span::plain(path));
                        signature_spans.push(Span::punctuation(")"));
                        signature_spans.push(Span::plain(" "));
                    }
                    Visibility::Default => {}
                }

                let name = item.name().unwrap_or("<unnamed>");
                let kind = item.kind();

                // For functions, show the signature inline
                if let ItemEnum::Function(inner) = &item.item().inner {
                    signature_spans.extend(self.format_function_signature(*item, name, inner));
                } else {
                    // For other items, show kind + name
                    let kind_str = match kind {
                        ItemKind::AssocConst => "const",
                        ItemKind::AssocType => "type",
                        _ => "",
                    };

                    if !kind_str.is_empty() {
                        signature_spans.push(Span::keyword(kind_str));
                        signature_spans.push(Span::plain(" "));
                    }

                    signature_spans.push(Span::plain(name));
                }

                let mut item_nodes = vec![DocumentNode::generated_code(signature_spans)];

                // Add brief doc preview
                if let Some(docs) = self.docs_to_show(*item, TruncationLevel::SingleLine) {
                    item_nodes.extend(docs);
                }

                ListItem::new(item_nodes)
            })
            .collect();

        vec![DocumentNode::section(
            vec![Span::plain(title)],
            vec![DocumentNode::list(list_items)],
        )]
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

        // Build trait implementation content
        let mut trait_content = vec![];

        // Add crate-local and external traits (most relevant)
        let mut primary_traits = Vec::new();
        primary_traits.extend(crate_local);
        primary_traits.extend(external);

        if !primary_traits.is_empty() {
            let mut trait_spans = vec![Span::plain("Trait Implementations: ")];
            for t in primary_traits {
                trait_spans.push(Span::plain(t.name).with_path(t.full_path));
                trait_spans.push(Span::plain(" "));
            }
            trait_content.push(DocumentNode::paragraph(trait_spans));
        }

        // Add std traits separately
        if !std_traits.is_empty() {
            let mut trait_spans = vec![Span::plain("std traits: ")];
            for t in std_traits {
                trait_spans.push(Span::plain(t.name).with_path(t.full_path));
                trait_spans.push(Span::plain(" "));
            }
            trait_content.push(DocumentNode::paragraph(trait_spans));
        }

        // Wrap in a section if we have any trait implementations
        if !trait_content.is_empty() {
            vec![DocumentNode::section(
                vec![Span::plain("Trait Implementations")],
                trait_content,
            )]
        } else {
            vec![]
        }
    }

    fn categorize_trait(&self, full_path: String, rendered_path: String) -> TraitImpl {
        // Check by explicit crate prefix (like std::fmt::Display)
        let crate_prefix = full_path.split("::").next().unwrap_or("");

        // Use Navigator's lookup to determine provenance
        if !crate_prefix.is_empty()
            && let Some(lookup_result) = self.lookup_crate(crate_prefix, &VersionReq::STAR)
        {
            let category = match lookup_result.provenance() {
                CrateProvenance::Std => TraitCategory::Std,
                CrateProvenance::Workspace => TraitCategory::CrateLocal,
                _ => TraitCategory::External,
            };

            return TraitImpl {
                category,
                name: rendered_path.to_string(),
                full_path,
            };
        }

        TraitImpl {
            category: TraitCategory::External,
            name: full_path.to_string(),
            full_path,
        }
    }
}
