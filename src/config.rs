//! Persistence for settings that should survive reconnects.
//!
//! The format is a deliberately small, dependency-free TOML subset. Unknown keys do not make loading
//! fail, so a binary can tolerate config fields it does not yet understand.

use std::env;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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
    // `None` means mx4 should leave that device setting alone when reconnecting.
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
    let path = config_write_path(&config_path()?)?;
    let _lock = lock_config(&path)?;
    save_to_path(&path, config)
}

fn lock_config(path: &Path) -> Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Keep a separate, persistent lock inode: replacing the config must not replace its lock.
    // Closing the handle releases the lock, including on errors or process termination.
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.lock()?;
    Ok(lock)
}

pub fn update(mutator: impl FnOnce(&mut SavedConfig)) -> Result<()> {
    update_at_path(&config_path()?, mutator)
}

fn update_at_path(path: &Path, mutator: impl FnOnce(&mut SavedConfig)) -> Result<()> {
    let path = config_write_path(path)?;
    let _lock = lock_config(&path)?;
    let mut config = load_from_path(&path)?;
    mutator(&mut config);
    save_to_path(&path, &config)
}

fn config_write_path(path: &Path) -> Result<PathBuf> {
    let mut path = path.to_path_buf();
    // Resolve before both locking and writing, so aliases share a lock and atomic replacement
    // updates the target without removing the user's symlink. Also support dangling relative links.
    for _ in 0..40 {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = fs::read_link(&path)?;
                path = if target.is_absolute() {
                    target
                } else {
                    path.parent().unwrap_or(Path::new(".")).join(target)
                };
            }
            Ok(_) => return Ok(fs::canonicalize(path)?),
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                fs::create_dir_all(parent)?;
                let filename = path.file_name().ok_or("config path must name a file")?;
                return Ok(fs::canonicalize(parent)?.join(filename));
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err("too many symlinks in the config path".into())
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TemporaryConfig(PathBuf);

impl Drop for TemporaryConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn save_to_path(path: &Path, config: &SavedConfig) -> Result<()> {
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary) {
            Ok(file) => break (TemporaryConfig(temporary), file),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    };
    if let Ok(metadata) = fs::metadata(path) {
        file.set_permissions(metadata.permissions())?;
    }
    file.write_all(config.to_toml().as_bytes())?;
    file.sync_all()?;
    drop(file);
    // Same-directory rename makes readers see either the old or the complete new configuration.
    fs::rename(&temporary.0, path)?;
    Ok(())
}

pub fn apply_saved_settings() -> Result<()> {
    apply(&load()?)
}

pub fn apply_best_effort(config: &SavedConfig) -> Vec<String> {
    let mut errors = Vec::new();

    // Each feature opens the current transport independently. Continue after failures so one
    // unsupported or temporarily busy feature does not prevent the remaining settings applying.
    if let Some(dpi) = config.dpi {
        collect_error(&mut errors, "dpi", features::dpi::set(&dpi.to_string()));
    }

    if let Some(ratchet) = config.wheel_ratchet {
        collect_error(
            &mut errors,
            "wheel ratchet",
            features::wheel::set("ratchet", ratchet.as_str()),
        );
    }

    if let Some(speed) = config.wheel_ratchet_speed {
        collect_error(
            &mut errors,
            "wheel ratchet-speed",
            features::wheel::set("ratchet-speed", &speed.to_string()),
        );
    }

    if let Some(force) = config.wheel_force {
        collect_error(
            &mut errors,
            "wheel force",
            features::wheel::set("force", &force.to_string()),
        );
    }

    if let Some(invert) = config.wheel_invert {
        collect_error(
            &mut errors,
            "wheel invert",
            features::wheel::set("invert", on_off(invert)),
        );
    }

    if let Some(resolution) = config.wheel_resolution {
        collect_error(
            &mut errors,
            "wheel resolution",
            features::wheel::set("resolution", on_off(resolution)),
        );
    }

    if let Some(divert) = config.wheel_divert {
        collect_error(
            &mut errors,
            "wheel divert",
            features::wheel::set("divert", on_off(divert)),
        );
    }

    if let Some(invert) = config.thumb_wheel_invert {
        collect_error(
            &mut errors,
            "thumb-wheel invert",
            features::wheel::set_thumb("invert", on_off(invert)),
        );
    }

    if let Some(divert) = config.thumb_wheel_divert {
        collect_error(
            &mut errors,
            "thumb-wheel divert",
            features::wheel::set_thumb("divert", on_off(divert)),
        );
    }

    if let Some(force_button) = config.force_button {
        collect_error(
            &mut errors,
            "force-button",
            features::force_button::set(&force_button.to_string()),
        );
    }

    if let Some(haptic_strength) = config.haptic_strength {
        collect_error(
            &mut errors,
            "haptic strength",
            features::haptic::set_strength_arg(&haptic_strength.to_string()),
        );
    }

    errors
}

fn collect_error(errors: &mut Vec<String>, label: &str, result: Result<()>) {
    if let Err(err) = result {
        errors.push(format!("{label}: {err}"));
    }
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

    // Only the scalar shapes emitted by `to_toml` are supported; a full TOML parser would be
    // unnecessary for this flat file and would add a dependency to the tiny crate.
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
    fn temporary_path() -> std::path::PathBuf {
        let sequence = super::TEMP_SEQUENCE.fetch_add(1, super::Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("mx4-config-test-{}-{sequence}", std::process::id()))
            .join("config.toml")
    }

    #[cfg(unix)]
    #[test]
    fn updates_symlink_targets_without_replacing_links() {
        use std::{fs, os::unix::fs::symlink};
        for relative in [false, true] {
            let path = temporary_path();
            let root = path.parent().unwrap();
            let target = root.join("dotfiles/settings.lock");
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, "dpi = 2000\n").unwrap();
            let intermediate = root.join("linked-config");
            symlink(
                if relative {
                    std::path::Path::new("dotfiles/settings.lock")
                } else {
                    &target
                },
                &intermediate,
            )
            .unwrap();
            symlink("linked-config", &path).unwrap();

            super::update_at_path(&path, |config| config.dpi = Some(2500)).unwrap();

            assert!(path.is_symlink());
            assert!(intermediate.is_symlink());
            assert_eq!(super::load_from_path(&target).unwrap().dpi, Some(2500));
            assert_eq!(
                super::config_write_path(&path).unwrap(),
                super::config_write_path(&target).unwrap()
            );
            assert!(target.with_file_name("settings.lock.lock").is_file());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn creates_dangling_symlink_target_and_rejects_cycles() {
        use std::{fs, os::unix::fs::symlink};
        let path = temporary_path();
        let root = path.parent().unwrap();
        fs::create_dir_all(root).unwrap();
        symlink("dotfiles/config.toml", &path).unwrap();
        super::update_at_path(&path, |config| config.dpi = Some(2500)).unwrap();
        assert!(path.is_symlink());
        assert_eq!(
            super::load_from_path(&root.join("dotfiles/config.toml"))
                .unwrap()
                .dpi,
            Some(2500)
        );
        fs::remove_file(&path).unwrap();
        symlink("config.toml", &path).unwrap();
        assert!(super::update_at_path(&path, |_| panic!("must reject a symlink cycle")).is_err());
        assert!(path.is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_updates_preserve_every_change() {
        let path = temporary_path();
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    super::update_at_path(path, |config| {
                        let previous = config.dpi.unwrap_or(200);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        config.dpi = Some(previous + 1);
                    })
                    .unwrap();
                });
            }
        });
        assert_eq!(super::load_from_path(&path).unwrap().dpi, Some(208));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn readers_never_see_a_partial_save() {
        let path = temporary_path();
        super::update_at_path(&path, |config| config.dpi = Some(200)).unwrap();
        let finished = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for value in 201..=250 {
                    super::update_at_path(&path, |config| config.dpi = Some(value)).unwrap();
                }
                finished.store(true, super::Ordering::Release);
            });
            while !finished.load(super::Ordering::Acquire) {
                let dpi = super::load_from_path(&path).unwrap().dpi.unwrap();
                assert!((200..=250).contains(&dpi));
            }
        });
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            2
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_config_is_not_overwritten() {
        let path = temporary_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "dpi = invalid\n").unwrap();
        assert!(
            super::update_at_path(&path, |_| panic!("must not mutate invalid config")).is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "dpi = invalid\n");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

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
