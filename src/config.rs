use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Clone, Serialize, Debug)]
pub struct ConfigInfo {
    pub cal_url: String,
    pub cal_username: String,
    pub cal_pass: String,
}

#[derive(Deserialize, Clone, Serialize, Debug)]
pub struct auth {
    pub auth: ConfigInfo,
}
impl ::std::default::Default for auth {
    fn default() -> Self {
        Self {
            auth: ConfigInfo {
                cal_url: "your cal url".to_string(),
                cal_username: "your cal_username".to_string(),
                cal_pass: "your cal password".to_string(),
            },
        }
    }
}
pub fn get_config() -> Result<auth, Box<dyn std::error::Error>> {
    let cfg: auth = confy::load("cronwave", None)?;
    dbg!(&cfg);
    Ok(cfg)
}
// pub fn get_config() -> auth {
//     get_config_confy().expect("failed to get confy config");
//     let file = confy::get_configuration_file_path("cronwave", None)
//         .expect("couldnt file config file path");
//     let config_data: auth = toml::from_str(file.to_str().expect("couldnt convert file to str"))
//         .expect("failed to parse the config");
//     config_data
// }
