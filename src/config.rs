use cronwave_lib::structs::ConfigInfo;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

pub fn get_config() -> Result<ConfigInfo, Box<dyn std::error::Error>> {
    let cfg: ConfigInfo = confy::load("cronwave", None)?;
    dbg!(&cfg);
    Ok(cfg)
}
