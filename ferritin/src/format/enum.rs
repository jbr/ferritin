use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

/// Semantic model of an `enum`: name, generics, variants, inherent methods, and
/// trait impls. Parallels [`StructDoc`](super::StructDoc); the `methods` reuse
/// the shared [`MethodDoc`](super::MethodDoc) model.
pub(crate) struct EnumDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) generics: Vec<Span<'a>>,
    pub(crate) where_clause: Vec<Span<'a>>,
    pub(crate) variants: Vec<VariantDoc<'a>>,
    pub(crate) methods: Vec<MethodDoc<'a>>,
    pub(crate) trait_impls: Vec<TraitImplDoc<'a>>,
}

/// A single enum variant.
pub(crate) struct VariantDoc<'a> {
    pub(crate) name: &'a str,
    pub(crate) shape: VariantShape<'a>,
    /// Single-line docs for the Variants section, if the variant has any.
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

/// The three shapes a variant can take.
pub(crate) enum VariantShape<'a> {
    /// A plain (fieldless) variant.
    Plain,
    /// A tuple variant; each entry is one field's resolved type spans.
    Tuple { fields: Vec<Vec<Span<'a>>> },
    /// A struct-like variant.
    Struct { fields: Vec<VariantField<'a>> },
}

/// A named field of a struct-like variant.
pub(crate) struct VariantField<'a> {
    pub(crate) name: &'a str,
    pub(crate) type_spans: Vec<Span<'a>>,
}

impl<'a> Request<'a> {
    /// Resolve an enum item into its semantic [`EnumDoc`] model.
    pub(super) fn model_enum(
        &mut self,
        item: DocRef<'a, Item>,
        enum_data: DocRef<'a, Enum>,
    ) -> EnumDoc<'a> {
        let name = item.name().unwrap_or("<unnamed>");

        let generics = self.format_generics(item, &enum_data.item().generics);
        let where_clause = if !enum_data.generics.where_predicates.is_empty() {
            self.format_where_clause(item, &enum_data.item().generics.where_predicates)
        } else {
            vec![]
        };

        let mut variants = vec![];
        for variant in self.ids(item, &enum_data.item().variants) {
            let ItemEnum::Variant(variant_enum) = &variant.item().inner else {
                continue;
            };
            let variant_name = variant.name().unwrap_or("<unnamed>");

            let shape = match &variant_enum.kind {
                VariantKind::Plain => VariantShape::Plain,
                VariantKind::Tuple(fields) => {
                    let mut field_spans = vec![];
                    for field_id in fields.iter().copied().flatten() {
                        if let Some(field) = enum_data.get(&field_id)
                            && let ItemEnum::StructField(field_type) = &field.item().inner
                        {
                            field_spans.push(self.format_type(item, field_type));
                        }
                    }
                    VariantShape::Tuple {
                        fields: field_spans,
                    }
                }
                VariantKind::Struct { fields, .. } => {
                    let mut variant_fields = vec![];
                    for field in self.ids(item, fields) {
                        if let ItemEnum::StructField(field_type) = &field.item().inner {
                            variant_fields.push(VariantField {
                                name: field.name().unwrap_or("<unnamed>"),
                                type_spans: self.format_type(item, field_type),
                            });
                        }
                    }
                    VariantShape::Struct {
                        fields: variant_fields,
                    }
                }
            };

            let docs = self.docs_to_show(variant, TruncationLevel::SingleLine);
            variants.push(VariantDoc {
                name: variant_name,
                shape,
                docs,
            });
        }

        let methods = self.model_inherent_methods(item);
        let trait_impls = self.model_trait_impls(item);

        EnumDoc {
            name,
            generics,
            where_clause,
            variants,
            methods,
            trait_impls,
        }
    }
}

/// Lower an [`EnumDoc`] to presentation nodes, reproducing the old `format_enum`
/// output byte-for-byte.
pub(super) fn lower_enum(model: EnumDoc<'_>) -> Vec<DocumentNode<'_>> {
    let EnumDoc {
        name,
        generics,
        where_clause,
        variants,
        methods,
        trait_impls,
    } = model;

    let mut code_spans = vec![
        Span::keyword("enum"),
        Span::plain(" "),
        Span::type_name(name),
    ];
    code_spans.extend(generics);
    code_spans.extend(where_clause);
    super::push_body_brace(&mut code_spans);
    code_spans.push(Span::plain("\n"));

    for variant in &variants {
        match &variant.shape {
            VariantShape::Plain => {
                code_spans.push(Span::plain("    "));
                code_spans.push(Span::type_name(variant.name));
                code_spans.push(Span::punctuation(","));
                code_spans.push(Span::plain("\n"));
            }
            VariantShape::Tuple { fields } => {
                code_spans.push(Span::plain("    "));
                code_spans.push(Span::type_name(variant.name));
                code_spans.push(Span::punctuation("("));
                for (i, field_type) in fields.iter().enumerate() {
                    if i > 0 {
                        code_spans.push(Span::punctuation(","));
                        code_spans.push(Span::plain(" "));
                    }
                    code_spans.extend(field_type.iter().cloned());
                }
                code_spans.push(Span::punctuation(")"));
                code_spans.push(Span::punctuation(","));
                code_spans.push(Span::plain("\n"));
            }
            VariantShape::Struct { fields } => {
                code_spans.push(Span::plain("    "));
                code_spans.push(Span::type_name(variant.name));
                code_spans.push(Span::plain(" "));
                code_spans.push(Span::punctuation("{"));
                code_spans.push(Span::plain("\n"));
                for field in fields {
                    code_spans.push(Span::plain("        "));
                    code_spans.push(Span::field_name(field.name));
                    code_spans.push(Span::punctuation(":"));
                    code_spans.push(Span::plain(" "));
                    code_spans.extend(field.type_spans.iter().cloned());
                    code_spans.push(Span::punctuation(","));
                    code_spans.push(Span::plain("\n"));
                }
                code_spans.push(Span::plain("    "));
                code_spans.push(Span::punctuation("}"));
                code_spans.push(Span::punctuation(","));
                code_spans.push(Span::plain("\n"));
            }
        }
    }

    code_spans.push(Span::punctuation("}"));

    let mut doc_nodes = vec![DocumentNode::generated_code(code_spans)];

    let variant_items: Vec<ListItem> = variants
        .into_iter()
        .filter_map(|variant| {
            let docs = variant.docs?;
            let mut content = vec![DocumentNode::paragraph(vec![
                Span::type_name(variant.name),
                Span::plain(" "),
            ])];
            content.extend(docs);
            Some(ListItem::new(content))
        })
        .collect();

    if !variant_items.is_empty() {
        doc_nodes.push(DocumentNode::section(
            vec![Span::plain("Variants:")],
            vec![DocumentNode::list(variant_items)],
        ));
    }

    doc_nodes.extend(super::impls::lower_inherent_methods(methods));
    doc_nodes.extend(super::trait_impls::lower_trait_impls(trait_impls));
    doc_nodes
}
