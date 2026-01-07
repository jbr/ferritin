use super::*;

impl Request {
    /// Enhanced type formatting for signatures
    pub(crate) fn format_type(&self, type_: &Type) -> String {
        match type_ {
            Type::ResolvedPath(path) => self.format_path(path),
            Type::DynTrait(dyn_trait) => {
                let traits: Vec<String> = dyn_trait
                    .traits
                    .iter()
                    .map(|t| self.format_path(&t.trait_))
                    .collect();
                format!("dyn {}", traits.join(" + "))
            }
            Type::Generic(name) => name.clone(),
            Type::Primitive(prim) => prim.clone(),
            Type::Array { type_, len } => {
                format!("[{}; {}]", self.format_type(type_), len)
            }
            Type::Slice(type_) => format!("[{}]", self.format_type(type_)),
            Type::BorrowedRef {
                lifetime,
                is_mutable,
                type_,
                ..
            } => {
                let mut result = String::from("&");
                if let Some(lt) = lifetime {
                    result.push_str(&format!("{lt} "));
                }
                if *is_mutable {
                    result.push_str("mut ");
                }
                result.push_str(&self.format_type(type_));
                result
            }
            Type::RawPointer { is_mutable, type_ } => {
                format!(
                    "*{} {}",
                    if *is_mutable { "mut" } else { "const" },
                    self.format_type(type_)
                )
            }
            Type::FunctionPointer(fp) => self.format_function_pointer(fp),
            Type::Tuple(types) => self.format_tuple(types),
            Type::ImplTrait(bounds) => {
                format!("impl {}", self.format_generic_bounds(bounds))
            }
            Type::Infer => "_".to_string(),
            Type::QualifiedPath {
                name,
                args,
                self_type,
                trait_,
            } => self.format_qualified_path(name, args.as_deref(), self_type, trait_),
            Type::Pat { .. } => "pattern".to_string(), // Handle pattern types
        }
    }

    pub(crate) fn format_tuple(&self, types: &[Type]) -> String {
        format!(
            "({})",
            types
                .iter()
                .map(|t| self.format_type(t))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    pub(crate) fn format_function_pointer(&self, fp: &FunctionPointer) -> String {
        let mut result = String::new();
        if !fp.generic_params.is_empty() {
            result.push_str("for<");
            result.push_str(
                &fp.generic_params
                    .iter()
                    .map(|p| self.format_generic_param(p))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            result.push_str("> ");
        }
        result.push_str("fn(");
        result.push_str(
            &fp.sig
                .inputs
                .iter()
                .map(|(_, t)| self.format_type(t))
                .collect::<Vec<_>>()
                .join(", "),
        );
        result.push(')');
        if let Some(output) = &fp.sig.output {
            result.push_str(" -> ");
            result.push_str(&self.format_type(output));
        }
        result
    }

    pub(crate) fn format_qualified_path(
        &self,
        name: &String,
        args: Option<&GenericArgs>,
        self_type: &Type,
        trait_: &Option<Path>,
    ) -> String {
        // For Self::AssociatedType, use simpler syntax when possible
        if matches!(self_type, Type::Generic(s) if s == "Self") {
            if let Some(trait_path) = trait_ {
                let trait_str = self.format_path(trait_path);
                if trait_str.is_empty() {
                    // If trait path is empty, just use Self::name
                    let mut result = format!("Self::{name}");
                    if let Some(args) = args {
                        result.push_str(&self.format_generic_args(args));
                    }
                    return result;
                } else {
                    // Use full qualified syntax: <Self as Trait>::name
                    let mut result = format!("<Self as {trait_str}>::{name}");
                    if let Some(args) = args {
                        result.push_str(&self.format_generic_args(args));
                    }
                    return result;
                }
            } else {
                // No trait specified, use Self::name
                let mut result = format!("Self::{name}");
                if let Some(args) = args {
                    result.push_str(&self.format_generic_args(args));
                }
                return result;
            }
        }
        // For other types, use full qualified syntax
        let mut result = format!("<{}", self.format_type(self_type));
        if let Some(trait_path) = trait_ {
            result.push_str(" as ");
            result.push_str(&self.format_path(trait_path));
        }
        result.push_str(">::");
        result.push_str(name);
        if let Some(args) = args {
            result.push_str(&self.format_generic_args(args));
        }
        result
    }
}
