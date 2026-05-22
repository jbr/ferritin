use rustdoc_types::{AssocItemConstraintKind, GenericArg, GenericArgs, GenericParamDefKind, Impl};

use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

impl<'a> Request<'a> {
    /// Format a trait
    pub(super) fn format_trait(
        &mut self,
        item: DocRef<'a, Item>,
        trait_data: DocRef<'a, Trait>,
    ) -> Vec<DocumentNode<'a>> {
        let trait_name = item.name().unwrap_or("<unnamed>");

        // Build concise trait signature
        let mut signature_spans = vec![
            Span::keyword("trait"),
            Span::plain(" "),
            Span::type_name(trait_name),
        ];

        if !trait_data.generics.params.is_empty() {
            signature_spans.extend(self.format_generics(item, &trait_data.item().generics));
        }

        if !trait_data.generics.where_predicates.is_empty() {
            signature_spans.extend(
                self.format_where_clause(item, &trait_data.item().generics.where_predicates),
            );
        }

        signature_spans.push(Span::plain(" "));
        signature_spans.push(Span::punctuation("{"));
        signature_spans.push(Span::plain(" ... "));
        signature_spans.push(Span::punctuation("}"));

        let mut nodes: Vec<DocumentNode> = vec![DocumentNode::generated_code(signature_spans)];

        // Build list of trait members
        let mut member_items = vec![];

        for trait_item in self.ids(item, &trait_data.item().items) {
            let item_name = trait_item.name().unwrap_or("<unnamed>");

            let signature_spans = match &trait_item.item().inner {
                ItemEnum::Function(f) => {
                    self.format_trait_method_signature(trait_item, f, item_name)
                }
                ItemEnum::AssocType {
                    generics,
                    bounds,
                    type_,
                } => self.format_trait_assoc_type_signature(
                    item,
                    generics,
                    bounds,
                    type_.as_ref(),
                    item_name,
                ),
                ItemEnum::AssocConst { type_, value } => {
                    self.format_trait_assoc_const_signature(item, type_, value, item_name)
                }
                _ => {
                    // Fallback for unknown item types
                    vec![Span::comment(format!(
                        "// {}: {:?}",
                        item_name, trait_item.inner
                    ))]
                }
            };

            // Prepend signature as a paragraph
            let mut item_content = vec![DocumentNode::paragraph({
                let mut sig = signature_spans;
                sig.push(Span::plain(" "));
                sig
            })];

            // Add docs if available
            if let Some(docs) = self.docs_to_show(trait_item, TruncationLevel::SingleLine) {
                item_content.extend(docs);
            }

            member_items.push(ListItem::new(item_content));
        }

        if !member_items.is_empty() {
            nodes.push(DocumentNode::list(member_items));
        }

        nodes.extend(self.format_implementors(item));

        nodes
    }

    /// Format the "Implementors" section for a trait page.
    ///
    /// Scans the current crate's index for impl blocks that implement this trait.
    /// Boring impls (no bounds, no assoc types) appear in a compact comma-separated list;
    /// everything else appears as a list item. Capped at 20 total.
    fn format_implementors(&mut self, trait_item: DocRef<'a, Item>) -> Vec<DocumentNode<'a>> {
        const MAX_IMPLEMENTORS: usize = 20;

        let mut boring: Vec<(DocRef<'a, Item>, &'a Impl)> = vec![];
        let mut non_boring: Vec<(DocRef<'a, Item>, &'a Impl)> = vec![];
        let mut total = 0;
        let mut overflow = 0;

        for impl_block in trait_item.implementors() {
            if let ItemEnum::Impl(impl_item) = impl_block.inner() {
                total += 1;
                if total > MAX_IMPLEMENTORS {
                    overflow += 1;
                    continue;
                }

                if self.is_boring_implementor(impl_block, impl_item) {
                    boring.push((impl_block, impl_item));
                } else {
                    non_boring.push((impl_block, impl_item));
                }
            }
        }

        if total == 0 {
            return vec![];
        }

        let mut content = vec![];

        if !boring.is_empty() {
            let mut spans = vec![];
            for (i, (impl_block, impl_item)) in boring.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::punctuation(","));
                    spans.push(Span::plain(" "));
                }
                spans.extend(self.format_implementor_type(*impl_block, impl_item));
            }
            content.push(DocumentNode::paragraph(spans));
        }

        if !non_boring.is_empty() {
            let items = non_boring
                .iter()
                .map(|(impl_block, impl_item)| {
                    let mut item_nodes = vec![DocumentNode::generated_code(
                        self.format_implementor_type(*impl_block, impl_item),
                    )];
                    item_nodes.extend(self.format_impl_assoc_types(*impl_block, impl_item));
                    ListItem::new(item_nodes)
                })
                .collect();
            content.push(DocumentNode::list(items));
        }

        if overflow > 0 {
            content.push(DocumentNode::paragraph(vec![Span::plain(format!(
                "… and {overflow} more"
            ))]));
        }

        vec![DocumentNode::section(
            vec![Span::plain("Implementors (this crate)")],
            content,
        )]
    }

    /// An implementor is boring if the impl has no type-param bounds and no concrete assoc types.
    /// Method overrides in `items` don't count — only `AssocType { type_: Some(_) }` items do.
    fn is_boring_implementor(&self, impl_block: DocRef<'_, Item>, impl_item: &Impl) -> bool {
        let no_bounds = impl_item.generics.params.iter().all(|p| {
            !matches!(p.kind, GenericParamDefKind::Type { ref bounds, .. } if !bounds.is_empty())
        }) && impl_item.generics.where_predicates.is_empty();

        let no_assoc_types = !impl_item.items.iter().any(|id| {
            impl_block.get(id).is_some_and(|item| {
                matches!(item.inner(), ItemEnum::AssocType { type_: Some(_), .. })
            })
        });

        no_bounds && no_assoc_types
    }

    /// Render the implementing type with bounds merged inline: `BufReader<R: Read>`.
    ///
    /// For boring implementors this is just the `for_` type. For non-boring ones,
    /// bounds from where predicates are merged into the type's generic args display.
    fn format_implementor_type(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
    ) -> Vec<Span<'a>> {
        // Build a map of simple where-predicate bounds to merge into type args
        let all_simple = impl_item.generics.where_predicates.iter().all(|pred| {
            matches!(pred, WherePredicate::BoundPredicate {
                type_: Type::Generic(_),
                generic_params,
                ..
            } if generic_params.is_empty())
        });

        let extra_bounds: Vec<(&str, &'a [GenericBound])> = if all_simple {
            impl_item
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
                .collect()
        } else {
            vec![]
        };

        match &impl_item.for_ {
            Type::ResolvedPath(path) => {
                self.format_implementor_path(impl_block, impl_item, path, &extra_bounds)
            }
            other => {
                // Primitive, slice, tuple, etc. — just format the type directly
                self.format_type(impl_block, other)
            }
        }
    }

    /// Format a `ResolvedPath` for_ type, merging extra bounds into the generic args.
    fn format_implementor_path(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        path: &'a rustdoc_types::Path,
        extra_bounds: &[(&str, &'a [GenericBound])],
    ) -> Vec<Span<'a>> {
        let name_span = Span::type_name(super::display_path_name(path))
            .with_target(self.get_path(impl_block, path.id));
        let mut inner: Vec<Span<'a>> = vec![];

        if let Some(args) = &path.args {
            match args.as_ref() {
                GenericArgs::AngleBracketed {
                    args: generic_args,
                    constraints,
                } => {
                    for (i, arg) in generic_args.iter().enumerate() {
                        if i > 0 {
                            inner.push(Span::punctuation(","));
                            inner.push(Span::plain(" "));
                        }

                        // If this arg is a generic param that has extra bounds, render inline
                        if let GenericArg::Type(Type::Generic(name)) = arg
                            && let Some(param_def) =
                                impl_item.generics.params.iter().find(|p| p.name == *name)
                        {
                            inner.push(Span::generic(&param_def.name));

                            // Inline bounds from the param definition
                            let mut bounds_started = false;
                            if let GenericParamDefKind::Type { bounds, .. } = &param_def.kind
                                && !bounds.is_empty()
                            {
                                inner.push(Span::punctuation(":"));
                                inner.push(Span::plain(" "));
                                inner.extend(self.format_generic_bounds(impl_block, bounds));
                                bounds_started = true;
                            }

                            // Append extra bounds from where predicates
                            for (pred_name, where_bounds) in extra_bounds {
                                if *pred_name == name.as_str() {
                                    for (j, bound) in where_bounds.iter().enumerate() {
                                        if j == 0 && !bounds_started {
                                            inner.push(Span::punctuation(":"));
                                            inner.push(Span::plain(" "));
                                        } else {
                                            inner.push(Span::plain(" + "));
                                        }
                                        inner.extend(self.format_generic_bound(impl_block, bound));
                                    }
                                    break;
                                }
                            }
                            continue;
                        }

                        // Non-generic arg (concrete type, lifetime, etc.) — render normally
                        match arg {
                            GenericArg::Lifetime(lt) => inner.push(Span::lifetime(lt)),
                            GenericArg::Type(ty) => {
                                inner.extend(self.format_type(impl_block, ty));
                            }
                            GenericArg::Const(c) => inner.push(Span::inline_code(&c.expr)),
                            GenericArg::Infer => inner.push(Span::plain("_")),
                        }
                    }

                    for constraint in constraints {
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
                _ => {
                    let mut spans = vec![name_span];
                    spans.extend(self.format_generic_args(impl_block, args));
                    return spans;
                }
            }
        }

        let mut spans = vec![name_span];
        if !inner.is_empty() {
            spans.push(Span::punctuation("<"));
            spans.extend(inner);
            spans.push(Span::punctuation(">"));
        }
        spans
    }

    pub(super) fn format_trait_assoc_const_signature(
        &mut self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        value: &'a Option<String>,
        const_name: &'a str,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![
            Span::keyword("const"),
            Span::plain(" "),
            Span::plain(const_name),
            Span::punctuation(":"),
            Span::plain(" "),
        ];

        spans.extend(self.format_type(item, type_));

        if let Some(default_val) = value {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("="));
            spans.push(Span::plain(" "));
            spans.push(Span::inline_rust_code(default_val));
        }

        spans.push(Span::punctuation(";"));
        spans
    }

    pub(super) fn format_trait_assoc_type_signature(
        &mut self,
        item: DocRef<'a, Item>,
        generics: &'a Generics,
        bounds: &'a [GenericBound],
        type_: Option<&'a Type>,
        type_name: &'a str,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![
            Span::keyword("type"),
            Span::plain(" "),
            Span::type_name(type_name),
        ];

        if !generics.params.is_empty() {
            spans.extend(self.format_generics(item, generics));
        }

        if !bounds.is_empty() {
            spans.push(Span::punctuation(":"));
            spans.push(Span::plain(" "));
            spans.extend(self.format_generic_bounds(item, bounds));
        }

        if let Some(default_type) = type_ {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("="));
            spans.push(Span::plain(" "));
            spans.extend(self.format_type(item, default_type));
        }

        spans.push(Span::punctuation(";"));
        spans
    }

    fn format_trait_method_signature(
        &mut self,
        item: DocRef<'a, Item>,
        f: &'a Function,
        method_name: &'a str,
    ) -> Vec<Span<'a>> {
        let has_default = f.has_body;

        let mut spans = self.format_function_signature(item, method_name, f);

        if has_default {
            spans.push(Span::plain(" "));
            spans.push(Span::punctuation("{"));
            spans.push(Span::plain(" ... "));
            spans.push(Span::punctuation("}"));
        } else {
            spans.push(Span::punctuation(";"));
        }

        spans
    }
}
