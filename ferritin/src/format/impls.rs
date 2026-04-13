use ferritin_common::CrateProvenance;
use rustdoc_types::{AssocItemConstraintKind, GenericArg, GenericArgs, GenericParamDefKind, Impl};

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};
use semver::VersionReq;

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
        use std::cmp::Ordering;
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
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => a.name.cmp(&b.name),
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
                        rustdoc_types::ItemKind::AssocConst => "const",
                        rustdoc_types::ItemKind::AssocType => "type",
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

    /// Format trait implementations: boring ones as compact lists, non-boring as full signatures
    fn format_trait_implementations<'a>(
        &self,
        trait_impls: &[DocRef<'a, Item>],
    ) -> Vec<DocumentNode<'a>> {
        let mut boring_non_std: Vec<(DocRef<'a, Item>, &'a Impl, &'a Path)> = vec![];
        let mut boring_std: Vec<(DocRef<'a, Item>, &'a Impl, &'a Path)> = vec![];
        let mut non_boring: Vec<(DocRef<'a, Item>, &'a Impl, &'a Path)> = vec![];

        for impl_block in trait_impls {
            // Use inner() (returns &'a ItemEnum) rather than Deref to preserve the 'a lifetime
            if let ItemEnum::Impl(impl_item) = impl_block.inner()
                && let Some(trait_path) = &impl_item.trait_
                && !impl_item.is_negative
            {
                let is_std = self.is_std_trait(*impl_block, trait_path);

                if self.is_boring_impl(impl_item) {
                    if is_std {
                        boring_std.push((*impl_block, impl_item, trait_path));
                    } else {
                        boring_non_std.push((*impl_block, impl_item, trait_path));
                    }
                } else {
                    non_boring.push((*impl_block, impl_item, trait_path));
                }
            }
        }

        let mut content = vec![];

        if !boring_non_std.is_empty() {
            let mut spans = vec![];
            for (i, (impl_block, impl_item, trait_path)) in boring_non_std.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::punctuation(","));
                    spans.push(Span::plain(" "));
                }
                spans.extend(self.format_boring_trait_ref(*impl_block, impl_item, trait_path));
            }
            content.push(DocumentNode::paragraph(spans));
        }

        if !boring_std.is_empty() {
            let mut spans = vec![Span::plain("std: ")];
            for (i, (impl_block, impl_item, trait_path)) in boring_std.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::punctuation(","));
                    spans.push(Span::plain(" "));
                }
                spans.extend(self.format_boring_trait_ref(*impl_block, impl_item, trait_path));
            }
            content.push(DocumentNode::paragraph(spans));
        }

        if !non_boring.is_empty() {
            let list_items = non_boring
                .iter()
                .map(|(impl_block, impl_item, trait_path)| {
                    let mut item_nodes = vec![DocumentNode::generated_code(
                        self.format_impl_signature(*impl_block, impl_item, trait_path),
                    )];
                    item_nodes.extend(self.format_impl_assoc_types(*impl_block, impl_item));
                    ListItem::new(item_nodes)
                })
                .collect();
            content.push(DocumentNode::list(list_items));
        }

        if !content.is_empty() {
            vec![DocumentNode::section(
                vec![Span::plain("Trait Implementations")],
                content,
            )]
        } else {
            vec![]
        }
    }

    /// Whether the trait is from std/core/alloc
    fn is_std_trait(&self, impl_block: DocRef<'_, Item>, trait_path: &Path) -> bool {
        let full_path = impl_block
            .crate_docs()
            .path(&trait_path.id)
            .map(|p| p.to_string())
            .unwrap_or_else(|| trait_path.path.clone());
        let crate_prefix = full_path.split("::").next().unwrap_or("");
        !crate_prefix.is_empty()
            && self
                .lookup_crate(crate_prefix, &VersionReq::STAR)
                .is_some_and(|info| matches!(info.provenance(), CrateProvenance::Std))
    }

    /// An impl is boring if no type params have bounds and there are no where predicates.
    /// Boring impls are shown compactly; non-boring ones get full signatures.
    fn is_boring_impl(&self, impl_item: &Impl) -> bool {
        for param in &impl_item.generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                if !bounds.is_empty() {
                    return false;
                }
            }
        }
        impl_item.generics.where_predicates.is_empty()
    }

    /// Render `TraitName<GenericArgs, AssocType = Value>` for a boring impl.
    /// Merges the trait path's generic args with any associated type assignments from the impl.
    fn format_boring_trait_ref<'a>(
        &self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        trait_path: &'a Path,
    ) -> Vec<Span<'a>> {
        let full_path = impl_block
            .crate_docs()
            .path(&trait_path.id)
            .map(|p| p.to_string())
            .unwrap_or_else(|| trait_path.path.clone());

        // Build inner angle-bracket content from trait args + impl assoc types
        let mut inner: Vec<Span<'a>> = vec![];

        if let Some(args) = &trait_path.args {
            match args.as_ref() {
                GenericArgs::AngleBracketed { args: generic_args, constraints } => {
                    for (i, arg) in generic_args.iter().enumerate() {
                        if i > 0 {
                            inner.push(Span::punctuation(","));
                            inner.push(Span::plain(" "));
                        }
                        match arg {
                            GenericArg::Lifetime(lt) => inner.push(Span::lifetime(lt)),
                            GenericArg::Type(ty) => {
                                inner.extend(self.format_type(impl_block, ty));
                            }
                            GenericArg::Const(c) => inner.push(Span::inline_code(&c.expr)),
                            GenericArg::Infer => inner.push(Span::plain("_")),
                        }
                    }
                    for constraint in constraints.iter() {
                        if !inner.is_empty() {
                            inner.push(Span::punctuation(","));
                            inner.push(Span::plain(" "));
                        }
                        inner.push(Span::plain(&constraint.name));
                        match &constraint.binding {
                            AssocItemConstraintKind::Equality(term) => {
                                inner.push(Span::plain(" "));
                                inner.push(Span::operator("="));
                                inner.push(Span::plain(" "));
                                inner.extend(self.format_term(impl_block, term));
                            }
                            AssocItemConstraintKind::Constraint(bounds) => {
                                inner.push(Span::punctuation(":"));
                                inner.push(Span::plain(" "));
                                inner.extend(self.format_generic_bounds(impl_block, bounds));
                            }
                        }
                    }
                }
                // For fn-trait syntax like Fn(A, B) -> C, fall back to format_generic_args
                _ => {
                    let mut spans =
                        vec![Span::type_name(&trait_path.path).with_path(full_path.clone())];
                    spans.extend(self.format_generic_args(impl_block, args));
                    return spans;
                }
            }
        }

        // Append associated type assignments from the impl's items
        for id in &impl_item.items {
            if let Some(assoc_item) = impl_block.get(id) {
                // Use inner() (returns &'a ItemEnum) to preserve the 'a lifetime
                if let ItemEnum::AssocType {
                    type_: Some(ty), ..
                } = assoc_item.inner()
                {
                    let name = assoc_item.name().unwrap_or("_");
                    if !inner.is_empty() {
                        inner.push(Span::punctuation(","));
                        inner.push(Span::plain(" "));
                    }
                    inner.push(Span::type_name(name));
                    inner.push(Span::plain(" "));
                    inner.push(Span::operator("="));
                    inner.push(Span::plain(" "));
                    inner.extend(self.format_type(impl_block, ty));
                }
            }
        }

        let mut spans = vec![Span::type_name(&trait_path.path).with_path(full_path)];
        if !inner.is_empty() {
            spans.push(Span::punctuation("<"));
            spans.extend(inner);
            spans.push(Span::punctuation(">"));
        }
        spans
    }

    /// Render `impl<T: Bound> Trait<T>` for a non-boring impl.
    ///
    /// If all where predicates are simple type-param bounds (no HRTBs, no qualified paths),
    /// merges them into the inline generic params so the signature fits on one line.
    /// Complex predicates fall back to a where clause.
    fn format_impl_signature<'a>(
        &self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        trait_path: &'a Path,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![Span::keyword("impl")];

        if !impl_item.generics.params.is_empty() {
            // If all where predicates are simple type-param bounds (no HRTBs, no qualified paths),
            // merge them into the inline generic params. Otherwise fall back to a where clause.
            let all_simple = impl_item.generics.where_predicates.iter().all(|pred| {
                matches!(pred, WherePredicate::BoundPredicate {
                    type_: Type::Generic(_),
                    generic_params,
                    ..
                } if generic_params.is_empty())
            });

            if all_simple && !impl_item.generics.where_predicates.is_empty() {
                // Build a lookup from param name to the bounds from the where predicate
                let extra_bounds: Vec<(&str, &'a [GenericBound])> = impl_item
                    .generics
                    .where_predicates
                    .iter()
                    .filter_map(|pred| {
                        if let WherePredicate::BoundPredicate {
                            type_: Type::Generic(name),
                            bounds,
                            ..
                        } = pred
                        {
                            Some((name.as_str(), bounds.as_slice()))
                        } else {
                            None
                        }
                    })
                    .collect();

                spans.push(Span::punctuation("<"));
                for (i, param) in impl_item.generics.params.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::punctuation(","));
                        spans.push(Span::plain(" "));
                    }
                    spans.extend(self.format_generic_param(impl_block, param));

                    // Append matching where-predicate bounds to this param inline
                    if let GenericParamDefKind::Type { bounds: inline_bounds, .. } = &param.kind {
                        for (pred_name, where_bounds) in &extra_bounds {
                            if *pred_name == param.name.as_str() {
                                for (j, bound) in where_bounds.iter().enumerate() {
                                    if j == 0 && inline_bounds.is_empty() {
                                        spans.push(Span::punctuation(":"));
                                        spans.push(Span::plain(" "));
                                    } else {
                                        spans.push(Span::plain(" + "));
                                    }
                                    spans.extend(self.format_generic_bound(impl_block, bound));
                                }
                                break;
                            }
                        }
                    }
                }
                spans.push(Span::punctuation(">"));
            } else {
                // Has complex predicates (HRTBs, qualified paths, etc.) — use where clause
                spans.extend(self.format_generics(impl_block, &impl_item.generics));
                if !impl_item.generics.where_predicates.is_empty() {
                    spans.extend(
                        self.format_where_clause(
                            impl_block,
                            &impl_item.generics.where_predicates,
                        ),
                    );
                }
            }
        }

        spans.push(Span::plain(" "));
        spans.extend(self.format_path(impl_block, trait_path));

        spans
    }

    /// Render associated type assignments from an impl as indented `type Foo = Bar` lines.
    fn format_impl_assoc_types<'a>(
        &self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
    ) -> Vec<DocumentNode<'a>> {
        let mut nodes = vec![];
        for id in &impl_item.items {
            if let Some(assoc_item) = impl_block.get(id) {
                if let ItemEnum::AssocType {
                    type_: Some(ty), ..
                } = assoc_item.inner()
                {
                    let name = assoc_item.name().unwrap_or("_");
                    let mut spans = vec![
                        Span::plain("    "),
                        Span::keyword("type"),
                        Span::plain(" "),
                        Span::type_name(name),
                        Span::plain(" "),
                        Span::operator("="),
                        Span::plain(" "),
                    ];
                    spans.extend(self.format_type(impl_block, ty));
                    nodes.push(DocumentNode::generated_code(spans));
                }
            }
        }
        nodes
    }
}
