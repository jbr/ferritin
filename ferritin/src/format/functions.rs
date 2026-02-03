use rustdoc_types::{AssocItemConstraint, AssocItemConstraintKind, TraitBoundModifier};

use super::*;
use crate::styled_string::{DocumentNode, Span as StyledSpan};

impl Request {
    /// Format a function signature
    pub(super) fn format_function<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        function: DocRef<'a, Function>,
    ) -> Vec<DocumentNode<'a>> {
        let name = item.name().unwrap_or("<unnamed>");
        let signature_spans = self.format_function_signature(item, name, function.item());
        vec![DocumentNode::generated_code(signature_spans)]
    }

    /// Format a function signature
    pub(super) fn format_function_signature<'a>(
        &self,
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
        if !func.generics.params.is_empty() {
            spans.extend(self.format_generics(item, &func.generics));
        }
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
    pub(super) fn format_parameter<'a>(
        &self,
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

    /// Format generics for signatures
    pub(super) fn format_generics<'a>(
        &self,
        item: DocRef<'a, Item>,
        generics: &'a Generics,
    ) -> Vec<StyledSpan<'a>> {
        if generics.params.is_empty() {
            return vec![];
        }

        let mut spans = vec![StyledSpan::punctuation("<")];

        for (i, param) in generics.params.iter().enumerate() {
            if i > 0 {
                spans.push(StyledSpan::punctuation(","));
                spans.push(StyledSpan::plain(" "));
            }
            spans.extend(self.format_generic_param(item, param));
        }

        spans.push(StyledSpan::punctuation(">"));
        spans
    }

    /// Format a single generic parameter
    pub(super) fn format_generic_param<'a>(
        &self,
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
    pub(super) fn format_generic_bounds<'a>(
        &self,
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
    pub(super) fn format_generic_bound<'a>(
        &self,
        item: DocRef<'a, Item>,
        bound: &'a GenericBound,
    ) -> Vec<StyledSpan<'a>> {
        match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => {
                let mut spans = vec![];

                if !generic_params.is_empty() {
                    spans.push(StyledSpan::keyword("for"));
                    spans.push(StyledSpan::punctuation("<"));
                    for (i, p) in generic_params.iter().enumerate() {
                        if i > 0 {
                            spans.push(StyledSpan::punctuation(","));
                            spans.push(StyledSpan::plain(" "));
                        }
                        spans.extend(self.format_generic_param(item, p));
                    }
                    spans.push(StyledSpan::punctuation(">"));
                    spans.push(StyledSpan::plain(" "));
                }

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
            GenericBound::Use(_) => vec![StyledSpan::plain("use<...>")], // Handle new bound type
        }
    }

    /// Format where clause
    pub(super) fn format_where_clause<'a>(
        &self,
        item: DocRef<'a, Item>,
        predicates: &'a [WherePredicate],
    ) -> Vec<StyledSpan<'a>> {
        if predicates.is_empty() {
            return vec![];
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

        spans
    }

    /// Format a where predicate
    pub(super) fn format_where_predicate<'a>(
        &self,
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

    fn format_bound_predicate<'a>(
        &self,
        item: DocRef<'a, Item>,
        type_: &'a Type,
        bounds: &'a [GenericBound],
        generic_params: &'a [GenericParamDef],
    ) -> Vec<StyledSpan<'a>> {
        let mut spans = vec![];

        if !generic_params.is_empty() {
            spans.push(StyledSpan::keyword("for"));
            spans.push(StyledSpan::punctuation("<"));
            for (i, p) in generic_params.iter().enumerate() {
                if i > 0 {
                    spans.push(StyledSpan::punctuation(","));
                    spans.push(StyledSpan::plain(" "));
                }
                spans.extend(self.format_generic_param(item, p));
            }
            spans.push(StyledSpan::punctuation(">"));
            spans.push(StyledSpan::plain(" "));
        }

        spans.extend(self.format_type(item, type_));
        spans.push(StyledSpan::punctuation(":"));
        spans.push(StyledSpan::plain(" "));
        spans.extend(self.format_generic_bounds(item, bounds));
        spans
    }

    /// Format a term (for associated type equality)
    pub(super) fn format_term<'a>(
        &self,
        item: DocRef<'a, Item>,
        term: &'a Term,
    ) -> Vec<StyledSpan<'a>> {
        match term {
            Term::Type(type_) => self.format_type(item, type_),
            Term::Constant(const_) => vec![StyledSpan::plain(const_.expr.clone())],
        }
    }

    /// Format a path
    pub(super) fn format_path<'a>(
        &self,
        item: DocRef<'a, Item>,
        path: &'a Path,
    ) -> Vec<StyledSpan<'a>> {
        if path.path.is_empty() {
            return vec![];
        }

        let type_span = StyledSpan::type_name(&path.path).with_target(item.get_path(path.id));

        let mut spans = vec![type_span];
        if let Some(args) = &path.args {
            spans.extend(self.format_generic_args(item, args));
        }
        spans
    }

    /// Format generic arguments
    pub(super) fn format_generic_args<'a>(
        &self,
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

    fn format_generic_parenthesized<'a>(
        &self,
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

    fn format_generic_angle_bracket<'a>(
        &self,
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
