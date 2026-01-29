use ferritin_common::{DocRef, doc_ref::Path};
use rustdoc_types::{Item, ItemEnum};

pub(crate) fn generate_docsrs_url(item: DocRef<'_, Item>) -> String {
    let docs = item.crate_docs();
    let crate_name = docs.name();
    let version = docs.crate_version.as_deref().unwrap_or("latest");
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
    let docs = item.crate_docs();
    let item_id = &item.id;
    let item_name = item.name().unwrap_or("unknown");
    let kind = item.kind();

    // Search through all impl blocks to find which one contains this item
    for impl_item in docs.index.values() {
        if let ItemEnum::Impl(impl_block) = &impl_item.inner
            && impl_block.items.contains(item_id)
        {
            // Found the parent impl
            if let rustdoc_types::Type::ResolvedPath(path) = &impl_block.for_
                && let Some(parent) = item.get(&path.id)
            {
                // Generate parent URL
                let parent_url = generate_docsrs_url(parent);

                // Generate fragment based on item kind
                let fragment = match kind {
                    rustdoc_types::ItemKind::Function => {
                        if impl_block.trait_.is_some() {
                            // Trait method
                            format!("#method.{}", item_name)
                        } else {
                            // Inherent method
                            format!("#method.{}", item_name)
                        }
                    }
                    rustdoc_types::ItemKind::AssocConst => {
                        format!("#associatedconstant.{}", item_name)
                    }
                    rustdoc_types::ItemKind::AssocType => {
                        format!("#associatedtype.{}", item_name)
                    }
                    _ => String::new(),
                };

                return format!("{}{}", parent_url, fragment);
            }
        }
    }

    // Check if this is an enum variant
    if matches!(kind, rustdoc_types::ItemKind::Variant) {
        // Find the parent enum
        for enum_item in docs.index.values() {
            if let ItemEnum::Enum(enum_data) = &enum_item.inner
                && enum_data.variants.contains(item_id)
            {
                let parent = item.build_ref(enum_item);
                let parent_url = generate_docsrs_url(parent);
                return format!("{}#variant.{}", parent_url, item_name);
            }
        }
    }

    // Check if this is a struct field
    if matches!(kind, rustdoc_types::ItemKind::StructField) {
        // Find the parent struct
        for struct_item in docs.index.values() {
            if let ItemEnum::Struct(struct_data) = &struct_item.inner
                && matches!(&struct_data.kind, rustdoc_types::StructKind::Plain { fields, .. } if fields.contains(item_id))
            {
                let parent = item.build_ref(struct_item);
                let parent_url = generate_docsrs_url(parent);
                return format!("{}#structfield.{}", parent_url, item_name);
            }
        }
    }

    // Fallback - couldn't determine parent
    if is_std {
        format!("https://doc.rust-lang.org/nightly/{}/", crate_name)
    } else {
        format!("https://docs.rs/{}/{}/{}/", crate_name, version, crate_name)
    }
}
