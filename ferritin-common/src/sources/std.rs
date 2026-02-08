use crate::CrateName;
use crate::RustdocData;
use crate::navigator::CrateInfo;
use crate::sources::CrateProvenance;
use crate::sources::Source;
use fieldwork::Fieldwork;
use rustc_hash::FxHashMap;
use rustdoc_types::{Crate, FORMAT_VERSION};
use semver::Version;
use semver::VersionReq;
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;

/// Descriptions for standard library crates
const STD_DESCRIPTIONS: [(&str, &str); 6] = [
    ("std", "The Rust Standard Library"),
    ("alloc", "The Rust core allocation and collections library"),
    ("core", "The Rust Core Library"),
    (
        "proc_macro",
        "A support library for macro authors when defining new macros",
    ),
    (
        "test",
        "Support code for rustc's built in unit-test and micro-benchmarking framework",
    ),
    ("std_detect", ""), // we claim to have a "std_detect" crate that we then fail to load
];

/// Source for std library documentation (rustup-managed)
#[derive(Debug, Clone, Fieldwork)]
#[field(get)]
pub struct StdSource {
    docs_path: PathBuf,
    rustc_version: Version,
    crates: FxHashMap<&'static str, CrateInfo>,
}

impl StdSource {
    /// Try to create a StdSource from the current rustup installation
    pub fn from_rustup() -> Option<Self> {
        let sysroot = Command::new("rustup")
            .args(["run", "nightly", "rustc", "--print", "sysroot"])
            .output()
            .ok()?;

        if !sysroot.status.success() {
            return None;
        }

        let s = std::str::from_utf8(&sysroot.stdout).ok()?;
        let docs_path = PathBuf::from(s.trim()).join("share/doc/rust/json/");
        if !docs_path.exists() {
            return None;
        }

        let version = Command::new("rustup")
            .args(["run", "nightly", "rustc", "--version", "--verbose"])
            .output()
            .ok()?;

        if !version.status.success() {
            return None;
        }

        let rustc_version = std::str::from_utf8(&version.stdout)
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("release: "))?
            .trim();

        let rustc_version = Version::parse(rustc_version).ok()?;

        let crates = STD_DESCRIPTIONS
            .into_iter()
            .map(|(name, description)| {
                (
                    name,
                    CrateInfo {
                        provenance: CrateProvenance::Std,
                        version: Some(rustc_version.clone()),
                        description: Some(description.to_string()),
                        name: name.to_string(),
                        default_crate: false,
                        used_by: vec![],
                        json_path: (name != "std_detect")
                            .then(|| docs_path.join(format!("{name}.json"))),
                    },
                )
            })
            .collect();

        Some(Self {
            docs_path,
            rustc_version,
            crates,
        })
    }
}

impl Source for StdSource {
    fn lookup<'a>(&'a self, name: &str, _version_req: &VersionReq) -> Option<Cow<'a, CrateInfo>> {
        let canonical = self.canonicalize(name)?;
        self.crates.get(&*canonical).map(Cow::Borrowed)
    }

    fn load(&self, crate_name: &str, _version: Option<&Version>) -> Option<RustdocData> {
        let crate_info = self.lookup(crate_name, &VersionReq::STAR)?;
        let json_path = crate_info.json_path.as_ref()?.to_owned();
        let content = std::fs::read(&json_path).ok()?;

        let Ok(FORMAT_VERSION) = sonic_rs::get_from_slice(&content, &["format_version"])
            .ok()?
            .as_raw_str()
            .parse()
        else {
            return None;
        };

        let crate_data: Crate = sonic_rs::serde::from_slice(&content).ok()?;
        Some(RustdocData {
            crate_data,
            name: crate_name.to_string(),
            provenance: CrateProvenance::Std,
            fs_path: json_path,
            version: Some(self.rustc_version.clone()),
        })
    }

    fn list_available<'a>(&'a self) -> Box<dyn Iterator<Item = &'a CrateInfo> + '_> {
        Box::new(
            self.crates
                .values()
                .filter(|crate_info| crate_info.json_path.is_some()),
        )
    }

    fn canonicalize(&self, input_name: &str) -> Option<CrateName<'static>> {
        let canonical = match input_name {
            "std" | "std_crate" => "std",
            "core" | "core_crate" => "core",
            "alloc" | "alloc_crate" => "alloc",
            "proc_macro" | "proc_macro_crate" => "proc_macro",
            "test" | "test_crate" => "test",
            "std_detect" => "std_detect", // fake crate
            _ => return None,
        };

        Some(CrateName::from(canonical))
    }
}
