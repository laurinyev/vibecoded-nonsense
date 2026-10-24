use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub targets: BTreeMap<String, Target>,
}

#[derive(Debug, Deserialize)]
pub struct Target {
    pub sources: Vec<PathBuf>,
    pub output: PathBuf,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub link_flags: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("read configuration {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&text)
            .with_context(|| format!("parse configuration {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            bail!("configuration must define at least one target");
        }
        for (name, target) in &self.targets {
            if name.is_empty() {
                bail!("target name cannot be empty");
            }
            if target.sources.is_empty() {
                bail!("target {name} must define at least one source");
            }
            if target.output.as_os_str().is_empty() {
                bail!("target {name} must define an output");
            }
            for flag in &target.flags {
                if matches!(flag.as_str(), "-c" | "-S" | "-E" | "-o") || flag == "--" {
                    bail!("target {name} flags cannot contain {flag:?}");
                }
            }
        }
        Ok(())
    }
}
