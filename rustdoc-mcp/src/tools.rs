use crate::state::RustdocTools;
use mcplease::types::ToolAnnotations;

/// The behavior hints shared by every tool here that only reads.
///
/// The spec's defaults are pessimistic — an undeclared tool is presumed
/// destructive and open-world — so declaring these is worthwhile.
/// `open_world_hint` is false: these read one resolved local workspace and its
/// dependencies, not an unbounded external corpus.
pub(crate) const READ_ONLY_LOCAL: ToolAnnotations = ToolAnnotations {
    title: None,
    read_only_hint: Some(true),
    destructive_hint: Some(false),
    idempotent_hint: Some(true),
    open_world_hint: Some(false),
};

mcplease::tools!(
    RustdocTools,
    (
        SetWorkingDirectory,
        set_working_directory,
        "set_working_directory"
    ),
    (GetItem, get_item, "get_item"),
    (ListCrates, list_crates, "list_crates"),
    (Search, search, "search")
);
