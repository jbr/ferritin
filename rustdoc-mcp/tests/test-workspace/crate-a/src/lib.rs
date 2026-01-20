use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct CrateAStruct {
    pub name: String,
    pub value: i32,
}

pub fn process_data(data: &str) -> Result<CrateAStruct, Box<dyn std::error::Error>> {
    let regex = regex::Regex::new(r"(\w+):(\d+)")?;
    if let Some(caps) = regex.captures(data) {
        Ok(CrateAStruct {
            name: caps[1].to_string(),
            value: caps[2].parse()?,
        })
    } else {
        Err("Invalid format".into())
    }
}