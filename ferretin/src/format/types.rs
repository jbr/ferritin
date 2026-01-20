use super::*;
use crate::styled_string::Span;

impl Request {
    /// Enhanced type formatting for signatures
    pub(crate) fn format_type<'a>(&self, type_: &'a Type) -> Vec<Span<'a>> {
        match type_ {
            Type::ResolvedPath(path) => self.format_path(path),
            Type::DynTrait(dyn_trait) => {
                let mut spans = vec![Span::keyword("dyn"), Span::plain(" ")];
                for (i, t) in dyn_trait.traits.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::plain(" + "));
                    }
                    spans.extend(self.format_path(&t.trait_));
                }
                spans
            }
            Type::Generic(name) => vec![Span::generic(name)],
            Type::Primitive(prim) => vec![Span::type_name(prim)],
            Type::Array { type_, len } => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_type(type_));
                spans.push(Span::punctuation(";"));
                spans.push(Span::plain(" "));
                spans.push(Span::plain(len));
                spans.push(Span::punctuation("]"));
                spans
            }
            Type::Slice(type_) => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_type(type_));
                spans.push(Span::punctuation("]"));
                spans
            }
            Type::BorrowedRef {
                lifetime,
                is_mutable,
                type_,
                ..
            } => {
                let mut spans = vec![Span::operator("&")];
                if let Some(lt) = lifetime {
                    spans.push(Span::lifetime(lt));
                    spans.push(Span::plain(" "));
                }
                if *is_mutable {
                    spans.push(Span::keyword("mut"));
                    spans.push(Span::plain(" "));
                }
                spans.extend(self.format_type(type_));
                spans
            }
            Type::RawPointer { is_mutable, type_ } => {
                let mut spans = vec![
                    Span::operator("*"),
                    Span::keyword(if *is_mutable { "mut" } else { "const" }),
                    Span::plain(" "),
                ];
                spans.extend(self.format_type(type_));
                spans
            }
            Type::FunctionPointer(fp) => self.format_function_pointer(fp),
            Type::Tuple(types) => self.format_tuple(types),
            Type::ImplTrait(bounds) => {
                let mut spans = vec![Span::keyword("impl"), Span::plain(" ")];
                spans.extend(self.format_generic_bounds(bounds));
                spans
            }
            Type::Infer => vec![Span::plain("_")],
            Type::QualifiedPath {
                name,
                args,
                self_type,
                trait_,
            } => self.format_qualified_path(name, args.as_deref(), self_type, trait_),
            Type::Pat { .. } => vec![Span::plain("pattern")],
        }
    }

    pub(crate) fn format_tuple<'a>(&self, types: &'a [Type]) -> Vec<Span<'a>> {
        let mut spans = vec![Span::punctuation("(")];

        for (i, type_) in types.iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain(" "));
            }
            spans.extend(self.format_type(type_));
        }

        spans.push(Span::punctuation(")"));
        spans
    }

    pub(crate) fn format_function_pointer<'a>(&self, fp: &'a FunctionPointer) -> Vec<Span<'a>> {
        let mut spans = vec![];

        if !fp.generic_params.is_empty() {
            spans.push(Span::keyword("for"));
            spans.push(Span::punctuation("<"));
            for (i, p) in fp.generic_params.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::punctuation(","));
                    spans.push(Span::plain(" "));
                }
                spans.extend(self.format_generic_param(p));
            }
            spans.push(Span::punctuation(">"));
            spans.push(Span::plain(" "));
        }

        spans.push(Span::keyword("fn"));
        spans.push(Span::punctuation("("));
        for (i, (_, t)) in fp.sig.inputs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain(" "));
            }
            spans.extend(self.format_type(t));
        }
        spans.push(Span::punctuation(")"));

        if let Some(output) = &fp.sig.output {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("->"));
            spans.push(Span::plain(" "));
            spans.extend(self.format_type(output));
        }

        spans
    }

    pub(crate) fn format_qualified_path<'a>(
        &self,
        name: &'a str,
        args: Option<&'a GenericArgs>,
        self_type: &'a Type,
        trait_: &'a Option<Path>,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![];

        // For Self::AssociatedType, use simpler syntax when possible
        if matches!(self_type, Type::Generic(s) if s == "Self") {
            if let Some(trait_path) = trait_ {
                let trait_spans = self.format_path(trait_path);
                if trait_spans.is_empty() {
                    // If trait path is empty, just use Self::name
                    spans.push(Span::generic("Self"));
                    spans.push(Span::punctuation("::"));
                    spans.push(Span::type_name(name));
                    if let Some(args) = args {
                        spans.extend(self.format_generic_args(args));
                    }
                    return spans;
                } else {
                    // Use full qualified syntax: <Self as Trait>::name
                    spans.push(Span::punctuation("<"));
                    spans.push(Span::generic("Self"));
                    spans.push(Span::plain(" "));
                    spans.push(Span::keyword("as"));
                    spans.push(Span::plain(" "));
                    spans.extend(trait_spans);
                    spans.push(Span::punctuation(">"));
                    spans.push(Span::punctuation("::"));
                    spans.push(Span::type_name(name));
                    if let Some(args) = args {
                        spans.extend(self.format_generic_args(args));
                    }
                    return spans;
                }
            } else {
                // No trait specified, use Self::name
                spans.push(Span::generic("Self"));
                spans.push(Span::punctuation("::"));
                spans.push(Span::plain(name));
                if let Some(args) = args {
                    spans.extend(self.format_generic_args(args));
                }
                return spans;
            }
        }

        // For other types, use full qualified syntax
        spans.push(Span::punctuation("<"));
        spans.extend(self.format_type(self_type));
        if let Some(trait_path) = trait_ {
            spans.push(Span::plain(" "));
            spans.push(Span::keyword("as"));
            spans.push(Span::plain(" "));
            spans.extend(self.format_path(trait_path));
        }
        spans.push(Span::punctuation(">"));
        spans.push(Span::punctuation("::"));
        spans.push(Span::plain(name));
        if let Some(args) = args {
            spans.extend(self.format_generic_args(args));
        }
        spans
    }
}
