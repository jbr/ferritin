use super::*;
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
/// The separator between an item's signature and the `{` that opens its body.
///
/// A `where` clause is laid out one predicate per line, so a brace following one
/// belongs on a fresh line rather than a space away from the last predicate.
pub(super) fn body_brace_separator(where_clause: &str) -> &'static str {
    if where_clause.is_empty() { " " } else { "\n" }
}

fn is_synthetic(param: &GenericParamDef) -> bool {
    matches!(
        param.kind,
        GenericParamDefKind::Type {
            is_synthetic: true,
            ..
        }
    )
}

impl<'a> Request<'a> {
    /// Format a function signature
    pub(super) fn format_function(
        &self,
        item: DocRef<'_, Item>,
        function: DocRef<'_, Function>,
        _context: &FormatContext,
    ) -> String {
        let name = item.name.as_deref().unwrap_or("<unnamed>");
        self.format_function_signature(name, &function)
    }

    /// Format a function signature
    pub(super) fn format_function_signature(&self, name: &str, func: &Function) -> String {
        let mut sig = String::new();

        // Add function modifiers in the correct order
        if func.header.is_const {
            sig.push_str("const ");
        }

        if func.header.is_async {
            sig.push_str("async ");
        }

        if func.header.is_unsafe {
            sig.push_str("unsafe ");
        }

        // Add ABI specification if not default Rust ABI
        match func.header.abi {
            Abi::Rust => {}
            Abi::C { unwind } => {
                if unwind {
                    sig.push_str("extern \"C-unwind\" ");
                } else {
                    sig.push_str("extern \"C\" ");
                }
            }
            Abi::Cdecl { unwind } => {
                if unwind {
                    sig.push_str("extern \"cdecl-unwind\" ");
                } else {
                    sig.push_str("extern \"cdecl\" ");
                }
            }
            Abi::Stdcall { unwind } => {
                if unwind {
                    sig.push_str("extern \"stdcall-unwind\" ");
                } else {
                    sig.push_str("extern \"stdcall\" ");
                }
            }
            Abi::Fastcall { unwind } => {
                if unwind {
                    sig.push_str("extern \"fastcall-unwind\" ");
                } else {
                    sig.push_str("extern \"fastcall\" ");
                }
            }
            Abi::Aapcs { unwind } => {
                if unwind {
                    sig.push_str("extern \"aapcs-unwind\" ");
                } else {
                    sig.push_str("extern \"aapcs\" ");
                }
            }
            Abi::Win64 { unwind } => {
                if unwind {
                    sig.push_str("extern \"win64-unwind\" ");
                } else {
                    sig.push_str("extern \"win64\" ");
                }
            }
            Abi::SysV64 { unwind } => {
                if unwind {
                    sig.push_str("extern \"sysv64-unwind\" ");
                } else {
                    sig.push_str("extern \"sysv64\" ");
                }
            }
            Abi::System { unwind } => {
                if unwind {
                    sig.push_str("extern \"system-unwind\" ");
                } else {
                    sig.push_str("extern \"system\" ");
                }
            }
            Abi::Other(ref abi_name) => {
                sig.push_str(&format!("extern \"{abi_name}\" "));
            }
        }

        // Add function name and generics
        sig.push_str("fn ");
        sig.push_str(name);
        if !func.generics.params.is_empty() {
            sig.push_str(&self.format_generics(&func.generics));
        }
        sig.push('(');

        // Add parameters
        let params: Vec<String> = func
            .sig
            .inputs
            .iter()
            .map(|(param_name, param_type)| self.format_parameter(param_name, param_type))
            .collect();
        sig.push_str(&params.join(", "));
        sig.push(')');

        // Add return type if not unit
        if let Some(output) = &func.sig.output {
            sig.push_str(&format!(" -> {}", self.format_type(output)));
        }

        // Add where clause if present
        if !func.generics.where_predicates.is_empty() {
            sig.push_str(&self.format_where_clause(&func.generics.where_predicates));
        }

        sig
    }

    /// Format a function parameter with idiomatic self shorthand
    pub(super) fn format_parameter(&self, param_name: &str, param_type: &Type) -> String {
        // Handle self parameters with idiomatic shorthand
        if param_name == "self" {
            match param_type {
                // self: Self -> self
                Type::Generic(name) if name == "Self" => "self".to_string(),
                // self: &Self -> &self
                Type::BorrowedRef {
                    lifetime: None,
                    is_mutable: false,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    "&self".to_string()
                }
                // self: &mut Self -> &mut self
                Type::BorrowedRef {
                    lifetime: None,
                    is_mutable: true,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    "&mut self".to_string()
                }
                // self: &'a Self -> &'a self
                Type::BorrowedRef {
                    lifetime: Some(lifetime),
                    is_mutable: false,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    format!("&{lifetime} self")
                }
                // self: &'a mut Self -> &'a mut self
                Type::BorrowedRef {
                    lifetime: Some(lifetime),
                    is_mutable: true,
                    type_,
                    ..
                } if matches!(type_.as_ref(), Type::Generic(name) if name == "Self") => {
                    format!("&{lifetime} mut self")
                }
                // For any other self type, use the full form
                _ => format!("{param_name}: {}", self.format_type(param_type)),
            }
        } else {
            // For non-self parameters, use the standard format
            format!("{param_name}: {}", self.format_type(param_type))
        }
    }

    /// Format a generic parameter list (`<T, 'a, const N: usize>`), eliding the
    /// parameters rustdoc synthesized for `impl Trait` arguments (see
    /// [`is_synthetic`]). Empty — not `<>` — when everything is elided.
    pub(super) fn format_generics(&self, generics: &Generics) -> String {
        self.format_generic_param_list(&generics.params)
    }

    /// The shared `<..>` rendering behind [`format_generics`] and every
    /// higher-ranked `for<..>` binder.
    fn format_generic_param_list(&self, params: &[GenericParamDef]) -> String {
        let params: Vec<String> = params
            .iter()
            .filter(|param| !is_synthetic(param))
            .map(|param| self.format_generic_param(param))
            .collect();

        if params.is_empty() {
            String::new()
        } else {
            format!("<{}>", params.join(", "))
        }
    }

    /// Format a higher-ranked binder — an HRTB's `for<'a>` — with a trailing
    /// space, or nothing when it is empty. Rustdoc records these in a
    /// `generic_params` list *beside* the thing they bind (a trait bound, a `dyn`
    /// object's trait, a `where` predicate, a function pointer), so each of those
    /// has to reassemble the `for<..>` itself.
    pub(super) fn format_hrtb(&self, generic_params: &[GenericParamDef]) -> String {
        match self.format_generic_param_list(generic_params) {
            params if params.is_empty() => params,
            params => format!("for{params} "),
        }
    }

    /// Format a single generic parameter
    pub(super) fn format_generic_param(&self, param: &GenericParamDef) -> String {
        match &param.kind {
            GenericParamDefKind::Lifetime { outlives } => {
                let mut result = param.name.clone();
                if !outlives.is_empty() {
                    result.push_str(": ");
                    result.push_str(&outlives.join(" + "));
                }
                result
            }
            GenericParamDefKind::Type {
                bounds, default, ..
            } => {
                let mut result = param.name.clone();
                if !bounds.is_empty() {
                    result.push_str(": ");
                    result.push_str(&self.format_generic_bounds(bounds));
                }
                if let Some(default_type) = default {
                    result.push_str(" = ");
                    result.push_str(&self.format_type(default_type));
                }
                result
            }
            GenericParamDefKind::Const { type_, default } => {
                let mut result = format!("const {}: {}", param.name, self.format_type(type_));
                if let Some(default_val) = default {
                    result.push_str(" = ");
                    result.push_str(default_val);
                }
                result
            }
        }
    }

    /// Format generic bounds
    pub(super) fn format_generic_bounds(&self, bounds: &[GenericBound]) -> String {
        bounds
            .iter()
            .map(|bound| self.format_generic_bound(bound))
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// Format a single generic bound
    pub(super) fn format_generic_bound(&self, bound: &GenericBound) -> String {
        match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => {
                let mut result = self.format_hrtb(generic_params);

                match modifier {
                    TraitBoundModifier::None => {}
                    TraitBoundModifier::Maybe => result.push('?'),
                    TraitBoundModifier::MaybeConst => result.push_str("~const "),
                }

                result.push_str(&self.format_path(trait_));
                result
            }
            GenericBound::Outlives(lifetime) => lifetime.clone(),
            GenericBound::Use(args) => {
                let args: Vec<&str> = args
                    .iter()
                    .map(|arg| match arg {
                        PreciseCapturingArg::Lifetime(lifetime) => lifetime.as_str(),
                        PreciseCapturingArg::Param(param) => param.as_str(),
                    })
                    .collect();
                format!("use<{}>", args.join(", "))
            }
        }
    }

    /// Format a `where` clause, one indented predicate per line. Callers that
    /// follow it with an item body's `{` should separate the two with
    /// [`body_brace_separator`].
    pub(super) fn format_where_clause(&self, predicates: &[WherePredicate]) -> String {
        if predicates.is_empty() {
            return String::new();
        }

        let mut result = String::from("\nwhere\n    ");
        let formatted_predicates: Vec<String> = predicates
            .iter()
            .map(|pred| self.format_where_predicate(pred))
            .collect();
        result.push_str(&formatted_predicates.join(",\n    "));
        result
    }

    /// Format a where predicate
    pub(super) fn format_where_predicate(&self, predicate: &WherePredicate) -> String {
        match predicate {
            WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } => self.format_bound_predicate(type_, bounds, generic_params),
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                format!("{}: {}", lifetime, outlives.join(" + "))
            }
            WherePredicate::EqPredicate { lhs, rhs } => {
                format!("{} = {}", self.format_type(lhs), self.format_term(rhs))
            }
        }
    }

    fn format_bound_predicate(
        &self,
        type_: &Type,
        bounds: &[GenericBound],
        generic_params: &[GenericParamDef],
    ) -> String {
        let mut result = self.format_hrtb(generic_params);
        result.push_str(&self.format_type(type_));
        result.push_str(": ");
        result.push_str(&self.format_generic_bounds(bounds));
        result
    }

    /// Format a term (for associated type equality)
    pub(super) fn format_term(&self, term: &Term) -> String {
        match term {
            Term::Type(type_) => self.format_type(type_).to_string(),
            Term::Constant(const_) => const_.expr.clone(),
        }
    }

    /// Format a path
    pub(super) fn format_path(&self, path: &Path) -> String {
        let mut result = path.path.clone();
        if let Some(args) = &path.args {
            result.push_str(&self.format_generic_args(args));
        }
        result
    }

    /// Format generic arguments
    pub(super) fn format_generic_args(&self, args: &GenericArgs) -> String {
        match args {
            GenericArgs::AngleBracketed { args, constraints } => {
                self.format_generic_angle_bracket(args, constraints)
            }
            GenericArgs::Parenthesized { inputs, output } => {
                self.format_generic_parenthesized(inputs, output)
            }
            GenericArgs::ReturnTypeNotation => "(..)".to_string(),
        }
    }

    fn format_generic_parenthesized(&self, inputs: &[Type], output: &Option<Type>) -> String {
        let mut result = format!(
            "({})",
            inputs
                .iter()
                .map(|t| self.format_type(t))
                .collect::<Vec<_>>()
                .join(", ")
        );
        if let Some(out) = output {
            result.push_str(" -> ");
            result.push_str(&self.format_type(out));
        }
        result
    }

    fn format_generic_angle_bracket(
        &self,
        args: &Vec<GenericArg>,
        constraints: &[AssocItemConstraint],
    ) -> String {
        let mut parts = Vec::new();
        for arg in args {
            parts.push(self.format_generic_arg(arg));
        }
        for constraint in constraints {
            let constraint_str = match &constraint.binding {
                AssocItemConstraintKind::Equality(term) => {
                    format!("{} = {}", constraint.name, self.format_term(term))
                }
                AssocItemConstraintKind::Constraint(bounds) => {
                    format!(
                        "{}: {}",
                        constraint.name,
                        self.format_generic_bounds(bounds)
                    )
                }
            };
            parts.push(constraint_str);
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("<{}>", parts.join(", "))
        }
    }

    /// Format a generic argument
    pub(super) fn format_generic_arg(&self, arg: &GenericArg) -> String {
        match arg {
            GenericArg::Lifetime(lifetime) => lifetime.clone(),
            GenericArg::Type(type_) => self.format_type(type_).to_string(),
            GenericArg::Const(const_) => const_.expr.clone(),
            GenericArg::Infer => "_".to_string(),
        }
    }
}
