use super::*;
use crate::styled_string::{DocumentNode, Span};

/// Semantic model of a `union` item. A union is a plain struct restricted to
/// named fields, so it reuses [`PlainField`] and the struct field machinery —
/// it differs only in the `union` keyword. Like a struct, it can carry generics,
/// inherent methods, and trait impls.
pub(crate) struct UnionDoc<'a> {
    pub(crate) name: &'a str,
    /// Generic-parameter spans (`<T, U>`); empty when the union has none.
    pub(crate) generics: Vec<Span<'a>>,
    /// `where`-clause spans; empty when there are none.
    pub(crate) where_clause: Vec<Span<'a>>,
    pub(crate) fields: Vec<PlainField<'a>>,
    pub(crate) hidden_count: usize,
    pub(crate) has_stripped_fields: bool,
    /// Inherent associated items (methods, assoc consts/types).
    pub(crate) methods: Vec<MethodDoc<'a>>,
    /// Trait implementations, structurally modeled, lowered after the inherent
    /// methods.
    pub(crate) trait_impls: Vec<TraitImplDoc<'a>>,
}

impl<'a> Request<'a> {
    /// Resolve a union item into its semantic [`UnionDoc`] model — the
    /// resolution half, mirroring `model_struct`'s plain-field path.
    pub(super) fn model_union(
        &mut self,
        item: DocRef<'a, Item>,
        union: DocRef<'a, Union>,
    ) -> UnionDoc<'a> {
        let name = item.name().unwrap_or("<unnamed>");

        let generics = self.format_generics(item, &union.item().generics);
        let where_clause = if !union.generics.where_predicates.is_empty() {
            self.format_where_clause(item, &union.item().generics.where_predicates)
        } else {
            vec![]
        };

        let (fields, hidden_count) = self.model_named_fields(item, &union.item().fields);
        let methods = self.model_inherent_methods(item);
        let trait_impls = self.model_trait_impls(item);

        UnionDoc {
            name,
            generics,
            where_clause,
            fields,
            hidden_count,
            has_stripped_fields: union.has_stripped_fields,
            methods,
            trait_impls,
        }
    }
}

/// Lower a [`UnionDoc`] to presentation [`DocumentNode`]s: identical to a plain
/// struct but for the `union` keyword.
pub(super) fn lower_union(model: UnionDoc<'_>) -> Vec<DocumentNode<'_>> {
    let UnionDoc {
        name,
        generics,
        where_clause,
        fields,
        hidden_count,
        has_stripped_fields,
        methods,
        trait_impls,
    } = model;

    let mut code_spans = vec![
        Span::keyword("union"),
        Span::plain(" "),
        Span::type_name(name),
    ];
    code_spans.extend(generics);
    code_spans.extend(where_clause);

    let mut doc_nodes =
        super::r#struct::lower_plain(code_spans, fields, hidden_count, has_stripped_fields);
    doc_nodes.extend(super::impls::lower_inherent_methods(methods));
    doc_nodes.extend(super::trait_impls::lower_trait_impls(trait_impls));
    doc_nodes
}
