use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::features;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelRatchet {
    Free,
    Ratchet,
}

impl WheelRatchet {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Ratchet => "ratchet",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "free" => Ok(Self::Free),
            "ratchet" => Ok(Self::Ratchet),
            _ => Err("wheel_ratchet must be `free` or `ratchet`".into()),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SavedConfig {
    pub dpi: Option<u32>,
    pub wheel_ratchet: Option<WheelRatchet>,
    pub wheel_ratchet_speed: Option<u8>,
    pub wheel_force: Option<u8>,
    pub wheel_invert: Option<bool>,
    pub wheel_resolution: Option<bool>,
    pub wheel_divert: Option<bool>,
    pub thumb_wheel_invert: Option<bool>,
    pub thumb_wheel_divert: Option<bool>,
    pub force_button: Option<u16>,
    pub haptic_strength: Option<u8>,
}

impl SavedConfig {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    fn to_toml(&self) -> String {
        let mut lines = vec![
            "# Saved by mx4. Adjusting this file by hand is supported.".to_string(),
            "# Unsupported keys are ignored.".to_string(),
            String::new(),
        ];

        push_u32(&mut lines, "dpi", self.dpi);
        push_string(
            &mut lines,
            "wheel_ratchet",
            self.wheel_ratchet.map(WheelRatchet::as_str),
        );
        push_u8(&mut lines, "wheel_ratchet_speed", self.wheel_ratchet_speed);
        push_u8(&mut lines, "wheel_force", self.wheel_force);
        push_bool(&mut lines, "wheel_invert", self.wheel_invert);
        push_bool(&mut lines, "wheel_resolution", self.wheel_resolution);
        push_bool(&mut lines, "wheel_divert", self.wheel_divert);
        push_bool(&mut lines, "thumb_wheel_invert", self.thumb_wheel_invert);
        push_bool(&mut lines, "thumb_wheel_divert", self.thumb_wheel_divert);
        push_u16(&mut lines, "force_button", self.force_button);
        push_u8(&mut lines, "haptic_strength", self.haptic_strength);

        lines.join("\n") + "\n"
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn load() -> Result<SavedConfig> {
    load_from_path(&config_path()?)
}

pub fn save(config: &SavedConfig) -> Result<()> {
    let path = config_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, config.to_toml())?;
    Ok(())
}

pub fn update(mutator: impl FnOnce(&mut SavedConfig)) -> Result<()> {
    let mut config = load()?;
    mutator(&mut config);
    save(&config)
}

pub fn apply_saved_settings() -> Result<()> {
    apply(&load()?)
}

pub fn apply_best_effort(config: &SavedConfig) -> Vec<String> {
    let mut errors = Vec::new();

    if let Some(dpi) = config.dpi {
        if let Err(err) = features::dpi::set(&dpi.to_string()) {
            errors.push(format!("dpi: {err}"));
        }
    }

    if let Some(ratchet) = config.wheel_ratchet {
        if let Err(err) = features::wheel::set("ratchet", ratchet.as_str()) {
            errors.push(format!("wheel ratchet: {err}"));
        }
    }

    if let Some(speed) = config.wheel_ratchet_speed {
        if let Err(err) = features::wheel::set("ratchet-speed", &speed.to_string()) {
            errors.push(format!("wheel ratchet-speed: {err}"));
        }
    }

    if let Some(force) = config.wheel_force {
        if let Err(err) = features::wheel::set("force", &force.to_string()) {
            errors.push(format!("wheel force: {err}"));
        }
    }

    if let Some(invert) = config.wheel_invert {
        if let Err(err) = features::wheel::set("invert", on_off(invert)) {
            errors.push(format!("wheel invert: {err}"));
        }
    }

    if let Some(resolution) = config.wheel_resolution {
        if let Err(err) = features::wheel::set("resolution", on_off(resolution)) {
            errors.push(format!("wheel resolution: {err}"));
        }
    }

    if let Some(divert) = config.wheel_divert {
        if let Err(err) = features::wheel::set("divert", on_off(divert)) {
            errors.push(format!("wheel divert: {err}"));
        }
    }

    if let Some(invert) = config.thumb_wheel_invert {
        if let Err(err) = features::wheel::set_thumb("invert", on_off(invert)) {
            errors.push(format!("thumb-wheel invert: {err}"));
        }
    }

    if let Some(divert) = config.thumb_wheel_divert {
        if let Err(err) = features::wheel::set_thumb("divert", on_off(divert)) {
            errors.push(format!("thumb-wheel divert: {err}"));
        }
    }

    if let Some(force_button) = config.force_button {
        if let Err(err) = features::force_button::set(&force_button.to_string()) {
            errors.push(format!("force-button: {err}"));
        }
    }

    if let Some(haptic_strength) = config.haptic_strength {
        if let Err(err) = features::haptic::set_strength_arg(&haptic_strength.to_string()) {
            errors.push(format!("haptic strength: {err}"));
        }
    }

    errors
}

fn apply(config: &SavedConfig) -> Result<()> {
    let errors = apply_best_effort(config);

    if let Some(first) = errors.first() {
        Err(first.clone().into())
    } else {
        Ok(())
    }
}

fn config_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("couldn't determine HOME")?;

    let base = if cfg!(target_os = "macos") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".config"))
    };

    Ok(base.join("mx4"))
}

fn load_from_path(path: &Path) -> Result<SavedConfig> {
    if !path.exists() {
        return Ok(SavedConfig::default());
    }

    parse(&fs::read_to_string(path)?)
}

fn parse(data: &str) -> Result<SavedConfig> {
    let mut config = SavedConfig::default();

    for raw_line in data.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, raw_value) = line
            .split_once('=')
            .ok_or("config lines must look like `key = value`")?;
        let key = key.trim();
        let value = raw_value.trim();

        match key {
            "dpi" => config.dpi = Some(value.parse()?),
            "wheel_ratchet" => {
                config.wheel_ratchet = Some(WheelRatchet::parse(&parse_string(value)?)?)
            }
            "wheel_ratchet_speed" => config.wheel_ratchet_speed = Some(value.parse()?),
            "wheel_force" => config.wheel_force = Some(value.parse()?),
            "wheel_invert" => config.wheel_invert = Some(parse_bool(value)?),
            "wheel_resolution" => config.wheel_resolution = Some(parse_bool(value)?),
            "wheel_divert" => config.wheel_divert = Some(parse_bool(value)?),
            "thumb_wheel_invert" => config.thumb_wheel_invert = Some(parse_bool(value)?),
            "thumb_wheel_divert" => config.thumb_wheel_divert = Some(parse_bool(value)?),
            "force_button" => config.force_button = Some(value.parse()?),
            "haptic_strength" => config.haptic_strength = Some(value.parse()?),
            _ => {}
        }
    }

    Ok(config)
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("boolean config values must be `true` or `false`".into()),
    }
}

fn parse_string(value: &str) -> Result<String> {
    let trimmed = value.trim();

    if !(trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2) {
        return Err("string config values must be double-quoted".into());
    }

    Ok(trimmed[1..trimmed.len() - 1]
        .replace("\\\"", "\"")
        .replace("\\\\", "\\"))
}

fn push_bool(lines: &mut Vec<String>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
}

fn push_string(lines: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        lines.push(format!(
            r#"{key} = "{}""#,
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
}

fn push_u8(lines: &mut Vec<String>, key: &str, value: Option<u8>) {
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
}

fn push_u16(lines: &mut Vec<String>, key: &str, value: Option<u16>) {
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
}

fn push_u32(lines: &mut Vec<String>, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::{SavedConfig, WheelRatchet, parse};

    #[test]
    fn parses_saved_config() {
        let config = parse(
            r#"
            dpi = 2500
            wheel_ratchet = "free"
            wheel_ratchet_speed = 10
            wheel_force = 75
            wheel_invert = true
            wheel_resolution = false
            wheel_divert = false
            thumb_wheel_invert = false
            thumb_wheel_divert = true
            force_button = 4310
            haptic_strength = 60
            "#,
        )
        .unwrap();

        assert_eq!(
            config,
            SavedConfig {
                dpi: Some(2500),
                wheel_ratchet: Some(WheelRatchet::Free),
                wheel_ratchet_speed: Some(10),
                wheel_force: Some(75),
                wheel_invert: Some(true),
                wheel_resolution: Some(false),
                wheel_divert: Some(false),
                thumb_wheel_invert: Some(false),
                thumb_wheel_divert: Some(true),
                force_button: Some(4310),
                haptic_strength: Some(60),
            }
        );
    }

    #[test]
    fn ignores_unknown_keys() {
        let config = parse("unknown = 1\nwheel_invert = true\n").unwrap();
        assert_eq!(config.wheel_invert, Some(true));
        assert_eq!(config.dpi, None);
    }

    #[test]
    fn renders_valid_config() {
        let text = SavedConfig {
            dpi: Some(2500),
            wheel_ratchet: Some(WheelRatchet::Ratchet),
            wheel_ratchet_speed: None,
            wheel_force: None,
            wheel_invert: Some(true),
            wheel_resolution: Some(false),
            wheel_divert: Some(true),
            thumb_wheel_invert: None,
            thumb_wheel_divert: None,
            force_button: Some(4310),
            haptic_strength: Some(60),
        }
        .to_toml();

        assert!(text.contains("dpi = 2500"));
        assert!(text.contains(r#"wheel_ratchet = "ratchet""#));
        assert!(text.contains("wheel_invert = true"));
        assert!(text.contains("wheel_divert = true"));
        assert!(text.contains("force_button = 4310"));
        assert!(text.contains("haptic_strength = 60"));
    }
}
