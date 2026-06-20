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

// Aliased-crate fixture. `aliased_a` renames `crate_a` at the source level, so
// rustdoc may record this re-export's `source` with a leading `aliased_a`
// segment even though the item lives in `crate_a`. The `use.id` still points to
// `crate-a` via the `paths` map, so id-based resolution finds it — while the
// `source` string would (wrongly) try to load a crate named `aliased_a`. This
// reproduces quinn's `proto` (= quinn_proto) / `udp` (= quinn_udp) re-exports.
use crate_a as aliased_a;
pub use aliased_a::CrateAStruct as AliasedCrateAStruct;

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