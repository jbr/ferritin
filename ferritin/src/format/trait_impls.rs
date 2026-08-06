use super::*;
use crate::{
    docsrs_url::generate_docsrs_url,
    styled_string::{DocumentNode, ListItem, Span},
};
use ferritin_common::CrateProvenance;
use rustdoc_types::{AssocItemConstraintKind, GenericArg, GenericArgs, GenericParamDefKind, Impl};
use semver::VersionReq;

/// Semantic model of a single trait implementation on a type. The IR captures
/// the full impl — including data the terminal currently drops (the impl's
/// methods, `provided_trait_methods`, negative/unsafe/synthetic/blanket flags,
/// and impl docs) — so non-terminal consumers aren't limited by the sparse
/// terminal rendering.
///
/// The two fiddly span assemblies (the compact merged trait-ref and the full
/// `impl<…> Trait` header) are memoized into [`ImplLeaf`] at model time — the
/// same "presentation leaf alongside structured fields" shape as
/// [`MethodDoc`](super::MethodDoc) — so lowering only buckets and selects, while
/// JSON serializes the structural projection.
pub(crate) struct TraitImplDoc<'a> {
    /// Display name of the implemented trait (e.g. `From`).
    pub(crate) trait_name: &'a str,
    /// Resolved nav URL for the trait, the hypermedia pointer for a JSON client.
    pub(crate) trait_url: Option<String>,
    /// The trait's generic arguments (`<T>` in `From<T>`), as resolved spans.
    pub(crate) trait_args: Vec<Span<'a>>,
    /// Associated-type bindings declared in the impl (`type Foo = Bar`).
    pub(crate) assoc_types: Vec<ImplAssocType<'a>>,
    /// Methods / assoc consts the impl provides or overrides, structurally
    /// modeled. **Dropped by the terminal rendering**, carried for richer
    /// consumers.
    pub(crate) methods: Vec<MethodDoc<'a>>,
    /// Names of trait-default methods inherited (not overridden) by this impl.
    pub(crate) provided_methods: Vec<&'a str>,
    /// `impl !Trait for …`.
    pub(crate) is_negative: bool,
    /// `unsafe impl …`.
    pub(crate) is_unsafe: bool,
    /// Compiler-implied impl (autotraits like `Send`/`Sync`).
    pub(crate) is_synthetic: bool,
    /// Whether the trait is from std/core/alloc.
    pub(crate) is_std: bool,
    /// Blanket source type, when this impl came from a blanket impl.
    pub(crate) blanket: Option<Vec<Span<'a>>>,
    /// The impl block's own (brief) docs, if any. Dropped by the terminal today.
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
    /// The memoized terminal rendering — built at model time so byte-identity of
    /// the intricate span assembly is preserved; ignored by JSON.
    pub(crate) leaf: ImplLeaf<'a>,
}

/// An associated-type binding inside an impl (`type Foo = Bar`).
pub(crate) struct ImplAssocType<'a> {
    pub(crate) name: &'a str,
    pub(crate) type_spans: Vec<Span<'a>>,
}

/// The terminal rendering of a trait impl: either a compact one-line trait
/// reference, or a full `impl<…> Trait` header (with the assoc-type lines lowered
/// separately from the structural [`TraitImplDoc::assoc_types`]).
pub(crate) enum ImplLeaf<'a> {
    Compact(Vec<Span<'a>>),
    Full(Vec<Span<'a>>),
}

impl<'a> Request<'a> {
    /// Resolve an item's trait implementations into structured [`TraitImplDoc`]s.
    ///
    /// Sorted by trait name (then rendered args), because the raw
    /// [`traits()`](DocRef::traits) order is `FxHashMap` iteration order — stable
    /// within a build but nondeterministic w.r.t. the crate's item set, so any
    /// added item would reshuffle every type's impl list. Sorting makes the
    /// output deterministic and meaningfully alphabetical; lowering then does the
    /// compact/std bucketing in this order.
    pub(super) fn model_trait_impls(&mut self, item: DocRef<'a, Item>) -> Vec<TraitImplDoc<'a>> {
        let mut impls = vec![];
        for impl_block in item.traits().collect::<Vec<_>>() {
            // Use inner() (returns &'a ItemEnum) rather than Deref to preserve 'a.
            if let ItemEnum::Impl(impl_item) = impl_block.inner()
                && let Some(trait_path) = &impl_item.trait_
            {
                impls.push(self.model_trait_impl(impl_block, impl_item, trait_path));
            }
        }

        impls.sort_by(|a, b| {
            a.trait_name
                .cmp(b.trait_name)
                .then_with(|| trait_args_text(a).cmp(&trait_args_text(b)))
                .then_with(|| a.is_negative.cmp(&b.is_negative))
        });

        impls
    }

    fn model_trait_impl(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        trait_path: &'a Path,
    ) -> TraitImplDoc<'a> {
        let trait_name = super::display_path_name(trait_path);
        let trait_url = self
            .get_path(impl_block, trait_path.id)
            .map(generate_docsrs_url);
        let mut trait_args = self.trait_arg_spans(impl_block, trait_path);
        let is_std = self.is_std_trait(impl_block, trait_path);

        let (methods, mut assoc_types) = self.model_impl_items(impl_block, impl_item);

        let blanket = impl_item
            .blanket_impl
            .as_ref()
            .map(|ty| self.format_type(impl_block, ty));
        let docs = self.docs_to_show(impl_block, TruncationLevel::SingleLine);

        // On this type's page the blanket impl's source param *is* Self:
        // `impl<T> From<T> for T` reaching this page means `T` = this type, so
        // rendering the param as `Self` (and dropping it from the param list)
        // is what stops `From<T>` reading as "from any T". Only claimed when
        // the param is a bare generic and every predicate is simple enough for
        // the header's inline merge — otherwise the param would still appear
        // in a `format_generics` param list, which cannot skip it.
        let blanket_self: Option<&'a str> = match &impl_item.blanket_impl {
            Some(Type::Generic(name)) if simple_impl_preds(impl_item) => Some(name.as_str()),
            _ => None,
        };

        let mut leaf = if is_compact_impl(impl_item) {
            ImplLeaf::Compact(self.build_compact_ref(
                impl_block,
                impl_item,
                trait_path,
                impl_item.is_negative,
            ))
        } else {
            ImplLeaf::Full(self.build_impl_header(
                impl_block,
                impl_item,
                trait_path,
                impl_item.is_negative,
                blanket_self,
            ))
        };

        if let Some(name) = blanket_self {
            let (ImplLeaf::Compact(spans) | ImplLeaf::Full(spans)) = &mut leaf;
            substitute_self(spans, name);
            substitute_self(&mut trait_args, name);
            for assoc in &mut assoc_types {
                substitute_self(&mut assoc.type_spans, name);
            }
        }

        TraitImplDoc {
            trait_name,
            trait_url,
            trait_args,
            assoc_types,
            methods,
            provided_methods: impl_item
                .provided_trait_methods
                .iter()
                .map(String::as_str)
                .collect(),
            is_negative: impl_item.is_negative,
            is_unsafe: impl_item.is_unsafe,
            is_synthetic: impl_item.is_synthetic,
            is_std,
            blanket,
            docs,
            leaf,
        }
    }

    /// The trait's generic-argument spans (`<T>` of `From<T>`), for the JSON
    /// projection. Empty when the trait has no arguments.
    fn trait_arg_spans(
        &mut self,
        impl_block: DocRef<'a, Item>,
        trait_path: &'a Path,
    ) -> Vec<Span<'a>> {
        match &trait_path.args {
            Some(args) => self.format_generic_args(impl_block, args),
            None => vec![],
        }
    }

    /// Partition an impl's associated items into structurally-modeled methods
    /// (functions / assoc consts) and associated-type bindings (`type X = Y`).
    /// Shared by trait implementations and the trait implementors section.
    ///
    /// Methods of a **blanket** impl are skipped: their `Item` is shared across
    /// every implementor (the same id appears in each synthesized impl block), so
    /// the method belongs to the blanket definition rather than this type, and
    /// its self-link resolves ambiguously (and nondeterministically) — see
    /// `generate_url_for_associated_item`. Assoc-type bindings are kept (they
    /// resolve to concrete types here).
    pub(super) fn model_impl_items(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
    ) -> (Vec<MethodDoc<'a>>, Vec<ImplAssocType<'a>>) {
        let model_methods = impl_item.blanket_impl.is_none();
        let mut methods = vec![];
        let mut assoc_types = vec![];
        for id in &impl_item.items {
            if let Some(assoc_item) = impl_block.get(id) {
                match assoc_item.inner() {
                    ItemEnum::AssocType {
                        type_: Some(ty), ..
                    } => assoc_types.push(ImplAssocType {
                        name: assoc_item.name().unwrap_or("_"),
                        type_spans: self.format_type(impl_block, ty),
                    }),
                    ItemEnum::AssocType { type_: None, .. } => {}
                    // An impl is an *edge* between a type and a trait, and only
                    // what is specific to that edge belongs on it. A method's
                    // signature is dictated by the trait — the node — so copying
                    // it onto every edge is denormalization, not documentation.
                    // Custom prose on an impl method is edge-specific (it is the
                    // one thing that exists nowhere else), so documented methods
                    // are kept and the rest dropped. Assoc-type bindings above
                    // are kept for the same reason: `type Item = u8` is a fact
                    // about this edge alone.
                    _ if model_methods => {
                        let method = self.model_method(assoc_item);
                        if method.docs.is_some() {
                            methods.push(method);
                        }
                    }
                    _ => {}
                }
            }
        }
        (methods, assoc_types)
    }

    /// Whether the trait is from std/core/alloc.
    fn is_std_trait(&self, impl_block: DocRef<'_, Item>, trait_path: &Path) -> bool {
        let full_path = impl_block
            .crate_docs()
            .path(&trait_path.id)
            .map(|p| p.to_string())
            .unwrap_or_else(|| trait_path.path.clone());
        let crate_prefix = full_path.split("::").next().unwrap_or("");
        !crate_prefix.is_empty()
            && self
                .navigator()
                .lookup_crate(crate_prefix, &VersionReq::STAR)
                .is_some_and(|info| matches!(info.provenance(), CrateProvenance::Std))
    }

    /// Build `[!]TraitName<GenericArgs, AssocType = Value>` for a compact impl,
    /// merging the trait path's generic args with the impl's associated-type
    /// assignments.
    fn build_compact_ref(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        trait_path: &'a Path,
        negative: bool,
    ) -> Vec<Span<'a>> {
        let full_path = impl_block
            .crate_docs()
            .path(&trait_path.id)
            .map(|p| p.to_string())
            .unwrap_or_else(|| trait_path.path.clone());

        let mut result: Vec<Span<'a>> = vec![];
        if negative {
            result.push(Span::operator("!"));
        }

        // Build inner angle-bracket content from trait args + impl assoc types.
        let mut inner: Vec<Span<'a>> = vec![];

        if let Some(args) = &trait_path.args {
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
                // For fn-trait syntax like Fn(A, B) -> C, fall back to format_generic_args.
                _ => {
                    result.push(
                        Span::type_name(super::display_path_name(trait_path))
                            .with_path(full_path.clone()),
                    );
                    result.extend(self.format_generic_args(impl_block, args));
                    return result;
                }
            }
        }

        // Append associated type assignments from the impl's items.
        for id in &impl_item.items {
            if let Some(assoc_item) = impl_block.get(id)
                && let ItemEnum::AssocType {
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

        result.push(Span::type_name(super::display_path_name(trait_path)).with_path(full_path));
        if !inner.is_empty() {
            result.push(Span::punctuation("<"));
            result.extend(inner);
            result.push(Span::punctuation(">"));
        }
        result
    }

    /// Build `impl<T: Bound> [!]Trait<T>` for a full (non-compact) impl.
    ///
    /// If all where predicates are simple type-param bounds (no HRTBs, no
    /// qualified paths), merges them into the inline generic params so the
    /// signature fits on one line. Complex predicates fall back to a where clause.
    ///
    /// When `blanket_self` names the impl's Self param (see
    /// [`model_trait_impl`](Request::model_trait_impl)), that param is dropped
    /// from the list and its bounds — the conditions this page's type must meet
    /// for the blanket to apply — trail as `where Self: …`.
    fn build_impl_header(
        &mut self,
        impl_block: DocRef<'a, Item>,
        impl_item: &'a Impl,
        trait_path: &'a Path,
        negative: bool,
        blanket_self: Option<&str>,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![Span::keyword("impl")];

        if simple_impl_preds(impl_item) {
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

            let mut inner: Vec<Span<'a>> = vec![];
            let mut self_bounds: Vec<Span<'a>> = vec![];
            for param in &impl_item.generics.params {
                let inline_bounds = match &param.kind {
                    GenericParamDefKind::Type { bounds, .. } => bounds.as_slice(),
                    _ => &[],
                };

                if Some(param.name.as_str()) == blanket_self {
                    let where_bounds = extra_bounds
                        .iter()
                        .filter(|(name, _)| *name == param.name.as_str())
                        .flat_map(|(_, bounds)| bounds.iter());
                    for (i, bound) in inline_bounds.iter().chain(where_bounds).enumerate() {
                        if i > 0 {
                            self_bounds.push(Span::plain(" + "));
                        }
                        self_bounds.extend(self.format_generic_bound(impl_block, bound));
                    }
                    continue;
                }

                if !inner.is_empty() {
                    inner.push(Span::punctuation(","));
                    inner.push(Span::plain(" "));
                }
                inner.extend(self.format_generic_param(impl_block, param));

                let mut bounds_started = !inline_bounds.is_empty();
                for (pred_name, where_bounds) in &extra_bounds {
                    if *pred_name == param.name.as_str() {
                        for bound in where_bounds.iter() {
                            if bounds_started {
                                inner.push(Span::plain(" + "));
                            } else {
                                inner.push(Span::punctuation(":"));
                                inner.push(Span::plain(" "));
                                bounds_started = true;
                            }
                            inner.extend(self.format_generic_bound(impl_block, bound));
                        }
                    }
                }
            }

            if !inner.is_empty() {
                spans.push(Span::punctuation("<"));
                spans.extend(inner);
                spans.push(Span::punctuation(">"));
            }

            spans.push(Span::plain(" "));
            if negative {
                spans.push(Span::operator("!"));
            }
            spans.extend(self.format_path(impl_block, trait_path));

            if !self_bounds.is_empty() {
                spans.push(Span::plain(" "));
                spans.push(Span::keyword("where"));
                spans.push(Span::plain(" "));
                spans.push(Span::generic("Self"));
                spans.push(Span::punctuation(":"));
                spans.push(Span::plain(" "));
                spans.extend(self_bounds);
            }
        } else {
            // Complex predicates: generics + trait on the header line, then
            // where clause below.
            spans.extend(self.format_generics(impl_block, &impl_item.generics));
            spans.push(Span::plain(" "));
            if negative {
                spans.push(Span::operator("!"));
            }
            spans.extend(self.format_path(impl_block, trait_path));
            if !impl_item.generics.where_predicates.is_empty() {
                spans.extend(
                    self.format_where_clause(impl_block, &impl_item.generics.where_predicates),
                );
            }
        }

        spans
    }
}

/// Whether every where predicate is a simple bound on one of the impl's own
/// bare type params — the shape [`build_impl_header`](Request::build_impl_header)
/// can merge inline. Anything else (HRTBs, predicates on non-param types,
/// lifetime/equality predicates) needs a real where clause.
fn simple_impl_preds(impl_item: &Impl) -> bool {
    impl_item.generics.where_predicates.iter().all(|pred| {
        matches!(pred, WherePredicate::BoundPredicate {
            type_: Type::Generic(name),
            generic_params,
            ..
        } if generic_params.is_empty()
            && impl_item.generics.params.iter().any(|param| param.name == *name))
    })
}

/// Rewrite occurrences of the blanket impl's Self param to `Self` in rendered
/// spans. Text-matching on [`SpanStyle::Generic`] spans is sound here: within
/// one impl's rendering, every generic span bearing the param's name denotes
/// that param.
fn substitute_self(spans: &mut [Span<'_>], param: &str) {
    for span in spans {
        if span.style == crate::styled_string::SpanStyle::Generic && span.text == param {
            span.text = "Self".into();
        }
    }
}

/// Concatenated text of a trait's rendered generic args, the secondary sort key
/// (disambiguates multiple impls of the same trait, e.g. `From<A>` / `From<B>`).
fn trait_args_text(impl_doc: &TraitImplDoc<'_>) -> String {
    impl_doc
        .trait_args
        .iter()
        .map(|span| span.text.as_ref())
        .collect()
}

/// An impl renders compactly when no type params carry bounds and there are no
/// where predicates — a simple `Trait for X` with nothing to expand.
fn is_compact_impl(impl_item: &Impl) -> bool {
    for param in &impl_item.generics.params {
        if let GenericParamDefKind::Type { bounds, .. } = &param.kind
            && !bounds.is_empty()
        {
            return false;
        }
    }
    impl_item.generics.where_predicates.is_empty()
}

/// Lower structural trait impls back to the "Trait Implementations" section,
/// reproducing the original output: compact non-std refs on one line, compact
/// std refs on a `std: …` line, then full signatures (each with its assoc-type
/// lines). Empty when there are no impls.
pub(super) fn lower_trait_impls(impls: Vec<TraitImplDoc<'_>>) -> Vec<DocumentNode<'_>> {
    let mut compact_other: Vec<Vec<Span>> = vec![];
    let mut compact_std: Vec<Vec<Span>> = vec![];
    let mut expanded: Vec<(Vec<Span>, Vec<ImplAssocType>)> = vec![];

    for impl_doc in impls {
        match impl_doc.leaf {
            ImplLeaf::Compact(spans) => {
                if impl_doc.is_std {
                    compact_std.push(spans);
                } else {
                    compact_other.push(spans);
                }
            }
            ImplLeaf::Full(header) => expanded.push((header, impl_doc.assoc_types)),
        }
    }

    let mut content = vec![];

    if !compact_other.is_empty() {
        content.push(DocumentNode::paragraph(join_refs(vec![], compact_other)));
    }

    if !compact_std.is_empty() {
        content.push(DocumentNode::paragraph(join_refs(
            vec![Span::plain("std: ")],
            compact_std,
        )));
    }

    if !expanded.is_empty() {
        let list_items = expanded
            .into_iter()
            .map(|(header, assoc_types)| {
                let mut item_nodes = vec![DocumentNode::generated_code(header)];
                item_nodes.extend(lower_impl_assoc_types(assoc_types));
                ListItem::new(item_nodes)
            })
            .collect();
        content.push(DocumentNode::list(list_items));
    }

    if content.is_empty() {
        vec![]
    } else {
        vec![DocumentNode::section(
            vec![Span::plain("Trait Implementations")],
            content,
        )]
    }
}

/// Join compact trait refs into a comma-separated span sequence, after `prefix`.
fn join_refs<'a>(mut spans: Vec<Span<'a>>, refs: Vec<Vec<Span<'a>>>) -> Vec<Span<'a>> {
    for (i, ref_spans) in refs.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::punctuation(","));
            spans.push(Span::plain(" "));
        }
        spans.extend(ref_spans);
    }
    spans
}

/// Lower an impl's associated-type bindings to indented `type Foo = Bar` lines.
pub(super) fn lower_impl_assoc_types(assoc_types: Vec<ImplAssocType<'_>>) -> Vec<DocumentNode<'_>> {
    assoc_types
        .into_iter()
        .map(|assoc| {
            let mut spans = vec![
                Span::plain("    "),
                Span::keyword("type"),
                Span::plain(" "),
                Span::type_name(assoc.name),
                Span::plain(" "),
                Span::operator("="),
                Span::plain(" "),
            ];
            spans.extend(assoc.type_spans);
            DocumentNode::generated_code(spans)
        })
        .collect()
}
