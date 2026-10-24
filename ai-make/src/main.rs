mod builder;
mod config;

use anyhow::{Context, Result};
use builder::Builder;
use config::Config;
use std::{env, path::PathBuf};

fn main() -> Result<()> {
    let config_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("AiMake.yaml"));
    let config = Config::load(&config_path)
        .with_context(|| format!("load configuration {}", config_path.display()))?;
    Builder::new(config_path, config)?.build()
}
