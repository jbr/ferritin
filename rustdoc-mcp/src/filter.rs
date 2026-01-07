use clap::ValueEnum;
use rustdoc_types::ItemKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

#[derive(
    Debug,
    Clone,
    Eq,
    Hash,
    PartialEq,
    ValueEnum,
    Serialize,
    Deserialize,
    JsonSchema,
    Copy,
    VariantArray,
)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    Struct,
    Enum,
    Trait,
    Function,
    Constant,
    Static,
    Module,
    Union,
    Macro,
    Type,
    Variant,
}

impl Filter {
    pub fn matches_kind(&self, kind: ItemKind) -> bool {
        match self {
            Filter::Struct => kind == ItemKind::Struct,
            Filter::Enum => kind == ItemKind::Enum,
            Filter::Trait => kind == ItemKind::Trait,
            Filter::Function => kind == ItemKind::Function,
            Filter::Constant => kind == ItemKind::Constant,
            Filter::Static => kind == ItemKind::Static,
            Filter::Module => kind == ItemKind::Module,
            Filter::Macro => matches!(
                kind,
                ItemKind::Macro | ItemKind::ProcAttribute | ItemKind::ProcDerive
            ),
            Filter::Type => kind == ItemKind::TypeAlias,
            Filter::Variant => kind == ItemKind::Variant,
            _ => true, // if we don't have a filter for it, it's always shown instead of always hidden
        }
    }
}
