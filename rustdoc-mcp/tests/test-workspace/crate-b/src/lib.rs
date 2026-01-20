use anyhow::Result;
use crate_a::CrateAStruct;

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