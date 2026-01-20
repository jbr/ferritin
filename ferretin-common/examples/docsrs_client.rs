use anyhow::Result;
use ferretin_common::docsrs_client::DocsRsClient;
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
        (name, Some(ver))
    } else {
        (input.as_str(), None)
    };

    println!("Fetching docs for: {}", crate_name);
    if let Some(v) = version {
        println!("Version: {}", v);
    } else {
        println!("Version: latest");
    }
    println!();

    // Create client with temp cache dir
    let cache_dir = env::temp_dir().join("rustdoc-cache-example");
    let client = DocsRsClient::new(cache_dir)?;

    // Fetch the crate (this blocks on async, so we need a runtime)
    let result = trillium_smol::async_global_executor::block_on(async {
        client.get_crate(crate_name, version).await
    })?;

    match result {
        Some(data) => {
            println!("✓ Successfully fetched rustdoc data!");
            println!();
            println!("Crate: {}", data.name());
            println!(
                "Version: {}",
                data.crate_version.as_deref().unwrap_or("unknown")
            );
            println!("Items in index: {}", data.index.len());
            println!("External crates: {}", data.external_crates.len());
            println!("Cache path: {}", data.fs_path().display());

            // Print the root module name
            if let Some(root_item) = data.index.get(&data.root) {
                println!(
                    "Root module: {}",
                    root_item.name.as_deref().unwrap_or("(unnamed)")
                );
            }
        }
        None => {
            println!("✗ Crate not found on docs.rs");
            std::process::exit(1);
        }
    }

    Ok(())
}
