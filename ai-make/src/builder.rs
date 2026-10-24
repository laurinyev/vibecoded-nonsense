use crate::config::{Config, Target};
use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Builder {
    config_dir: PathBuf,
    config: Config,
    ai_cc_dir: PathBuf,
    ai_cc: PathBuf,
}

impl Builder {
    pub fn new(config_path: PathBuf, config: Config) -> Result<Self> {
        let config_dir = config_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()
            .with_context(|| {
                format!("resolve configuration directory {}", config_path.display())
            })?;
        let ai_cc_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("ai-cc")
            .canonicalize()
            .context("resolve ../ai-cc workspace")?;
        let ai_cc = ai_cc_dir.join("target").join("release").join("ai-cc");
        Ok(Self {
            config_dir,
            config,
            ai_cc_dir,
            ai_cc,
        })
    }

    pub fn build(&self) -> Result<()> {
        self.build_compiler()?;
        for (name, target) in &self.config.targets {
            self.build_target(name, target)?;
        }
        Ok(())
    }

    fn build_compiler(&self) -> Result<()> {
        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&self.ai_cc_dir)
            .status()
            .context("run cargo build --release in ../ai-cc")?;
        if !status.success() {
            bail!("ai-cc release build failed with {status}");
        }
        if !self.ai_cc.is_file() {
            bail!(
                "ai-cc release binary was not produced: {}",
                self.ai_cc.display()
            );
        }
        Ok(())
    }

    fn build_target(&self, name: &str, target: &Target) -> Result<()> {
        let output = resolve_path(&self.config_dir, &target.output);
        let artifact_dir = output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".ai-make")
            .join(name);
        fs::create_dir_all(&artifact_dir)
            .with_context(|| format!("create artifact directory {}", artifact_dir.display()))?;
        let mut objects = Vec::with_capacity(target.sources.len());
        for (index, source) in target.sources.iter().enumerate() {
            let source = resolve_path(&self.config_dir, source);
            if !source.is_file() {
                bail!("target {name} source does not exist: {}", source.display());
            }
            let object = artifact_dir.join(format!("{index:04}.o"));
            self.compile_source(name, target, &source, &object)?;
            objects.push(object);
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create output directory {}", parent.display()))?;
        }
        let status = Command::new("cc")
            .args(&target.link_flags)
            .args(&objects)
            .arg("-o")
            .arg(&output)
            .status()
            .with_context(|| format!("link target {name}"))?;
        if !status.success() {
            bail!("linking target {name} failed with {status}");
        }
        Ok(())
    }

    fn compile_source(
        &self,
        name: &str,
        target: &Target,
        source: &Path,
        object: &Path,
    ) -> Result<()> {
        let status = Command::new(&self.ai_cc)
            .args(&target.flags)
            .args(["-c", "-o"])
            .arg(object)
            .arg(source)
            .status()
            .with_context(|| format!("compile {} source {}", name, source.display()))?;
        if !status.success() {
            bail!(
                "compiling {} failed for {} with {status}",
                name,
                source.display()
            );
        }
        Ok(())
    }
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
