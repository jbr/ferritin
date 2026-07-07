use ferritin_common::{DocRef, doc_ref::Path};
use rustdoc_types::Item;

pub(crate) fn generate_docsrs_url(item: DocRef<'_, Item>) -> String {
    let docs = item.crate_docs();
    let crate_name = docs.name();
    let version = docs.crate_version().unwrap_or("latest");
    let is_std = docs.provenance().is_std();

    // Check if this item has its own page (has a path in the paths map)
    if let Some(path) = item.path() {
        generate_url_for_item_with_path(crate_name, version, is_std, &path, &item)
    } else {
        // This is an associated item or variant - need to find parent and generate fragment URL
        generate_url_for_associated_item(item, crate_name, version, is_std)
    }
}

fn generate_url_for_item_with_path(
    crate_name: &str,
    version: &str,
    is_std: bool,
    path: &Path<'_>,
    item: &DocRef<'_, Item>,
) -> String {
    let segments = path.to_string();
    let parts: Vec<&str> = segments.split("::").collect();

    let item_name = item.name().unwrap_or("unknown");
    let kind = item.kind();

    let base = if is_std {
        String::from("http://docs.rust-lang.org/nightly")
    } else {
        format!("https://docs.rs/{crate_name}/{version}",)
    };

    // For modules, the full path (after crate name) forms the module path
    // For other items, the last part is the item name, everything before is the module path
    match kind {
        rustdoc_types::ItemKind::Module => {
            // Module: full path after crate name is the module path
            // e.g., tokio::net -> tokio/net/index.html
            if parts.len() <= 1 {
                // Just the crate root
                format!("{}/{}/index.html", base, crate_name)
            } else {
                // parts[1..] are all part of the module path
                let module_path = parts[1..].join("/");
                format!("{}/{}/{}/index.html", base, crate_name, module_path)
            }
        }
        _ => {
            // For non-module items, split the path into module path and item name
            // e.g., tokio::net::TcpStream -> module_path="tokio/net", item_name="TcpStream"
            let module_path = if parts.len() > 2 {
                parts[1..parts.len() - 1].join("/")
            } else {
                String::new()
            };

            let path_prefix = if module_path.is_empty() {
                crate_name.to_string()
            } else {
                format!("{}/{}", crate_name, module_path)
            };

            match kind {
                rustdoc_types::ItemKind::Struct => {
                    format!("{}/{}/struct.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Enum => {
                    format!("{}/{}/enum.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Trait => {
                    format!("{}/{}/trait.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Function => {
                    format!("{}/{}/fn.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::TypeAlias => {
                    format!("{}/{}/type.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Constant => {
                    format!("{}/{}/constant.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Static => {
                    format!("{}/{}/static.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Union => {
                    format!("{}/{}/union.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Macro
                | rustdoc_types::ItemKind::ProcAttribute
                | rustdoc_types::ItemKind::ProcDerive => {
                    format!("{}/{}/macro.{}.html", base, path_prefix, item_name)
                }
                rustdoc_types::ItemKind::Primitive => {
                    format!("{}/{}/primitive.{}.html", base, crate_name, item_name)
                }
                _ => {
                    // Fallback for unknown kinds
                    format!("{}/{}/", base, crate_name)
                }
            }
        }
    }
}

fn generate_url_for_associated_item(
    item: DocRef<'_, Item>,
    crate_name: &str,
    version: &str,
    is_std: bool,
) -> String {
    let item_name = item.name().unwrap_or("unknown");
    let kind = item.kind();

    // Only these kinds actually live *inside* another item, so only they can
    // have a parent to hang a fragment URL on. Everything else — notably
    // modules that lack a `paths` entry, like the compiler's per-integer
    // `core::i32` support modules — short-circuits.
    use rustdoc_types::ItemKind;
    if !matches!(
        kind,
        ItemKind::Function
            | ItemKind::AssocConst
            | ItemKind::AssocType
            | ItemKind::Variant
            | ItemKind::StructField
    ) {
        // A path-less module still has a derivable page: `{base}/{crate}/{name}/`.
        if kind == ItemKind::Module {
            let base = if is_std {
                String::from("http://docs.rust-lang.org/nightly")
            } else {
                format!("https://docs.rs/{crate_name}/{version}")
            };
            return format!("{base}/{crate_name}/{item_name}/index.html");
        }
        return fallback_url(crate_name, version, is_std);
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

    fallback_url(crate_name, version, is_std)
}

/// Last-resort URL when we can't place an item precisely: the crate root page.
fn fallback_url(crate_name: &str, version: &str, is_std: bool) -> String {
    if is_std {
        format!("https://doc.rust-lang.org/nightly/{crate_name}/")
    } else {
        format!("https://docs.rs/{crate_name}/{version}/{crate_name}/")
    }
}
