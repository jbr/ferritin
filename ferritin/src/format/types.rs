use super::*;
use crate::styled_string::Span;
use rustdoc_types::DynTrait;

impl<'a> Request<'a> {
    /// Enhanced type formatting for signatures
    pub(crate) fn format_type(&mut self, item: DocRef<'a, Item>, type_: &'a Type) -> Vec<Span<'a>> {
        match type_ {
            Type::ResolvedPath(path) => self.format_path(item, path),
            Type::DynTrait(dyn_trait) => self.format_dyn_trait(item, dyn_trait),
            Type::Generic(name) => vec![Span::generic(name)],
            Type::Primitive(prim) => vec![Span::type_name(prim)],
            Type::Array { type_, len } => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_type(item, type_));
                spans.push(Span::punctuation(";"));
                spans.push(Span::plain(" "));
                spans.push(Span::plain(len));
                spans.push(Span::punctuation("]"));
                spans
            }
            Type::Slice(type_) => {
                let mut spans = vec![Span::punctuation("[")];
                spans.extend(self.format_type(item, type_));
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
                spans.extend(self.format_pointee(item, type_));
                spans
            }
            Type::RawPointer { is_mutable, type_ } => {
                let mut spans = vec![
                    Span::operator("*"),
                    Span::keyword(if *is_mutable { "mut" } else { "const" }),
                    Span::plain(" "),
                ];
                spans.extend(self.format_pointee(item, type_));
                spans
            }
            Type::FunctionPointer(fp) => self.format_function_pointer(item, fp),
            Type::Tuple(types) => self.format_tuple(item, types),
            Type::ImplTrait(bounds) => {
                let mut spans = vec![Span::keyword("impl"), Span::plain(" ")];
                spans.extend(self.format_generic_bounds(item, bounds));
                spans
            }
            Type::Infer => vec![Span::plain("_")],
            Type::QualifiedPath {
                name,
                args,
                self_type,
                trait_,
            } => self.format_qualified_path(item, name, args.as_deref(), self_type, trait_),
            Type::Pat { .. } => vec![Span::plain("pattern")],
        }
    }

    /// Format a `dyn` trait object.
    ///
    /// Two pieces live outside the trait paths and are easy to lose: each
    /// trait's higher-ranked binder is a `generic_params` list beside its path
    /// (so `dyn for<'a> Fn(&'a str)` renders as a dangling `dyn Fn(&'a str)`
    /// without it), and the object's own lifetime bound is a field of the
    /// `DynTrait` rather than one of its traits.
    fn format_dyn_trait(
        &mut self,
        item: DocRef<'a, Item>,
        dyn_trait: &'a DynTrait,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![Span::keyword("dyn"), Span::plain(" ")];

        for (i, poly_trait) in dyn_trait.traits.iter().enumerate() {
            if i > 0 {
                spans.push(Span::plain(" + "));
            }
            spans.extend(self.format_hrtb(item, &poly_trait.generic_params));
            spans.extend(self.format_path(item, &poly_trait.trait_));
        }

        if let Some(lifetime) = &dyn_trait.lifetime {
            spans.push(Span::plain(" + "));
            spans.push(Span::lifetime(lifetime));
        }

        spans
    }

    /// Format the type behind a `&`/`&mut`/`*const`/`*mut`, parenthesizing it
    /// when it is a `+`-joined trait object or `impl Trait`.
    ///
    /// `&dyn Error + Send` does not parse — the `+` is ambiguous between
    /// extending the trait object and bounding the reference — so Rust requires
    /// `&(dyn Error + Send)`. A single bound needs no parentheses, and
    /// `Box<dyn Error + Send>` needs none either, since the angle brackets
    /// already delimit it.
    fn format_pointee(&mut self, item: DocRef<'a, Item>, type_: &'a Type) -> Vec<Span<'a>> {
        let bounds = match type_ {
            Type::DynTrait(dyn_trait) => {
                dyn_trait.traits.len() + usize::from(dyn_trait.lifetime.is_some())
            }
            Type::ImplTrait(bounds) => bounds.len(),
            _ => 1,
        };

        if bounds < 2 {
            return self.format_type(item, type_);
        }

        let mut spans = vec![Span::punctuation("(")];
        spans.extend(self.format_type(item, type_));
        spans.push(Span::punctuation(")"));
        spans
    }

    pub(crate) fn format_tuple(
        &mut self,
        item: DocRef<'a, Item>,
        types: &'a [Type],
    ) -> Vec<Span<'a>> {
        let mut spans = vec![Span::punctuation("(")];

        for (i, type_) in types.iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain(" "));
            }
            spans.extend(self.format_type(item, type_));
        }

        spans.push(Span::punctuation(")"));
        spans
    }

    pub(crate) fn format_function_pointer(
        &mut self,
        item: DocRef<'a, Item>,
        fp: &'a FunctionPointer,
    ) -> Vec<Span<'a>> {
        let mut spans = self.format_hrtb(item, &fp.generic_params);

        spans.push(Span::keyword("fn"));
        spans.push(Span::punctuation("("));
        for (i, (_, t)) in fp.sig.inputs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::punctuation(","));
                spans.push(Span::plain(" "));
            }
            spans.extend(self.format_type(item, t));
        }
        spans.push(Span::punctuation(")"));

        if let Some(output) = &fp.sig.output {
            spans.push(Span::plain(" "));
            spans.push(Span::operator("->"));
            spans.push(Span::plain(" "));
            spans.extend(self.format_type(item, output));
        }

        spans
    }

    pub(crate) fn format_qualified_path(
        &mut self,
        item: DocRef<'a, Item>,
        name: &'a str,
        args: Option<&'a GenericArgs>,
        self_type: &'a Type,
        trait_: &'a Option<Path>,
    ) -> Vec<Span<'a>> {
        let mut spans = vec![];

        // For Self::AssociatedType, use simpler syntax when possible
        if matches!(self_type, Type::Generic(s) if s == "Self") {
            if let Some(trait_path) = trait_ {
                let trait_spans = self.format_path(item, trait_path);
                if trait_spans.is_empty() {
                    // If trait path is empty, just use Self::name
                    spans.push(Span::generic("Self"));
                    spans.push(Span::punctuation("::"));
                    spans.push(Span::type_name(name));
                    if let Some(args) = args {
                        spans.extend(self.format_generic_args(item, args));
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
                        spans.extend(self.format_generic_args(item, args));
                    }
                    return spans;
                }
            } else {
                // No trait specified, use Self::name
                spans.push(Span::generic("Self"));
                spans.push(Span::punctuation("::"));
                spans.push(Span::plain(name));
                if let Some(args) = args {
                    spans.extend(self.format_generic_args(item, args));
                }
                return spans;
            }
        }

        // For other types, use full qualified syntax: <Type as Trait>::name
        // If the trait path is empty (rustdoc sometimes omits it), fall back to
        // the bare `Type::name` form rather than emitting `<Type as >::name`.
        let trait_spans = trait_
            .as_ref()
            .map(|trait_path| self.format_path(item, trait_path))
            .unwrap_or_default();

        if trait_spans.is_empty() {
            spans.extend(self.format_type(item, self_type));
            spans.push(Span::punctuation("::"));
            spans.push(Span::plain(name));
            if let Some(args) = args {
                spans.extend(self.format_generic_args(item, args));
            }
            return spans;
        }

        spans.push(Span::punctuation("<"));
        spans.extend(self.format_type(item, self_type));
        spans.push(Span::plain(" "));
        spans.push(Span::keyword("as"));
        spans.push(Span::plain(" "));
        spans.extend(trait_spans);
        spans.push(Span::punctuation(">"));
        spans.push(Span::punctuation("::"));
        spans.push(Span::plain(name));
        if let Some(args) = args {
            spans.extend(self.format_generic_args(item, args));
        }
        spans
    }
}
