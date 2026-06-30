use super::*;
use crate::styled_string::{DocumentNode, ListItem, Span};

impl<'a> Request<'a> {
    /// Resolve an item's inherent associated items (methods, assoc consts, assoc
    /// types) into structured [`MethodDoc`]s, sorted by source location.
    pub(super) fn model_inherent_methods(&mut self, item: DocRef<'a, Item>) -> Vec<MethodDoc<'a>> {
        use std::cmp::Ordering;

        let mut items = item
            .methods()
            .filter(|&method| !self.hidden_by_visibility(method))
            .collect::<Vec<_>>();

        items.sort_by(|a, b| match (&a.span, &b.span) {
            (Some(span_a), Some(span_b)) => span_a
                .filename
                .cmp(&span_b.filename)
                .then(span_a.begin.0.cmp(&span_b.begin.0))
                .then(span_a.begin.1.cmp(&span_b.begin.1)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        });

        items.iter().map(|&item| self.model_method(item)).collect()
    }

    /// Model a single associated item (method, assoc const, or assoc type) into a
    /// [`MethodDoc`]. Shared by inherent methods and trait-impl items.
    pub(super) fn model_method(&mut self, item: DocRef<'a, Item>) -> MethodDoc<'a> {
        // Visibility prefix spans, matching the original inline formatting.
        let mut signature = vec![];
        let visibility = match &item.item().visibility {
            Visibility::Public => {
                signature.push(Span::keyword("pub"));
                signature.push(Span::plain(" "));
                MethodVisibility::Public
            }
            Visibility::Crate => {
                signature.push(Span::keyword("pub"));
                signature.push(Span::punctuation("("));
                signature.push(Span::keyword("crate"));
                signature.push(Span::punctuation(")"));
                signature.push(Span::plain(" "));
                MethodVisibility::Crate
            }
            Visibility::Restricted { path, .. } => {
                signature.push(Span::keyword("pub"));
                signature.push(Span::punctuation("("));
                signature.push(Span::plain(path));
                signature.push(Span::punctuation(")"));
                signature.push(Span::plain(" "));
                MethodVisibility::Restricted
            }
            Visibility::Default => MethodVisibility::Default,
        };

        let name = item.name().unwrap_or("<unnamed>");

        let (kind, is_async, is_const, is_unsafe, returns) =
            if let ItemEnum::Function(inner) = &item.item().inner {
                signature.extend(self.format_function_signature(item, name, inner));
                let returns = match &inner.sig.output {
                    Some(output) => self.format_type(item, output),
                    None => vec![],
                };
                (
                    AssocKind::Method,
                    inner.header.is_async,
                    inner.header.is_const,
                    inner.header.is_unsafe,
                    returns,
                )
            } else {
                let (kind_str, kind) = match item.kind() {
                    rustdoc_types::ItemKind::AssocConst => ("const", AssocKind::Const),
                    rustdoc_types::ItemKind::AssocType => ("type", AssocKind::Type),
                    _ => ("", AssocKind::Method),
                };
                if !kind_str.is_empty() {
                    signature.push(Span::keyword(kind_str));
                    signature.push(Span::plain(" "));
                }
                signature.push(Span::plain(name));
                (kind, false, false, false, vec![])
            };

        let docs = self.docs_to_show(item, TruncationLevel::SingleLine);

        MethodDoc {
            name,
            kind,
            visibility,
            is_async,
            is_const,
            is_unsafe,
            returns,
            signature,
            docs,
        }
    }
}

/// Lower structural inherent methods back to the "Methods" section. Empty when
/// there are none (matching the original formatter, which omitted the section).
pub(super) fn lower_inherent_methods<'a>(methods: Vec<MethodDoc<'a>>) -> Vec<DocumentNode<'a>> {
    if methods.is_empty() {
        return vec![];
    }

    let list_items: Vec<ListItem> = methods
        .into_iter()
        .map(|method| {
            let mut item_nodes = vec![DocumentNode::generated_code(method.signature)];
            if let Some(docs) = method.docs {
                item_nodes.extend(docs);
            }
            ListItem::new(item_nodes)
        })
        .collect();

    vec![DocumentNode::section(
        vec![Span::plain("Methods")],
        vec![DocumentNode::list(list_items)],
    )]
}
