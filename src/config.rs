use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItem {
    pub label: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub children: Vec<MenuItem>,
    #[serde(default)]
    pub separator_before: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub items: Vec<MenuItem>,
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(config_dir()?.join("config.toml"))
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read required config {}", path.display()))?;
        let config: Self =
            toml::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))?;

        if config.items.is_empty() {
            anyhow::bail!(
                "{} must contain at least one [[items]] entry",
                path.display()
            );
        }

        Ok(config)
    }
}

pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("mhyprmenu"));
    }

    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/mhyprmenu"))
}
