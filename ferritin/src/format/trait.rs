use super::*;
use crate::{
    docsrs_url::generate_docsrs_url,
    styled_string::{DocumentNode, ListItem, Span},
};
use rustdoc_types::{AssocItemConstraintKind, GenericArg, GenericArgs, GenericParamDefKind, Impl};

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
    /// Every type implementing this trait in the current crate, structurally
    /// modeled and sorted by name. Uncapped: the cap is a *terminal* concern
    /// applied in [`lower_implementors`], so the API can offer the whole list.
    pub(crate) implementors: Vec<ImplementorDoc<'a>>,
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

        let generics = self.format_generics(item, &trait_data.item().generics);
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

        let implementors = self.model_implementors(item);

        TraitDoc {
            name,
            generics,
            supertraits,
            where_clause,
            members,
            implementors,
        }
    }

    /// Resolve every type implementing this trait in the current crate into
    /// structured [`ImplementorDoc`]s.
    ///
    /// Sorted by the implementing type's *rendered* form, because
    /// [`implementors()`](DocRef::implementors) yields `FxHashMap` iteration
    /// order — so without sorting, the order (and, once the terminal caps the
    /// list, *which* implementors survive) would be nondeterministic w.r.t. the
    /// crate's item set. The rendered text is the key (rather than the type's
    /// name) so non-path implementors — tuples, arrays, bare generics — get a
    /// deterministic order too instead of all sorting as equal.
    fn model_implementors(&mut self, trait_item: DocRef<'a, Item>) -> Vec<ImplementorDoc<'a>> {
        let mut docs: Vec<ImplementorDoc<'a>> = vec![];
        for impl_block in trait_item.implementors() {
            if let ItemEnum::Impl(impl_item) = impl_block.inner() {
                docs.push(self.model_implementor(impl_block, impl_item));
            }
        }

        docs.sort_by_cached_key(|doc| {
            doc.for_type
                .iter()
                .map(|span| span.text.as_ref())
                .collect::<String>()
        });

        docs
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

    /// Render the implementing type with bounds merged inline wherever a bare
    /// generic param appears in it: `BufReader<R: Read>`, `(A: Handler, B:
    /// Handler)`, `[H: Handler; const L: usize]`, `Fun: Fn(Conn) -> Fut`.
    ///
    /// Bounds that cannot be attributed to such a position — a bounded param
    /// nested inside another type's args, a predicate on a non-param type, an
    /// HRTB predicate, a second param that never appears in the type (the `Fut`
    /// of the `Fn` shape) — trail in a `where` clause, so no bound is ever
    /// silently dropped.
    fn format_implementor_type(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
    ) -> Vec<Span<'a>> {
        let mut bounds = ImplementorBounds::collect(&impl_item.generics);
        let mut spans = self.format_implementor_component(impl_block, &impl_item.for_, &mut bounds);
        spans.extend(self.implementor_where_clause(impl_block, bounds));
        spans
    }

    /// Render one structural component of the implementing type, merging a bare
    /// generic param's bounds inline when the position allows it: the whole
    /// type, a tuple element, an array/slice element, or a path's generic arg.
    ///
    /// References deliberately do *not* merge (`&'a T: Reactor` would be
    /// ambiguous about what carries the bound), and neither does anything else
    /// unrecognized — those params keep their bounds for the where clause.
    fn format_implementor_component(
        &mut self,
        impl_block: DocRef<'a, Item>,
        type_: &'a Type,
        bounds: &mut ImplementorBounds<'a>,
    ) -> Vec<Span<'a>> {
        match type_ {
            Type::Generic(name) => {
                let mut spans = vec![Span::generic(name)];
                let taken = bounds.take(name);
                for (i, bound) in taken.iter().enumerate() {
                    if i == 0 {
                        spans.push(Span::punctuation(":"));
                        spans.push(Span::plain(" "));
                    } else {
                        spans.push(Span::plain(" + "));
                    }
                    spans.extend(self.format_generic_bound(impl_block, bound));
                }
                spans
            }
            Type::Tuple(types) => {
                let mut spans = vec![Span::punctuation("(")];
                for (i, elem) in types.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::punctuation(","));
                        spans.push(Span::plain(" "));
                    }
                    spans.extend(self.format_implementor_component(impl_block, elem, bounds));
                }
                spans.push(Span::punctuation(")"));
                spans
            }
            Type::Array { type_, len } => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_implementor_component(impl_block, type_, bounds));
                spans.push(Span::punctuation(";"));
                spans.push(Span::plain(" "));
                spans.extend(self.format_implementor_array_len(impl_block, len, bounds));
                spans.push(Span::punctuation("]"));
                spans
            }
            Type::Slice(type_) => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_implementor_component(impl_block, type_, bounds));
                spans.push(Span::punctuation("]"));
                spans
            }
            Type::ResolvedPath(path) => self.format_implementor_path(impl_block, path, bounds),
            other => self.format_type(impl_block, other),
        }
    }

    /// An array length in an implementing type: when it names one of the impl's
    /// const params, annotate it as its declaration (`const L: usize`) so it
    /// reads as "generic over length" rather than as a named constant.
    fn format_implementor_array_len(
        &mut self,
        impl_block: DocRef<'a, Item>,
        len: &'a str,
        bounds: &ImplementorBounds<'a>,
    ) -> Vec<Span<'a>> {
        match bounds.const_param(len) {
            Some(type_) => {
                let mut spans = vec![
                    Span::keyword("const"),
                    Span::plain(" "),
                    Span::plain(len),
                    Span::punctuation(":"),
                    Span::plain(" "),
                ];
                spans.extend(self.format_type(impl_block, type_));
                spans
            }
            None => vec![Span::plain(len)],
        }
    }

    /// Format a `ResolvedPath` component of the implementing type, merging
    /// bounds into generic args that are bare params.
    fn format_implementor_path(
        &mut self,
        impl_block: DocRef<'a, Item>,
        path: &'a rustdoc_types::Path,
        bounds: &mut ImplementorBounds<'a>,
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

                        match arg {
                            GenericArg::Lifetime(lt) => inner.push(Span::lifetime(lt)),
                            GenericArg::Type(ty) => {
                                inner.extend(
                                    self.format_implementor_component(impl_block, ty, bounds),
                                );
                            }
                            GenericArg::Const(c) => {
                                if bounds.const_param(&c.expr).is_some() {
                                    inner.extend(self.format_implementor_array_len(
                                        impl_block, &c.expr, bounds,
                                    ));
                                } else {
                                    inner.push(Span::inline_code(&c.expr));
                                }
                            }
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

    /// The trailing `where` clause for everything [`ImplementorBounds`] still
    /// holds after inline merging: params whose bounds found no inline
    /// position, plus the complex predicates. Mirrors
    /// [`format_where_clause`](Request::format_where_clause)'s shapes — inline
    /// for one entry, one indented entry per line for several — without the
    /// trailing comma/newline, since nothing follows it here.
    fn implementor_where_clause(
        &mut self,
        impl_block: DocRef<'a, Item>,
        bounds: ImplementorBounds<'a>,
    ) -> Vec<Span<'a>> {
        let ImplementorBounds {
            params, complex, ..
        } = bounds;

        let mut entries: Vec<Vec<Span<'a>>> = vec![];
        for (name, param_bounds) in params {
            if param_bounds.is_empty() {
                continue;
            }
            let mut entry = vec![
                Span::generic(name),
                Span::punctuation(":"),
                Span::plain(" "),
            ];
            for (i, bound) in param_bounds.iter().enumerate() {
                if i > 0 {
                    entry.push(Span::plain(" + "));
                }
                entry.extend(self.format_generic_bound(impl_block, bound));
            }
            entries.push(entry);
        }
        for pred in complex {
            entries.push(self.format_where_predicate(impl_block, pred));
        }

        if entries.is_empty() {
            return vec![];
        }

        if entries.len() == 1 {
            let mut spans = vec![
                Span::plain(" "),
                Span::keyword("where"),
                Span::plain(" "),
            ];
            spans.extend(entries.pop().unwrap());
            return spans;
        }

        let mut spans = vec![
            Span::plain("\n"),
            Span::keyword("where"),
            Span::plain("\n    "),
        ];
        for (i, entry) in entries.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain("\n    "));
            }
            spans.extend(entry);
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

        spans.extend(self.format_generics(item, generics));

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
            super::push_body_brace(&mut spans);
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
    super::push_body_brace(&mut signature_spans);
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

    nodes.extend(lower_implementors(implementors));
    nodes
}

/// The bounds an impl places on its generic params, gathered for the
/// implementor rendering: each type param's combined bounds (inline
/// declaration bounds first, then simple where-predicate bounds, preserving
/// order), the const params (for the `const L: usize` annotation), and the
/// predicates too complex to attribute to a single bare param.
///
/// Inline rendering consumes entries via [`take`](Self::take); whatever
/// remains afterwards feeds the trailing where clause, so every bound is
/// rendered exactly once.
struct ImplementorBounds<'a> {
    /// Type params in declaration order, each with its not-yet-rendered bounds.
    params: Vec<(&'a str, Vec<&'a GenericBound>)>,
    /// Const params, name → declared type.
    consts: Vec<(&'a str, &'a Type)>,
    /// Where predicates that aren't a simple bound on a bare type param:
    /// HRTBs, predicates on non-param types, lifetime and equality predicates.
    complex: Vec<&'a WherePredicate>,
}

impl<'a> ImplementorBounds<'a> {
    fn collect(generics: &'a Generics) -> Self {
        let mut params: Vec<(&'a str, Vec<&'a GenericBound>)> = vec![];
        let mut consts = vec![];
        for param in &generics.params {
            match &param.kind {
                GenericParamDefKind::Type { bounds, .. } => {
                    params.push((param.name.as_str(), bounds.iter().collect()));
                }
                GenericParamDefKind::Const { type_, .. } => {
                    consts.push((param.name.as_str(), type_));
                }
                GenericParamDefKind::Lifetime { .. } => {}
            }
        }

        let mut complex = vec![];
        for pred in &generics.where_predicates {
            match pred {
                WherePredicate::BoundPredicate {
                    type_: Type::Generic(name),
                    bounds,
                    generic_params,
                } if generic_params.is_empty()
                    && let Some((_, param_bounds)) =
                        params.iter_mut().find(|(param, _)| param == name) =>
                {
                    param_bounds.extend(bounds);
                }
                other => complex.push(other),
            }
        }

        Self {
            params,
            consts,
            complex,
        }
    }

    /// Take `name`'s pending bounds for inline rendering. Empty when `name`
    /// isn't a type param of this impl or its bounds were already rendered (a
    /// param appearing twice in the type shows its bounds only once).
    fn take(&mut self, name: &str) -> Vec<&'a GenericBound> {
        self.params
            .iter_mut()
            .find(|(param, _)| *param == name)
            .map(|(_, bounds)| std::mem::take(bounds))
            .unwrap_or_default()
    }

    /// The declared type of const param `name`, if `name` is one.
    fn const_param(&self, name: &str) -> Option<&'a Type> {
        self.consts
            .iter()
            .find(|(param, _)| *param == name)
            .map(|(_, type_)| *type_)
    }
}

/// Lower the modeled implementors back to the "Implementors (this crate)"
/// section: compact ones in a comma-separated list, the rest as list items with
/// their assoc-type lines, then the overflow note. Empty when there are none.
///
/// The cap lives here, not in the model: a terminal page has finite room, but an
/// API client can page through the whole list.
fn lower_implementors(implementors: Vec<ImplementorDoc<'_>>) -> Vec<DocumentNode<'_>> {
    const MAX_IMPLEMENTORS: usize = 20;

    if implementors.is_empty() {
        return vec![];
    }

    let overflow = implementors.len().saturating_sub(MAX_IMPLEMENTORS);

    let mut compact: Vec<Vec<Span>> = vec![];
    let mut expanded: Vec<(Vec<Span>, Vec<ImplAssocType>)> = vec![];
    for imp in implementors.into_iter().take(MAX_IMPLEMENTORS) {
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
