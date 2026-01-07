use crate::state::RustdocTools;
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
