use super::sigil_for_kind;
use ferritin_common::{DocRef, RustdocData, doc_ref::Path};
use rustdoc_types::{Item, ItemKind};

/// Where a crate's rendered documentation lives — everything a URL needs besides
/// the item itself.
///
/// The two names are not interchangeable. `crate_name` is the package name, which
/// is what docs.rs qualifies with a version; `lib_name` is the Rust identifier,
/// which is the directory rustdoc roots its output at. Emitting the package name
/// in the library position yields a 404 for any crate where they differ
/// (`docs.rs/futures-io/0.3.31/futures-io/…`).
struct DocRoot<'a> {
    crate_name: &'a str,
    lib_name: &'a str,
    version: &'a str,
    is_std: bool,
}

impl<'a> DocRoot<'a> {
    fn of(docs: &'a RustdocData) -> Self {
        Self {
            crate_name: docs.name(),
            lib_name: docs.lib_name(),
            version: docs.crate_version().unwrap_or("latest"),
            is_std: docs.provenance().is_std(),
        }
    }

    /// The URL prefix up to, but not including, the library directory.
    fn base(&self) -> String {
        if self.is_std {
            String::from("https://doc.rust-lang.org/nightly")
        } else {
            let Self {
                crate_name,
                version,
                ..
            } = self;
            format!("https://docs.rs/{crate_name}/{version}")
        }
    }

    /// Last-resort URL when we can't place an item precisely: the crate root page.
    fn fallback_url(&self) -> String {
        let Self { lib_name, .. } = self;
        format!("{}/{lib_name}/", self.base())
    }
}

/// The URL prefix a crate's documentation is served under, up to but not including
/// its library directory. Everything this crate's pages live beneath.
pub(crate) fn crate_base_url(docs: &RustdocData) -> String {
    DocRoot::of(docs).base()
}

pub(crate) fn generate_docsrs_url(item: DocRef<'_, Item>) -> String {
    let root = DocRoot::of(item.crate_docs());

    // Check if this item has its own page (has a path in the paths map)
    if let Some(path) = item.path() {
        generate_url_for_item_with_path(&root, &path, &item)
    } else {
        // This is an associated item or variant - need to find parent and generate fragment URL
        generate_url_for_associated_item(item, &root)
    }
}

fn generate_url_for_item_with_path(
    root: &DocRoot<'_>,
    path: &Path<'_>,
    item: &DocRef<'_, Item>,
) -> String {
    let segments = path.to_string();
    // `parts[0]` is the library name, which `root` already carries.
    let parts: Vec<&str> = segments.split("::").collect();

    let item_name = item.name().unwrap_or("unknown");
    let kind = item.kind();

    let base = root.base();
    let lib_name = root.lib_name;

    // For modules, the full path (after the library name) forms the module path
    // For other items, the last part is the item name, everything before is the module path
    match kind {
        ItemKind::Module => {
            // Module: full path after the library name is the module path
            // e.g., tokio::net -> tokio/net/index.html
            if parts.len() <= 1 {
                // Just the crate root
                format!("{base}/{lib_name}/index.html")
            } else {
                let module_path = parts[1..].join("/");
                format!("{base}/{lib_name}/{module_path}/index.html")
            }
        }

        // Primitives are documented at the crate root regardless of where the
        // path places them.
        ItemKind::Primitive => format!("{base}/{lib_name}/primitive.{item_name}.html"),

        _ => {
            // For non-module items, split the path into module path and item name
            // e.g., tokio::net::TcpStream -> module_path="tokio/net", item_name="TcpStream"
            let module_path = if parts.len() > 2 {
                parts[1..parts.len() - 1].join("/")
            } else {
                String::new()
            };

            let path_prefix = if module_path.is_empty() {
                lib_name.to_string()
            } else {
                format!("{lib_name}/{module_path}")
            };

            match sigil_for_kind(kind) {
                Some(sigil) => format!("{base}/{path_prefix}/{sigil}.{item_name}.html"),
                // A kind rustdoc gives no page of its own.
                None => root.fallback_url(),
            }
        }
    }
}

fn generate_url_for_associated_item(item: DocRef<'_, Item>, root: &DocRoot<'_>) -> String {
    let item_name = item.name().unwrap_or("unknown");
    let kind = item.kind();

    // Only these kinds actually live *inside* another item, so only they can
    // have a parent to hang a fragment URL on. Everything else — notably
    // modules that lack a `paths` entry, like the compiler's per-integer
    // `core::i32` support modules — short-circuits.
    if !matches!(
        kind,
        ItemKind::Function
            | ItemKind::AssocConst
            | ItemKind::AssocType
            | ItemKind::Variant
            | ItemKind::StructField
    ) {
        // A path-less module still has a derivable page: `{base}/{lib}/{name}/`.
        if kind == ItemKind::Module {
            let base = root.base();
            let lib_name = root.lib_name;
            return format!("{base}/{lib_name}/{item_name}/index.html");
        }
        return root.fallback_url();
    }

    // The parent comes from traversal context when available, otherwise from
    // the crate's derived parent index — never from an index scan. Blanket-impl
    // members have neither (their shared items can't be attributed to one
    // implementor) and take the fallback, as they always have.
    if let Some(parent) = item.parent_item() {
        let parent_url = generate_docsrs_url(parent);
        let fragment = match kind {
            ItemKind::Function => format!("#method.{item_name}"),
            ItemKind::AssocConst => format!("#associatedconstant.{item_name}"),
            ItemKind::AssocType => format!("#associatedtype.{item_name}"),
            ItemKind::Variant => format!("#variant.{item_name}"),
            ItemKind::StructField => format!("#structfield.{item_name}"),
            _ => String::new(),
        };
        return format!("{parent_url}{fragment}");
    }

    root.fallback_url()
}
