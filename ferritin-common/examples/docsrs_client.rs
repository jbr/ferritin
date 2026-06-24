use anyhow::Result;
use ferritin_common::sources::{DocsRsSource, Source};
use semver::VersionReq;
use std::env;

fn main() -> Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <crate_name[@version]>", args[0]);
        eprintln!("Examples:");
        eprintln!("  {} clap", args[0]);
        eprintln!("  {} clap@latest", args[0]);
        eprintln!("  {} serde@1.0.195", args[0]);
        std::process::exit(1);
    }

    let input = &args[1];
    let (crate_name, version) = if let Some((name, ver)) = input.split_once('@') {
        (name, VersionReq::parse(ver)?)
    } else {
        (input.as_str(), VersionReq::STAR)
    };

    println!("Resolving {crate_name} version {version}\n");

    // Create client with temp cache dir
    let cache_dir = env::temp_dir().join("rustdoc-cache-example");
    let source = DocsRsSource::new(cache_dir)?;

    match source.lookup(crate_name, &version) {
        Some(crate_info) => {
            println!(
                "Resolved {}@{}: {}\n",
                crate_info.name(),
                crate_info.version().unwrap(),
                crate_info
                    .description()
                    .unwrap_or_default()
                    .replace('\n', " ")
            );
            match source.load(crate_info.name(), crate_info.version()) {
                Some(data) => {
                    println!("✓ Successfully fetched rustdoc data!");
                    println!();
                    println!("Crate: {}", data.name());
                    println!("Version: {}", data.crate_version().unwrap_or("unknown"));
                    println!("Items in index: {}", data.all_items().len());
                    println!("External crates: {}", data.external_crates_iter().count());
                    println!("Cache path: {}", data.fs_path().display());

                    // Print the root module name
                    if let Some(root_item) = data.get_item(data.root_id()) {
                        println!(
                            "Root module: {}",
                            root_item.name.as_deref().unwrap_or("(unnamed)")
                        );
                    }
                }

                None => {
                    println!("✗ json not found on docs.rs");
                    std::process::exit(1);
                }
            }
        }
        None => {
            println!("✗ Crate not found on crates.io");
            std::process::exit(1);
        }
    }

    Ok(())
}
