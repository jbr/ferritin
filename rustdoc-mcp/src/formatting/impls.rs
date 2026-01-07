use rustdoc_types::ItemKind;

use super::*;
use rustdoc_core::project::RUST_CRATES;
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TraitCategory {
    CrateLocal, // From current crate/workspace (most relevant)
    External,   // Third-party crates
    Std,        // std/core/alloc (least relevant, usually noise)
}

#[derive(Debug)]
struct TraitImpl {
    name: String,
    category: TraitCategory,
}

impl Request {
    /// Add associated methods for a struct or enum
    pub(super) fn format_associated_methods(
        &self,
        item: DocRef<'_, Item>,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();

        let inherent_methods = item.methods().collect::<Vec<_>>();
        // Show inherent methods first
        if !inherent_methods.is_empty() {
            result.push_str(&self.format_item_list(inherent_methods, "Associated Types", context));
        }

        let trait_impls = item.traits().collect::<Vec<_>>();
        // Show trait implementations
        if !trait_impls.is_empty() {
            let formatted_traits = self.format_trait_implementations(&trait_impls, context);
            if !formatted_traits.is_empty() {
                result.write_fmt(format_args!("\n{formatted_traits}"));
            }
        }

        result
    }

    fn format_item_list(
        &self,
        mut items: Vec<DocRef<'_, Item>>,
        title: &str,
        context: &FormatContext,
    ) -> String {
        let mut result = String::new();
        result.write_fmt(format_args!("\n{title}:\n\n"));

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

        for item in items {
            let visibility = match &item.visibility {
                Visibility::Public => "pub ".to_string(),
                Visibility::Default => "".to_string(),
                Visibility::Crate => "pub(crate) ".to_string(),
                Visibility::Restricted { path, .. } => format!("pub({path}) "),
            };

            let name = item.name.as_deref().unwrap_or("<unnamed>");
            let kind = item.kind();

            // For functions, show the signature inline
            if let ItemEnum::Function(inner) = &item.inner {
                let signature = self.format_function_signature(name, inner);
                result.write_fmt(format_args!("• {visibility}{signature}\n"));
            } else {
                result.write_fmt(format_args!("• {visibility}"));

                match kind {
                    ItemKind::AssocConst => result.push_str("const"),
                    ItemKind::AssocType => result.push_str("type"),
                    other => result.write_fmt(format_args!("{other:?}")),
                }

                result.write_fmt(format_args!(" {name}\n"));
            }
            // Add brief doc preview
            if let Some(docs) = self.docs_to_show(item, true, context) {
                result.write_fmt(format_args!("{}", Indent::new(&docs, 4)));
            }

            result.push('\n');
        }

        result
    }

    /// Format trait implementations with explicit category groups
    fn format_trait_implementations(
        &self,
        trait_impls: &[DocRef<'_, Item>],
        context: &FormatContext,
    ) -> String {
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
                let rendered_path = self.format_path(trait_path);
                let impl_ = self.categorize_trait(full_path, rendered_path);
                match impl_.category {
                    TraitCategory::CrateLocal => crate_local.push(impl_.name),
                    TraitCategory::External => external.push(impl_.name),
                    TraitCategory::Std => std_traits.push(impl_.name),
                }
            }
        }

        // Sort each category alphabetically for stable output
        crate_local.sort();
        external.sort();
        std_traits.sort();

        let mut result = String::new();
        let mut sections = Vec::new();

        // Add crate-local and external traits (most relevant)
        let mut primary_traits = Vec::new();
        primary_traits.extend(crate_local);
        primary_traits.extend(external);

        if !primary_traits.is_empty() {
            sections.push(format!(
                "Trait Implementations:\n{}",
                primary_traits.join(", ")
            ));
        }

        // Add std traits separately with truncation
        if !std_traits.is_empty() {
            let displayed_count = if context.verbosity().is_full() {
                std_traits.len()
            } else {
                std_traits.len().min(10)
            };

            let displayed_traits = std_traits
                .iter()
                .take(displayed_count)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");

            let std_section = if displayed_count < std_traits.len() {
                let hidden_count = std_traits.len() - displayed_count;
                format!("std traits: {displayed_traits} [+{hidden_count} more]")
            } else {
                format!("std traits: {displayed_traits}")
            };

            sections.push(std_section);
        }

        if !sections.is_empty() {
            result = sections.join("\n");
            result.push('\n');
        }

        result
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
                };
            }

            // Check if it's from current workspace
            if self.project.is_workspace_package(normalized) {
                return TraitImpl {
                    category: TraitCategory::CrateLocal,
                    name: rendered_path.to_string(),
                };
            }
        }

        TraitImpl {
            category: TraitCategory::External,
            name: full_path.to_string(),
        }
    }
}
