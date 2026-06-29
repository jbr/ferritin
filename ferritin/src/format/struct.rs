use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span, TruncationLevel};

/// Semantic model of a `struct` item: the resolved domain data (name, generics,
/// fields) *before* any presentation lowering. Built by [`Request::model_struct`]
/// and turned into presentation [`DocumentNode`]s by [`lower_struct`].
///
/// Signature-level references (field types, generics) stay as span sequences —
/// the shared "leaf" vocabulary that survives across IR levels — while the
/// struct's own structure (shape, fields) is explicit, so a non-terminal
/// consumer (e.g. the JSON output) can render it at a higher level than "spans
/// in a code block".
pub(crate) struct StructDoc<'a> {
    pub(crate) name: &'a str,
    /// Generic-parameter spans (`<T, U>`); empty when the struct has none.
    pub(crate) generics: Vec<Span<'a>>,
    /// `where`-clause spans; empty when there are none.
    pub(crate) where_clause: Vec<Span<'a>>,
    pub(crate) shape: StructShape<'a>,
    /// Associated inherent methods, still as opaque presentation nodes. Unit 3
    /// will model these structurally (`Vec<MethodDoc>`); for now they are
    /// carried verbatim so the struct body lowers identically and the JSON
    /// output can at least emit them as a rendered block.
    pub(crate) methods: Vec<DocumentNode<'a>>,
}

/// The three structural shapes a struct can take, each carrying its own fields.
pub(crate) enum StructShape<'a> {
    Unit,
    Tuple {
        fields: Vec<TupleField<'a>>,
        hidden_count: usize,
    },
    Plain {
        fields: Vec<PlainField<'a>>,
        hidden_count: usize,
        has_stripped_fields: bool,
    },
}

/// A named field of a plain struct.
pub(crate) struct PlainField<'a> {
    /// Field name; `None` for an unnamed field (rendered as `<unnamed>`).
    pub(crate) name: Option<&'a str>,
    /// Whether the field is declared `pub`.
    pub(crate) is_pub: bool,
    /// The field's type, as a resolved span sequence.
    pub(crate) type_spans: Vec<Span<'a>>,
    /// Single-line docs for the Fields section, if the field has any.
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

/// A positional field of a tuple struct.
pub(crate) struct TupleField<'a> {
    pub(crate) index: usize,
    pub(crate) is_pub: bool,
    pub(crate) type_spans: Vec<Span<'a>>,
    pub(crate) docs: Option<Vec<DocumentNode<'a>>>,
}

impl<'a> Request<'a> {
    /// Resolve a struct item into its semantic [`StructDoc`] model — the half of
    /// the old `format_*struct` functions that does index lookups and type
    /// resolution, with the span-assembly half moved to [`lower_struct`].
    pub(super) fn model_struct(
        &mut self,
        item: DocRef<'a, Item>,
        struct_data: DocRef<'a, Struct>,
    ) -> StructDoc<'a> {
        let name = item.name().unwrap_or("<unnamed>");

        let generics = if !struct_data.generics.params.is_empty() {
            self.format_generics(item, &struct_data.item().generics)
        } else {
            vec![]
        };
        let where_clause = if !struct_data.generics.where_predicates.is_empty() {
            self.format_where_clause(item, &struct_data.item().generics.where_predicates)
        } else {
            vec![]
        };

        let shape = match &struct_data.kind {
            StructKind::Unit => StructShape::Unit,
            StructKind::Tuple(fields) => self.model_tuple_fields(item, struct_data, fields),
            StructKind::Plain {
                fields,
                has_stripped_fields,
            } => self.model_plain_fields(item, fields, *has_stripped_fields),
        };

        let methods = self.format_associated_methods(item);

        StructDoc {
            name,
            generics,
            where_clause,
            shape,
            methods,
        }
    }

    fn model_plain_fields(
        &mut self,
        item: DocRef<'a, Item>,
        fields: &[Id],
        has_stripped_fields: bool,
    ) -> StructShape<'a> {
        let (visible_fields, hidden_count) = self.categorize_fields(item, fields);

        let mut model_fields = Vec::new();
        for field in &visible_fields {
            if let ItemEnum::StructField(field_type) = &field.item().inner {
                model_fields.push(PlainField {
                    name: field.name(),
                    is_pub: matches!(field.item().visibility, Visibility::Public),
                    type_spans: self.format_type(item, field_type),
                    docs: self.docs_to_show(*field, TruncationLevel::SingleLine),
                });
            }
        }

        StructShape::Plain {
            fields: model_fields,
            hidden_count,
            has_stripped_fields,
        }
    }

    fn model_tuple_fields(
        &mut self,
        item: DocRef<'a, Item>,
        struct_data: DocRef<'a, Struct>,
        fields: &[Option<Id>],
    ) -> StructShape<'a> {
        let mut model_fields = Vec::new();
        let mut hidden_count = 0;

        for (i, field_id_opt) in fields.iter().enumerate() {
            if let Some(field_id) = field_id_opt
                && let Some(field) = struct_data.get(field_id)
                && !self.hidden_by_visibility(field)
            {
                // A visible non-`StructField` field produces no output and is
                // not counted as hidden, matching the original formatter.
                if let ItemEnum::StructField(field_type) = &field.item().inner {
                    model_fields.push(TupleField {
                        index: i,
                        is_pub: matches!(field.item().visibility, Visibility::Public),
                        type_spans: self.format_type(item, field_type),
                        docs: self.docs_to_show(field, TruncationLevel::SingleLine),
                    });
                }
            } else {
                hidden_count += 1;
            }
        }

        StructShape::Tuple {
            fields: model_fields,
            hidden_count,
        }
    }

    /// Partition struct fields into visible `DocRef`s and a hidden count,
    /// honoring the `--public` visibility filter.
    fn categorize_fields(
        &self,
        item: DocRef<'a, Item>,
        fields: &[Id],
    ) -> (Vec<DocRef<'a, Item>>, usize) {
        let mut visible_fields = Vec::new();
        let mut hidden_count = 0;

        for field_id in fields {
            match item.get(field_id) {
                Some(field) if !self.hidden_by_visibility(field) => {
                    visible_fields.push(field);
                }
                _ => hidden_count += 1,
            }
        }

        (visible_fields, hidden_count)
    }
}

/// Lower a [`StructDoc`] to presentation [`DocumentNode`]s. This is the
/// terminal-facing half: it must reproduce the old formatters' output
/// byte-for-byte (insta snapshots are the guardrail).
pub(super) fn lower_struct(model: StructDoc<'_>) -> Vec<DocumentNode<'_>> {
    let StructDoc {
        name,
        generics,
        where_clause,
        shape,
        methods,
    } = model;

    let mut code_spans = vec![
        Span::keyword("struct"),
        Span::plain(" "),
        Span::type_name(name),
    ];
    code_spans.extend(generics);
    code_spans.extend(where_clause);

    let mut doc_nodes = match shape {
        StructShape::Unit => {
            code_spans.push(Span::punctuation(";"));
            vec![DocumentNode::generated_code(code_spans)]
        }
        StructShape::Tuple {
            fields,
            hidden_count,
        } => lower_tuple(code_spans, fields, hidden_count),
        StructShape::Plain {
            fields,
            hidden_count,
            has_stripped_fields,
        } => lower_plain(code_spans, fields, hidden_count, has_stripped_fields),
    };

    doc_nodes.extend(methods);
    doc_nodes
}

fn lower_plain<'a>(
    mut code_spans: Vec<Span<'a>>,
    fields: Vec<PlainField<'a>>,
    hidden_count: usize,
    has_stripped_fields: bool,
) -> Vec<DocumentNode<'a>> {
    code_spans.push(Span::plain(" "));
    code_spans.push(Span::punctuation("{"));
    code_spans.push(Span::plain("\n"));

    for field in &fields {
        code_spans.push(Span::plain("    "));
        if field.is_pub {
            code_spans.push(Span::keyword("pub"));
            code_spans.push(Span::plain(" "));
        }
        code_spans.push(Span::field_name(field.name.unwrap_or("<unnamed>")));
        code_spans.push(Span::punctuation(":"));
        code_spans.push(Span::plain(" "));
        code_spans.extend(field.type_spans.iter().cloned());
        code_spans.push(Span::punctuation(","));
        code_spans.push(Span::plain("\n"));
    }

    if hidden_count > 0 {
        code_spans.push(Span::plain("    "));
        code_spans.push(Span::comment(format!(
            "// ... {} private field{} hidden",
            hidden_count,
            if hidden_count == 1 { "" } else { "s" }
        )));
        code_spans.push(Span::plain("\n"));
    } else if fields.is_empty() && has_stripped_fields {
        // Rustdoc stripped all fields from the public view but the struct
        // still has a body — surface that explicitly.
        code_spans.push(Span::plain("    "));
        code_spans.push(Span::comment("/* private fields */"));
        code_spans.push(Span::plain("\n"));
    }

    code_spans.push(Span::punctuation("}"));

    let mut doc_nodes = vec![DocumentNode::generated_code(code_spans)];

    let field_items: Vec<ListItem> = fields
        .into_iter()
        .filter_map(|field| {
            let name = field.name?;
            let docs = field.docs?;
            let mut signature_spans = vec![
                Span::field_name(name),
                Span::punctuation(":"),
                Span::plain(" "),
            ];
            signature_spans.extend(field.type_spans);

            let mut item_nodes = vec![DocumentNode::generated_code(signature_spans)];
            item_nodes.extend(docs);
            Some(ListItem::new(item_nodes))
        })
        .collect();

    if !field_items.is_empty() {
        doc_nodes.push(DocumentNode::section(
            vec![Span::plain("Fields:")],
            vec![DocumentNode::list(field_items)],
        ));
    }

    doc_nodes
}

fn lower_tuple<'a>(
    mut code_spans: Vec<Span<'a>>,
    fields: Vec<TupleField<'a>>,
    hidden_count: usize,
) -> Vec<DocumentNode<'a>> {
    code_spans.push(Span::punctuation("("));
    code_spans.push(Span::plain("\n"));

    for field in &fields {
        code_spans.push(Span::plain("    "));
        if field.is_pub {
            code_spans.push(Span::keyword("pub"));
            code_spans.push(Span::plain(" "));
        }
        code_spans.extend(field.type_spans.iter().cloned());
        code_spans.push(Span::punctuation(","));
        code_spans.push(Span::plain(" "));
        code_spans.push(Span::comment(format!("// field {}", field.index)));
        code_spans.push(Span::plain("\n"));
    }

    if hidden_count > 0 {
        code_spans.push(Span::plain("    "));
        code_spans.push(Span::comment(format!(
            "// ... {} private field{} hidden",
            hidden_count,
            if hidden_count == 1 { "" } else { "s" }
        )));
        code_spans.push(Span::plain("\n"));
    }

    code_spans.push(Span::punctuation(")"));

    let mut doc_nodes = vec![DocumentNode::generated_code(code_spans)];

    let field_items: Vec<ListItem> = fields
        .into_iter()
        .filter_map(|field| {
            let docs = field.docs?;
            let mut signature_spans = vec![Span::plain(format!("Field {}: ", field.index))];
            signature_spans.extend(field.type_spans);

            let mut item_nodes = vec![DocumentNode::generated_code(signature_spans)];
            item_nodes.extend(docs);
            Some(ListItem::new(item_nodes))
        })
        .collect();

    if !field_items.is_empty() {
        doc_nodes.push(DocumentNode::section(
            vec![Span::plain("Fields:")],
            vec![DocumentNode::list(field_items)],
        ));
    }

    doc_nodes
}
