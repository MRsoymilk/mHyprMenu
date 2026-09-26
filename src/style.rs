use std::{fmt, fs, path::Path};

use anyhow::{Context, Result};
use cosmic_text::Color;
use serde::{Deserialize, Deserializer};

use crate::config::config_dir;

#[derive(Debug, Clone, Deserialize)]
pub struct Style {
    pub menu: MenuStyle,
    pub font: FontStyle,
    pub colors: Colors,
    pub border: BorderStyle,
    pub separator: SeparatorStyle,
    pub indicator: IndicatorStyle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuStyle {
    pub width: f64,
    pub item_height: f64,
    pub padding_x: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FontStyle {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Colors {
    pub background: Rgba,
    pub border: Rgba,
    pub hover: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    pub separator: Rgba,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BorderStyle {
    pub width: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeparatorStyle {
    pub inset: f64,
    pub width: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndicatorStyle {
    pub symbol: String,
    pub width: f64,
    pub right_padding: f64,
    pub size_delta: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba([u8; 4]);

impl Rgba {
    pub fn bytes(self) -> [u8; 4] {
        self.0
    }

    pub fn text_color(self) -> Color {
        Color::rgba(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    fn parse(value: &str) -> Result<Self, String> {
        let hex = value
            .strip_prefix('#')
            .ok_or_else(|| format!("color {value:?} must start with '#'"))?;

        let (rgb, alpha) = match hex.len() {
            6 => (hex, 255),
            8 => (&hex[..6], parse_byte(&hex[6..8])?),
            _ => {
                return Err(format!("color {value:?} must use #RRGGBB or #RRGGBBAA"));
            }
        };

        Ok(Self([
            parse_byte(&rgb[0..2])?,
            parse_byte(&rgb[2..4])?,
            parse_byte(&rgb[4..6])?,
            alpha,
        ]))
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02X}{:02X}{:02X}{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

fn parse_byte(value: &str) -> Result<u8, String> {
    u8::from_str_radix(value, 16).map_err(|_| format!("invalid hex byte {value:?}"))
}

impl Style {
    pub fn load() -> Result<Self> {
        let path = config_dir()?.join("style.toml");
        Self::load_from(path)
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read required style {}", path.display()))?;
        let style: Self =
            toml::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))?;
        style.validate(path)?;
        Ok(style)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        if self.menu.width <= 0.0 {
            anyhow::bail!("{}: menu.width must be > 0", path.display());
        }
        if self.menu.item_height <= 0.0 {
            anyhow::bail!("{}: menu.item_height must be > 0", path.display());
        }
        if self.font.size <= 0.0 {
            anyhow::bail!("{}: font.size must be > 0", path.display());
        }
        if self.font.family.trim().is_empty() {
            anyhow::bail!("{}: font.family must not be empty", path.display());
        }
        if self.border.width < 0.0 || self.separator.width < 0.0 {
            anyhow::bail!("{}: border/separator width must be >= 0", path.display());
        }
        if self.indicator.symbol.is_empty() {
            anyhow::bail!("{}: indicator.symbol must not be empty", path.display());
        }
        Ok(())
    }
}
