use super::*;
use crate::styled_string::{DocumentNode, Span as StyledSpan};
use rustdoc_types::{
    AssocItemConstraint, AssocItemConstraintKind, PreciseCapturingArg, TraitBoundModifier,
};

/// Is this generic parameter one rustdoc synthesized for an `impl Trait`
/// argument rather than one the author wrote?
///
/// `fn set_data(data: impl Into<String>)` is lowered to a parameter *named*
/// `impl Into<String>`, bounded by the `impl`'s own bounds and flagged
/// `is_synthetic`. Rendering it in the parameter list produces
/// `fn set_data<impl Into<String>: Into<String>>(data: impl Into<String>)`,
/// which is not Rust — and the bounds are already spelled out at the parameter
/// that introduced them, so nothing is lost by eliding it.
fn is_synthetic(param: &GenericParamDef) -> bool {
    matches!(
        param.kind,
        GenericParamDefKind::Type {
            is_synthetic: true,
            ..
        }
    )
}

/// Semantic model of a free `function` item. A function is the same fn-shape as
/// an inherent [`MethodDoc`], minus the assoc-item `kind`/`visibility` (a free
/// function isn't an associated item, and its signature carries no `pub` prefix)
/// and the per-item `docs` (a top-level item's own prose renders via
/// [`ItemDoc`](super::ItemDoc), not in the body). Parameters stay inside the
/// `signature` span sequence — the shared leaf vocabulary — so a function and a
/// method serialize identically.
pub(crate) struct FunctionDoc<'a> {
    pub(crate) name: &'a str,
    /// `true` only for `async fn`.
    pub(crate) is_async: bool,
    pub(crate) is_const: bool,
    pub(crate) is_unsafe: bool,
    /// Return-type spans (functions with an explicit output); empty otherwise.
    pub(crate) returns: Vec<StyledSpan<'a>>,
    /// Full display signature — the lowering leaf.
    pub(crate) signature: Vec<StyledSpan<'a>>,
}

impl<'a> Request<'a> {
    /// Resolve a function item into its semantic [`FunctionDoc`] model — the
    /// resolution half of the old `format_function`, with span assembly left to
    /// [`lower_function`].
    pub(super) fn model_function(
        &mut self,
        item: DocRef<'a, Item>,
        function: DocRef<'a, Function>,
    ) -> FunctionDoc<'a> {
        let name = item.name().unwrap_or("<unnamed>");
        let func = function.item();
        let signature = self.format_function_signature(item, name, func);
        let returns = match &func.sig.output {
            Some(output) => self.format_type(item, output),
            None => vec![],
        };

        FunctionDoc {
            name,
            is_async: func.header.is_async,
            is_const: func.header.is_const,
            is_unsafe: func.header.is_unsafe,
            returns,
            signature,
        }
    }

    /// Format a function signature
    pub(super) fn format_function_signature(
        &mut self,
        item: DocRef<'a, Item>,
        name: &'a str,
        func: &'a Function,
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = vec![];

        // Add function modifiers in the correct order
        if func.header.is_const {
            spans.push(StyledSpan::keyword("const"));
            spans.push(StyledSpan::plain(" "));
        }

        if func.header.is_async {
            spans.push(StyledSpan::keyword("async"));
            spans.push(StyledSpan::plain(" "));
        }

        if func.header.is_unsafe {
            spans.push(StyledSpan::keyword("unsafe"));
            spans.push(StyledSpan::plain(" "));
        }

        // Add ABI specification if not default Rust ABI
        match func.header.abi {
            Abi::Rust => {}
            Abi::C { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"C-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"C\" "));
                }
            }
            Abi::Cdecl { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"cdecl-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"cdecl\" "));
                }
            }
            Abi::Stdcall { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"stdcall-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"stdcall\" "));
                }
            }
            Abi::Fastcall { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"fastcall-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"fastcall\" "));
                }
            }
            Abi::Aapcs { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"aapcs-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"aapcs\" "));
                }
            }
            Abi::Win64 { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"win64-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"win64\" "));
                }
            }
            Abi::SysV64 { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"sysv64-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"sysv64\" "));
                }
            }
            Abi::System { unwind } => {
                if unwind {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"system-unwind\" "));
                } else {
                    spans.push(StyledSpan::keyword("extern"));
                    spans.push(StyledSpan::plain(" \"system\" "));
                }
            }
            Abi::Other(ref abi_name) => {
                spans.push(StyledSpan::keyword("extern"));
                spans.push(StyledSpan::plain(format!(" \"{abi_name}\" ")));
            }
        }

        // Add function name and generics
        spans.push(StyledSpan::keyword("fn"));
        spans.push(StyledSpan::plain(" "));
        spans.push(StyledSpan::plain(name).with_target(Some(item)));
        spans.extend(self.format_generics(item, &func.generics));
        spans.push(StyledSpan::punctuation("("));

        // Add parameters
        for (i, (param_name, param_type)) in func.sig.inputs.iter().enumerate() {
            if i > 0 {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            spans.extend(self.format_parameter(item, param_name, param_type));
        }
        spans.push(StyledSpan::punctuation(")"));

        // Add return type if not unit
        if let Some(output) = &func.sig.output {
            spans.push(StyledSpan::plain(" "));
            spans.push(StyledSpan::operator("->"));
            spans.push(StyledSpan::plain(" "));
            spans.extend(self.format_type(item, output));
        }

        // Add where clause if present
        if !func.generics.where_predicates.is_empty() {
            spans.extend(self.format_where_clause(item, &func.generics.where_predicates));
        }

        spans
    }

    /// Format a function parameter with idiomatic self shorthand
    pub(super) fn format_parameter(
        &mut self,
        item: DocRef<'a, Item>,
        param_name: &'a str,
        param_type: &'a Type,
    ) -> Vec<StyledSpan<'a>> {
        // Handle self parameters with idiomatic shorthand
        if param_name == "self" {
            match param_type {
                // self: Self -> self
                Type::Generic(name) if name == "Self" => vec![StyledSpan::plain("self")],
                // self: &Self -> &self
                Type::BorrowedRef {
                    lifetime: None,
                    is_mutable: false,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    vec![StyledSpan::punctuation("&"), StyledSpan::plain("self")]
                }
                // self: &mut Self -> &mut self
                Type::BorrowedRef {
                    lifetime: None,
                    is_mutable: true,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    vec![
                        StyledSpan::punctuation("&"),
                        StyledSpan::keyword("mut"),
                        StyledSpan::plain(" "),
                        StyledSpan::plain("self"),
                    ]
                }
                // self: &'a Self -> &'a self
                Type::BorrowedRef {
                    lifetime: Some(lifetime),
                    is_mutable: false,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    vec![
                        StyledSpan::punctuation("&"),
                        StyledSpan::lifetime(lifetime),
                        StyledSpan::plain(" "),
                        StyledSpan::plain("self"),
                    ]
                }
                // self: &'a mut Self -> &'a mut self
                Type::BorrowedRef {
                    lifetime: Some(lifetime),
                    is_mutable: true,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    vec![
                        StyledSpan::punctuation("&"),
                        StyledSpan::lifetime(lifetime),
                        StyledSpan::plain(" "),
                        StyledSpan::keyword("mut"),
                        StyledSpan::plain(" "),
                        StyledSpan::plain("self"),
                    ]
                }
                // For any other self type, use the full form
                _ => {
                    let mut spans = vec![
                        StyledSpan::plain(param_name),
                        StyledSpan::punctuation(":"),
                        StyledSpan::plain(" "),
                    ];
                    spans.extend(self.format_type(item, param_type));
                    spans
                }
            }
        } else {
            // For non-self parameters, use the standard format
            let mut spans = vec![
                StyledSpan::plain(param_name),
                StyledSpan::punctuation(":"),
                StyledSpan::plain(" "),
            ];
            spans.extend(self.format_type(item, param_type));
            spans
        }
    }

    /// Format a generic parameter list (`<T, 'a, const N: usize>`) for a
    /// signature, eliding the parameters rustdoc synthesized for `impl Trait`
    /// arguments (see [`is_synthetic`]). Returns no spans at all — not an empty
    /// `<>` — when every parameter is elided or there were none to begin with.
    pub(super) fn format_generics(
        &mut self,
        item: DocRef<'a, Item>,
        generics: &'a Generics,
    ) -> Vec<StyledSpan<'a>> {
        self.format_generic_param_list(item, &generics.params)
    }

    /// The shared `<..>` rendering behind [`format_generics`] and every
    /// higher-ranked `for<..>` binder.
    fn format_generic_param_list(
        &mut self,
        item: DocRef<'a, Item>,
        params: &'a [GenericParamDef],
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = vec![];

        for param in params.iter().filter(|param| !is_synthetic(param)) {
            if spans.is_empty() {
                spans.push(StyledSpan::punctuation("<"));
            } else {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            spans.extend(self.format_generic_param(item, param));
        }

        if !spans.is_empty() {
            spans.push(StyledSpan::punctuation(">"));
        }
        spans
    }

    /// Format a higher-ranked binder — the `for<'a>` of an HRTB — followed by a
    /// trailing space, or nothing when the binder is empty. Rustdoc records
    /// these in a `generic_params` list *beside* the thing they bind (a trait
    /// bound, a `dyn` object's trait, a `where` predicate, a function pointer),
    /// so every one of those has to reassemble the `for<..>` itself.
    pub(super) fn format_hrtb(
        &mut self,
        item: DocRef<'a, Item>,
        generic_params: &'a [GenericParamDef],
    ) -> Vec<StyledSpan<'a>> {
        let params = self.format_generic_param_list(item, generic_params);
        if params.is_empty() {
            return vec![];
        }

        let mut spans = vec![StyledSpan::keyword("for")];
        spans.extend(params);
        spans.push(StyledSpan::plain(" "));
        spans
    }

    /// Format a single generic parameter
    pub(super) fn format_generic_param(
        &mut self,
        item: DocRef<'a, Item>,
        param: &'a GenericParamDef,
    ) -> Vec<StyledSpan<'a>> {
        match &param.kind {
            GenericParamDefKind::Lifetime { outlives } => {
                let mut spans = vec![StyledSpan::lifetime(&param.name)];
                if !outlives.is_empty() {
                    spans.push(StyledSpan::punctuation(":"));
                    spans.push(StyledSpan::plain(" "));
                    for (i, lifetime) in outlives.iter().enumerate() {
                        if i > 0 {
                            spans.push(StyledSpan::plain(" + "));
                        }
                        spans.push(StyledSpan::lifetime(lifetime));
                    }
                }
                spans
            }
            GenericParamDefKind::Type {
                bounds, default, ..
            } => {
                let mut spans = vec![StyledSpan::generic(&param.name)];
                if !bounds.is_empty() {
                    spans.push(StyledSpan::punctuation(":"));
                    spans.push(StyledSpan::plain(" "));
                    spans.extend(self.format_generic_bounds(item, bounds));
                }
                if let Some(default_type) = default {
                    spans.push(StyledSpan::plain(" "));
                    spans.push(StyledSpan::operator("="));
                    spans.push(StyledSpan::plain(" "));
                    spans.extend(self.format_type(item, default_type));
                }
                spans
            }
            GenericParamDefKind::Const { type_, default } => {
                let mut spans = vec![
                    StyledSpan::keyword("const"),
                    StyledSpan::plain(" "),
                    StyledSpan::plain(&param.name),
                    StyledSpan::punctuation(":"),
                    StyledSpan::plain(" "),
                ];
                spans.extend(self.format_type(item, type_));
                if let Some(default_val) = default {
                    spans.push(StyledSpan::plain(" "));
                    spans.push(StyledSpan::operator("="));
                    spans.push(StyledSpan::plain(" "));
                    spans.push(StyledSpan::plain(default_val));
                }
                spans
            }
        }
    }

    /// Format generic bounds
    pub(super) fn format_generic_bounds(
        &mut self,
        item: DocRef<'a, Item>,
        bounds: &'a [GenericBound],
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = vec![];
        for (i, bound) in bounds.iter().enumerate() {
            if i > 0 {
                spans.push(StyledSpan::plain(" + "));
            }
            spans.extend(self.format_generic_bound(item, bound));
        }
        spans
    }

    /// Format a single generic bound
    pub(super) fn format_generic_bound(
        &mut self,
        item: DocRef<'a, Item>,
        bound: &'a GenericBound,
    ) -> Vec<StyledSpan<'a>> {
        match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => {
                let mut spans = self.format_hrtb(item, generic_params);

                match modifier {
                    TraitBoundModifier::None => {}
                    TraitBoundModifier::Maybe => spans.push(StyledSpan::operator("?")),
                    TraitBoundModifier::MaybeConst => {
                        spans.push(StyledSpan::operator("~const"));
                        spans.push(StyledSpan::plain(" "));
                    }
                }

                spans.extend(self.format_path(item, trait_));
                spans
            }
            GenericBound::Outlives(lifetime) => vec![StyledSpan::lifetime(lifetime)],
            GenericBound::Use(args) => {
                let mut spans = vec![StyledSpan::keyword("use"), StyledSpan::punctuation("<")];
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        spans.push(StyledSpan::punctuation(","));
                        spans.push(StyledSpan::plain(" "));
                    }
                    spans.push(match arg {
                        PreciseCapturingArg::Lifetime(lifetime) => StyledSpan::lifetime(lifetime),
                        PreciseCapturingArg::Param(param) => StyledSpan::generic(param),
                    });
                }
                spans.push(StyledSpan::punctuation(">"));
                spans
            }
        }
    }

    /// Format a `where` clause: inline after the signature for a single
    /// predicate, and one indented predicate per line for several.
    ///
    /// The multi-predicate form ends with a trailing comma and newline, so
    /// whatever follows — an item body's `{`, a signature's end — starts on its
    /// own line, the way rustfmt writes it. Callers that append a brace should
    /// go through [`push_body_brace`](super::push_body_brace) rather than
    /// unconditionally pushing a space first.
    pub(super) fn format_where_clause(
        &mut self,
        item: DocRef<'a, Item>,
        predicates: &'a [WherePredicate],
    ) -> Vec<StyledSpan<'a>> {
        if predicates.is_empty() {
            return vec![];
        }

        if predicates.len() == 1 {
            let mut spans = vec![
                StyledSpan::plain(" "),
                StyledSpan::keyword("where"),
                StyledSpan::plain(" "),
            ];
            spans.extend(self.format_where_predicate(item, &predicates[0]));
            return spans;
        }

        let mut spans = vec![
            StyledSpan::plain("\n"),
            StyledSpan::keyword("where"),
            StyledSpan::plain("\n    "),
        ];

        for (i, pred) in predicates.iter().enumerate() {
            if i > 0 {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain("\n    "));
            }
            spans.extend(self.format_where_predicate(item, pred));
        }

        spans.push(StyledSpan::punctuation(","));
        spans.push(StyledSpan::plain("\n"));
        spans
    }

    /// Format a where predicate
    pub(super) fn format_where_predicate(
        &mut self,
        item: DocRef<'a, Item>,
        predicate: &'a WherePredicate,
    ) -> Vec<StyledSpan<'a>> {
        match predicate {
            WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } => self.format_bound_predicate(item, type_, bounds, generic_params),
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                let mut spans = vec![StyledSpan::lifetime(lifetime), StyledSpan::punctuation(":")];
                if !outlives.is_empty() {
                    spans.push(StyledSpan::plain(" "));
                    for (i, lt) in outlives.iter().enumerate() {
                        if i > 0 {
                            spans.push(StyledSpan::plain(" + "));
                        }
                        spans.push(StyledSpan::lifetime(lt));
                    }
                }
                spans
            }
            WherePredicate::EqPredicate { lhs, rhs } => {
                let mut spans = vec![];
                spans.extend(self.format_type(item, lhs));
                spans.push(StyledSpan::plain(" "));
                spans.push(StyledSpan::operator("="));
                spans.push(StyledSpan::plain(" "));
                spans.extend(self.format_term(item, rhs));
                spans
            }
        }
    }

    fn format_bound_predicate(
        &mut self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        bounds: &'a [GenericBound],
        generic_params: &'a [GenericParamDef],
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = self.format_hrtb(item, generic_params);

        spans.extend(self.format_type(item, type_));
        spans.push(StyledSpan::punctuation(":"));
        spans.push(StyledSpan::plain(" "));
        spans.extend(self.format_generic_bounds(item, bounds));
        spans
    }

    /// Format a term (for associated type equality)
    pub(super) fn format_term(
        &mut self,
        item: DocRef<'a, Item>,
        term: &'a Term,
    ) -> Vec<StyledSpan<'a>> {
        match term {
            Term::Type(type_) => self.format_type(item, type_),
            Term::Constant(const_) => vec![StyledSpan::plain(const_.expr.clone())],
        }
    }

    /// Format a path
    pub(super) fn format_path(
        &mut self,
        item: DocRef<'a, Item>,
        path: &'a Path,
    ) -> Vec<StyledSpan<'a>> {
        let display_name = super::display_path_name(path);
        if display_name.is_empty() {
            return vec![];
        }

        let type_span =
            StyledSpan::type_name(display_name).with_target(self.get_path(item, path.id));

        let mut spans = vec![type_span];
        if let Some(args) = &path.args {
            spans.extend(self.format_generic_args(item, args));
        }
        spans
    }

    /// Format generic arguments
    pub(super) fn format_generic_args(
        &mut self,
        item: DocRef<'a, Item>,
        args: &'a GenericArgs,
    ) -> Vec<StyledSpan<'a>> {
        match args {
            GenericArgs::AngleBracketed { args, constraints } => {
                self.format_generic_angle_bracket(item, args, constraints)
            }
            GenericArgs::Parenthesized { inputs, output } => {
                self.format_generic_parenthesized(item, inputs, output)
            }
            GenericArgs::ReturnTypeNotation => vec![StyledSpan::plain("(..)")],
        }
    }

    fn format_generic_parenthesized(
        &mut self,
        item: DocRef<'a, Item>,
        inputs: &'a [Type],
        output: &'a Option<Type>,
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = vec![];

        spans.push(StyledSpan::punctuation("("));
        for (i, t) in inputs.iter().enumerate() {
            if i > 0 {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            spans.extend(self.format_type(item, t));
        }
        spans.push(StyledSpan::punctuation(")"));

        if let Some(out) = output {
            spans.push(StyledSpan::plain(" "));
            spans.push(StyledSpan::operator("->"));
            spans.push(StyledSpan::plain(" "));
            spans.extend(self.format_type(item, out));
        }

        spans
    }

    fn format_generic_angle_bracket(
        &mut self,
        item: DocRef<'a, Item>,
        args: &'a [GenericArg],
        constraints: &'a [AssocItemConstraint],
    ) -> Vec<StyledSpan<'a>> {
        if args.is_empty() && constraints.is_empty() {
            return vec![];
        }

        let mut spans = vec![StyledSpan::punctuation("<")];
        let mut first = true;

        for arg in args {
            if !first {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            first = false;

            match arg {
                GenericArg::Lifetime(lifetime) => spans.push(StyledSpan::lifetime(lifetime)),
                GenericArg::Type(type_) => spans.extend(self.format_type(item, type_)),
                GenericArg::Const(const_) => spans.push(StyledSpan::inline_code(&const_.expr)),
                GenericArg::Infer => spans.push(StyledSpan::plain("_")),
            }
        }

        for constraint in constraints {
            if !first {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            first = false;

            // Format constraints with proper spans
            spans.push(StyledSpan::plain(&constraint.name));
            match &constraint.binding {
                AssocItemConstraintKind::Equality(term) => {
                    spans.push(StyledSpan::plain(" "));
                    spans.push(StyledSpan::operator("="));
                    spans.push(StyledSpan::plain(" "));
                    spans.extend(self.format_term(item, term));
                }
                AssocItemConstraintKind::Constraint(bounds) => {
                    spans.push(StyledSpan::punctuation(":"));
                    spans.push(StyledSpan::plain(" "));
                    spans.extend(self.format_generic_bounds(item, bounds));
                }
            };
        }

        spans.push(StyledSpan::punctuation(">"));
        spans
    }
}

/// Lower a [`FunctionDoc`] to presentation nodes, reproducing the old
/// `format_function` output: the signature as a single generated-code block.
pub(super) fn lower_function(model: FunctionDoc<'_>) -> Vec<DocumentNode<'_>> {
    vec![DocumentNode::generated_code(model.signature)]
}
