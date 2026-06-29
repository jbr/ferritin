use super::*;
use crate::styled_string::{DocumentNode, Span};

/// Semantic model of a `type` alias: its name and the aliased type (the RHS of
/// `=`) as a resolved span sequence. Generics are intentionally absent — the
/// current presentation doesn't render them, and this stays byte-identical.
pub(crate) struct TypeAliasDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) aliased: Vec<Span<'a>>,
}

/// Semantic model of a `const` item: name, type, and optional value expression.
pub(crate) struct ConstantDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) type_signature: Vec<Span<'a>>,
    /// Value expression, as written; `None` when rustdoc elides it.
    pub(crate) value: Option<&'a str>,
}

/// Semantic model of a `static` item: name, type, and value expression.
pub(crate) struct StaticDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) type_signature: Vec<Span<'a>>,
    pub(crate) value: &'a str,
}

/// Semantic model of a `macro_rules!` item: its definition source verbatim.
pub(crate) struct MacroDoc<'a> {
    pub(crate) definition: &'a str,
}

impl<'a> Request<'a> {
    /// Resolve a type-alias item into its [`TypeAliasDoc`] model.
    pub(super) fn model_type_alias(
        &mut self,
        item: DocRef<'a, Item>,
        type_alias: DocRef<'a, TypeAlias>,
    ) -> TypeAliasDoc<'a> {
        TypeAliasDoc {
            name: item.name().unwrap_or("<unnamed>"),
            aliased: self.format_type(item, &type_alias.item().type_),
        }
    }

    /// Resolve a constant item into its [`ConstantDoc`] model.
    pub(super) fn model_constant(
        &mut self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        const_: &'a Constant,
    ) -> ConstantDoc<'a> {
        ConstantDoc {
            name: item.name().unwrap_or("<unnamed>"),
            type_signature: self.format_type(item, type_),
            value: const_.value.as_deref(),
        }
    }

    /// Resolve a static item into its [`StaticDoc`] model.
    pub(super) fn model_static(
        &mut self,
        item: DocRef<'a, Item>,
        static_item: &'a Static,
    ) -> StaticDoc<'a> {
        StaticDoc {
            name: item.name().unwrap_or("<unnamed>"),
            type_signature: self.format_type(item, &static_item.type_),
            value: &static_item.expr,
        }
    }

    /// Format a union
    pub(crate) fn format_union(
        &mut self,
        _item: DocRef<'a, Item>,
        _union: DocRef<'a, Union>,
    ) -> Vec<DocumentNode<'a>> {
        // TODO: Implement union formatting
        vec![DocumentNode::paragraph(vec![Span::plain(
            "[Union formatting not yet implemented]",
        )])]
    }
}

/// Lower a [`TypeAliasDoc`] to presentation nodes: `type <name> = <aliased>;`.
pub(super) fn lower_type_alias(model: TypeAliasDoc<'_>) -> Vec<DocumentNode<'_>> {
    let TypeAliasDoc { name, aliased } = model;

    let mut spans = vec![
        Span::keyword("type"),
        Span::plain(" "),
        Span::type_name(name),
        Span::plain(" "),
        Span::operator("="),
        Span::plain(" "),
    ];
    spans.extend(aliased);
    spans.push(Span::punctuation(";"));

    vec![DocumentNode::generated_code(spans)]
}

/// Lower a [`ConstantDoc`]: `const <name>: <type> [= <value>];`.
pub(super) fn lower_constant(model: ConstantDoc<'_>) -> Vec<DocumentNode<'_>> {
    let ConstantDoc {
        name,
        type_signature,
        value,
    } = model;

    let mut spans = vec![
        Span::keyword("const"),
        Span::plain(" "),
        Span::plain(name),
        Span::punctuation(":"),
        Span::plain(" "),
    ];
    spans.extend(type_signature);

    if let Some(value) = value {
        spans.push(Span::plain(" "));
        spans.push(Span::operator("="));
        spans.push(Span::plain(" "));
        spans.push(Span::inline_code(value));
    }

    spans.push(Span::punctuation(";"));

    vec![DocumentNode::generated_code(spans)]
}

/// Lower a [`StaticDoc`]: `static <name>: <type> = <value>;`.
pub(super) fn lower_static(model: StaticDoc<'_>) -> Vec<DocumentNode<'_>> {
    let StaticDoc {
        name,
        type_signature,
        value,
    } = model;

    let mut spans = vec![
        Span::keyword("static"),
        Span::plain(" "),
        Span::plain(name),
        Span::punctuation(":"),
        Span::plain(" "),
    ];
    spans.extend(type_signature);
    spans.push(Span::plain(" "));
    spans.push(Span::operator("="));
    spans.push(Span::plain(" "));
    spans.push(Span::inline_code(value));
    spans.push(Span::punctuation(";"));

    vec![DocumentNode::generated_code(spans)]
}

/// Lower a [`MacroDoc`]: a `Macro definition:` label and a rust code block.
pub(super) fn lower_macro(model: MacroDoc<'_>) -> Vec<DocumentNode<'_>> {
    vec![
        DocumentNode::paragraph(vec![Span::plain("Macro definition:")]),
        DocumentNode::code_block(Some("rust"), model.definition),
    ]
}
