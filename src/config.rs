use cronwave::structs::ConfigInfo;

pub fn get_config() -> Result<ConfigInfo, Box<dyn std::error::Error>> {
    let cfg: ConfigInfo = confy::load("cronwave", None)?;
    dbg!(&cfg);
    Ok(cfg)
}
