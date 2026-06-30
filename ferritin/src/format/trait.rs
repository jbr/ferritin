use rustdoc_types::{AssocItemConstraintKind, GenericArg, GenericArgs, GenericParamDefKind, Impl};

use super::*;
use crate::generate_docsrs_url::generate_docsrs_url;
use crate::styled_string::{DocumentNode, ListItem, Span};

/// Semantic model of a `trait`: signature pieces, its members (methods, assoc
/// types/consts), and the implementors section. Members reuse the shared
/// [`AssocKind`](super::AssocKind).
pub(crate) struct TraitDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) generics: Vec<Span<'a>>,
    /// Supertrait bound spans (the `Eq + PartialOrd` of `trait Ord: Eq + …`),
    /// without the leading `: `.
    pub(crate) supertraits: Vec<Span<'a>>,
    pub(crate) where_clause: Vec<Span<'a>>,
    pub(crate) members: Vec<TraitMember<'a>>,
    /// Types implementing this trait in the current crate, structurally modeled
    /// (capped — see [`Request::model_implementors`]).
    pub(crate) implementors: Vec<ImplementorDoc<'a>>,
    /// Implementors beyond the render cap (the "… and N more").
    pub(crate) implementor_overflow: usize,
}

/// A single implementor of a trait: the implementing type (the impl's `for_`)
/// plus the impl's structured metadata. Like [`TraitImplDoc`](super::TraitImplDoc),
/// the IR is richer than the terminal — it carries the impl's methods, assoc
/// types, `provided_trait_methods`, flags, and docs even though the terminal
/// shows only the implementing type (and assoc types for non-compact impls).
pub(crate) struct ImplementorDoc<'a> {
    /// Implementing type's display name when it's a named path; `None` for
    /// primitives, slices, tuples, etc.
    pub(crate) type_name: Option<&'a str>,
    /// Nav URL for the implementing type, when resolvable.
    pub(crate) type_url: Option<String>,
    /// The implementing type as a span leaf, bounds merged inline
    /// (`BufReader<R: Read>`) — the display leaf.
    pub(crate) for_type: Vec<Span<'a>>,
    pub(crate) assoc_types: Vec<ImplAssocType<'a>>,
    pub(crate) methods: Vec<MethodDoc<'a>>,
    pub(crate) provided_methods: Vec<&'a str>,
    pub(crate) is_unsafe: bool,
    pub(crate) is_synthetic: bool,
    pub(crate) blanket: Option<Vec<Span<'a>>>,
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
    /// Renders in the compact comma list (vs. an expanded list item with assoc
    /// types). Mirrors the old "boring implementor" classification.
    pub(crate) is_compact: bool,
}

/// A single trait member: a method (required or provided), an associated type,
/// or an associated const.
pub(crate) struct TraitMember<'a> {
    pub(crate) name: &'a str,
    pub(crate) kind: AssocKind,
    /// For methods: whether it has a default body (provided vs. required).
    pub(crate) has_default: bool,
    /// Member signature spans — the display leaf.
    pub(crate) signature: Vec<Span<'a>>,
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

impl<'a> Request<'a> {
    /// Resolve a trait item into its semantic [`TraitDoc`] model.
    pub(super) fn model_trait(
        &mut self,
        item: DocRef<'a, Item>,
        trait_data: DocRef<'a, Trait>,
    ) -> TraitDoc<'a> {
        let name = item.name().unwrap_or("<unnamed>");

        let generics = if !trait_data.generics.params.is_empty() {
            self.format_generics(item, &trait_data.item().generics)
        } else {
            vec![]
        };
        let supertraits = if !trait_data.bounds.is_empty() {
            self.format_generic_bounds(item, &trait_data.item().bounds)
        } else {
            vec![]
        };
        let where_clause = if !trait_data.generics.where_predicates.is_empty() {
            self.format_where_clause(item, &trait_data.item().generics.where_predicates)
        } else {
            vec![]
        };

        let mut members = vec![];
        for trait_item in self.ids(item, &trait_data.item().items) {
            let item_name = trait_item.name().unwrap_or("<unnamed>");

            let (kind, has_default, signature) = match &trait_item.item().inner {
                ItemEnum::Function(f) => (
                    AssocKind::Method,
                    f.has_body,
                    self.format_trait_method_signature(trait_item, f, item_name),
                ),
                ItemEnum::AssocType {
                    generics,
                    bounds,
                    type_,
                    default_unstable: _,
                } => (
                    AssocKind::Type,
                    false,
                    self.format_trait_assoc_type_signature(
                        item,
                        generics,
                        bounds,
                        type_.as_ref(),
                        item_name,
                    ),
                ),
                ItemEnum::AssocConst {
                    type_,
                    value,
                    default_unstable: _,
                } => (
                    AssocKind::Const,
                    false,
                    self.format_trait_assoc_const_signature(item, type_, value, item_name),
                ),
                other => (
                    AssocKind::Method,
                    false,
                    vec![Span::comment(format!("// {}: {:?}", item_name, other))],
                ),
            };

            let docs = self.docs_to_show(trait_item, TruncationLevel::SingleLine);
            members.push(TraitMember {
                name: item_name,
                kind,
                has_default,
                signature,
                docs,
            });
        }

        let (implementors, implementor_overflow) = self.model_implementors(item);

        TraitDoc {
            name,
            generics,
            supertraits,
            where_clause,
            members,
            implementors,
            implementor_overflow,
        }
    }

    /// Resolve the types implementing this trait in the current crate into
    /// structured [`ImplementorDoc`]s, plus the count beyond the render cap.
    ///
    /// Sorted by the implementing type's name *before* the cap, because
    /// [`implementors()`](DocRef::implementors) yields `FxHashMap` iteration
    /// order — so without sorting, *which* implementors survive the cap (and
    /// their order) would be nondeterministic w.r.t. the crate's item set.
    fn model_implementors(
        &mut self,
        trait_item: DocRef<'a, Item>,
    ) -> (Vec<ImplementorDoc<'a>>, usize) {
        const MAX_IMPLEMENTORS: usize = 20;

        let mut blocks: Vec<(DocRef<'a, Item>, &'a Impl)> = vec![];
        for impl_block in trait_item.implementors() {
            if let ItemEnum::Impl(impl_item) = impl_block.inner() {
                blocks.push((impl_block, impl_item));
            }
        }

        blocks.sort_by(|a, b| implementor_sort_key(a.1).cmp(implementor_sort_key(b.1)));

        let overflow = blocks.len().saturating_sub(MAX_IMPLEMENTORS);

        let implementors = blocks
            .into_iter()
            .take(MAX_IMPLEMENTORS)
            .map(|(impl_block, impl_item)| self.model_implementor(impl_block, impl_item))
            .collect();

        (implementors, overflow)
    }

    fn model_implementor(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
    ) -> ImplementorDoc<'a> {
        let is_compact = self.is_compact_implementor(impl_block, impl_item);
        let for_type = self.format_implementor_type(impl_block, impl_item);

        let (type_name, type_url) = match &impl_item.for_ {
            Type::ResolvedPath(path) => (
                Some(super::display_path_name(path)),
                self.get_path(impl_block, path.id).map(generate_docsrs_url),
            ),
            _ => (None, None),
        };

        let (methods, assoc_types) = self.model_impl_items(impl_block, impl_item);
        let blanket = impl_item
            .blanket_impl
            .as_ref()
            .map(|ty| self.format_type(impl_block, ty));
        let docs = self.docs_to_show(impl_block, TruncationLevel::SingleLine);

        ImplementorDoc {
            type_name,
            type_url,
            for_type,
            assoc_types,
            methods,
            provided_methods: impl_item
                .provided_trait_methods
                .iter()
                .map(String::as_str)
                .collect(),
            is_unsafe: impl_item.is_unsafe,
            is_synthetic: impl_item.is_synthetic,
            blanket,
            docs,
            is_compact,
        }
    }

    /// An implementor is "compact" if the impl has no type-param bounds and no
    /// concrete assoc types. Method overrides in `items` don't count — only
    /// `AssocType { type_: Some(_) }` items do.
    fn is_compact_implementor(&self, impl_block: DocRef<'_, Item>, impl_item: &Impl) -> bool {
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

/// Lower a [`TraitDoc`] to presentation nodes, reproducing the old `format_trait`
/// output byte-for-byte.
pub(super) fn lower_trait(model: TraitDoc<'_>) -> Vec<DocumentNode<'_>> {
    let TraitDoc {
        name,
        generics,
        supertraits,
        where_clause,
        members,
        implementors,
        implementor_overflow,
    } = model;

    let mut signature_spans = vec![
        Span::keyword("trait"),
        Span::plain(" "),
        Span::type_name(name),
    ];
    signature_spans.extend(generics);
    if !supertraits.is_empty() {
        signature_spans.push(Span::punctuation(":"));
        signature_spans.push(Span::plain(" "));
        signature_spans.extend(supertraits);
    }
    signature_spans.extend(where_clause);
    signature_spans.push(Span::plain(" "));
    signature_spans.push(Span::punctuation("{"));
    signature_spans.push(Span::plain(" ... "));
    signature_spans.push(Span::punctuation("}"));

    let mut nodes = vec![DocumentNode::generated_code(signature_spans)];

    if !members.is_empty() {
        let member_items: Vec<ListItem> = members
            .into_iter()
            .map(|member| {
                let mut sig = member.signature;
                sig.push(Span::plain(" "));
                let mut content = vec![DocumentNode::paragraph(sig)];
                if let Some(docs) = member.docs {
                    content.extend(docs);
                }
                ListItem::new(content)
            })
            .collect();
        nodes.push(DocumentNode::list(member_items));
    }

    nodes.extend(lower_implementors(implementors, implementor_overflow));
    nodes
}

/// Sort key for a trait implementor — the implementing type's display name.
/// `implementors()` order is otherwise `FxHashMap` iteration order.
fn implementor_sort_key(impl_item: &Impl) -> &str {
    match &impl_item.for_ {
        Type::ResolvedPath(path) => super::display_path_name(path),
        Type::Primitive(name) => name,
        _ => "",
    }
}

/// Lower the modeled implementors back to the "Implementors (this crate)"
/// section: compact ones in a comma-separated list, the rest as list items with
/// their assoc-type lines, then the overflow note. Empty when there are none.
fn lower_implementors(
    implementors: Vec<ImplementorDoc<'_>>,
    overflow: usize,
) -> Vec<DocumentNode<'_>> {
    if implementors.is_empty() {
        return vec![];
    }

    let mut compact: Vec<Vec<Span>> = vec![];
    let mut expanded: Vec<(Vec<Span>, Vec<ImplAssocType>)> = vec![];
    for imp in implementors {
        if imp.is_compact {
            compact.push(imp.for_type);
        } else {
            expanded.push((imp.for_type, imp.assoc_types));
        }
    }

    let mut content = vec![];

    if !compact.is_empty() {
        let mut spans = vec![];
        for (i, for_type) in compact.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain(" "));
            }
            spans.extend(for_type);
        }
        content.push(DocumentNode::paragraph(spans));
    }

    if !expanded.is_empty() {
        let items = expanded
            .into_iter()
            .map(|(for_type, assoc_types)| {
                let mut item_nodes = vec![DocumentNode::generated_code(for_type)];
                item_nodes.extend(super::trait_impls::lower_impl_assoc_types(assoc_types));
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
