use anyhow::Result;
use crate_a::CrateAStruct;

// Prefix-resolution fixtures. Every re-export's `use.id` points into `crate-a`,
// which is NOT in crate-b's local index — so the iterator falls through to
// `Navigator::resolve_path(&use.source)` for each. This exercises the
// cross-crate fallback path where prefix-rewriting actually matters.
//
// Shapes exercised (rustdoc emits `source` verbatim, modulo glob expansion):
// - `crate_a::CrateAStruct`    — absolute external path (baseline)
// - `self::PrefixCrateAAlias`  — self:: at crate root, id foreign
// - `super::PrefixCrateAAlias` — super:: from `prefix_inner`, id foreign
// - `crate::PrefixCrateAAlias` — crate:: from depth, id foreign
pub use crate_a::CrateAStruct as PrefixCrateAAlias;
pub use self::PrefixCrateAAlias as SelfPrefixAlias;

pub mod prefix_inner {
    pub use super::PrefixCrateAAlias as SuperPrefixAlias;
    pub use crate::PrefixCrateAAlias as CratePrefixAlias;
}

pub struct CrateBProcessor {
    pub data: Vec<CrateAStruct>,
}

impl CrateBProcessor {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn add_item(&mut self, item: CrateAStruct) -> Result<()> {
        log::info!("Adding item: {:?}", item);
        self.data.push(item);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.data.len()
    }
}