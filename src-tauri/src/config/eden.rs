//! Eden per-game configuration.
//!
//! Eden inherits yuzu's scheme: `config/custom/<TitleID>.ini`, where every key
//! is paired with a `\use_global` flag. Overriding a setting means writing
//! *two* lines:
//!
//! ```ini
//! [Renderer]
//! use_vsync\use_global=false
//! use_vsync=1
//! ```
//!
//! and inheriting means `use_vsync\use_global=true` with no value line. This
//! maps cleanly onto "absent = inherit" everywhere else in the app: clearing an
//! override flips the flag back to `true` rather than deleting the key, because
//! Eden expects the flag to be present.
//!
//! Three per-game configs already exist on this machine, so files are merged
//! rather than rewritten.

use super::ini_merge::{self, Edit};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Per-game overrides. Every field optional: `None` = inherit global.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EdenGameConfig {
    /// 0 = OpenGL, 1 = Vulkan, 2 = Null.
    pub graphics_backend: Option<u32>,
    /// Internal resolution scale: 1 = 720p (native docked), 2 = 1440p, ...
    pub resolution_setup: Option<u32>,
    /// 0 = nearest, 1 = bilinear, 2 = bicubic, 3 = gaussian, 4 = scaleforce, 5 = FSR.
    pub scaling_filter: Option<u32>,
    /// 0 = none, 1 = FXAA, 2 = SMAA.
    pub anti_aliasing: Option<u32>,
    /// 0 = normal, 1 = high, 2 = extreme.
    pub gpu_accuracy: Option<u32>,
    pub use_vsync: Option<u32>,
    pub use_multi_core: Option<bool>,
    /// Percentage; requires `use_speed_limit`.
    pub speed_limit: Option<u32>,
    pub use_docked_mode: Option<bool>,
}

/// Which INI section each key lives in. Getting this wrong means Eden silently
/// ignores the override.
fn section_for(key: &str) -> &'static str {
    match key {
        "use_multi_core" | "use_speed_limit" | "speed_limit" => "Core",
        "use_docked_mode" => "System",
        _ => "Renderer",
    }
}

fn bool_str(v: bool) -> String {
    if v { "true".into() } else { "false".into() }
}

impl EdenGameConfig {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Build the paired `\use_global` + value edits.
    ///
    /// Managed keys always emit their flag — set to `false` with a value when
    /// overridden, `true` when not — so clearing an override in the UI actually
    /// restores global inheritance instead of leaving the old value behind.
    fn edits(&self) -> Vec<Edit> {
        let mut out: Vec<Edit> = Vec::new();
        let mut push = |key: &str, value: Option<String>| {
            let section = section_for(key).to_string();
            match value {
                Some(v) => {
                    out.push((section.clone(), format!("{key}\\use_global"), Some("false".into())));
                    out.push((section, key.to_string(), Some(v)));
                }
                None => {
                    out.push((section.clone(), format!("{key}\\use_global"), Some("true".into())));
                    // Drop any stale value line so the global genuinely applies.
                    out.push((section, key.to_string(), None));
                }
            }
        };

        push("backend", self.graphics_backend.map(|v| v.to_string()));
        push("resolution_setup", self.resolution_setup.map(|v| v.to_string()));
        push("scaling_filter", self.scaling_filter.map(|v| v.to_string()));
        push("anti_aliasing", self.anti_aliasing.map(|v| v.to_string()));
        push("gpu_accuracy", self.gpu_accuracy.map(|v| v.to_string()));
        push("use_vsync", self.use_vsync.map(|v| v.to_string()));
        push("use_multi_core", self.use_multi_core.map(bool_str));
        push("use_docked_mode", self.use_docked_mode.map(bool_str));

        // A speed limit only takes effect with its enable flag set.
        match self.speed_limit {
            Some(v) => {
                push("use_speed_limit", Some("true".into()));
                push("speed_limit", Some(v.to_string()));
            }
            None => {
                push("use_speed_limit", None);
                push("speed_limit", None);
            }
        }
        out
    }
}

pub fn config_path(eden_data_dir: &Path, title_id: &str) -> PathBuf {
    eden_data_dir
        .join("config")
        .join("custom")
        .join(format!("{}.ini", title_id.to_uppercase()))
}

/// Write per-game overrides, preserving every unmanaged key already present.
pub fn apply(eden_data_dir: &Path, title_id: &str, config: &EdenGameConfig) -> Result<()> {
    let path = config_path(eden_data_dir, title_id);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    // With nothing overridden and no file yet, writing a config of pure
    // `use_global=true` would be noise -- leave the filesystem alone.
    if config.is_empty() && existing.is_empty() {
        return Ok(());
    }

    let merged = ini_merge::merge(&existing, &config.edits());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, merged).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of a real per-game config on this machine.
    const EXISTING: &str = "[LibraryApplet]\ncabinet_applet_mode\\use_global=true\nweb_applet_mode\\use_global=true\n\n\n[Core]\nuse_multi_core\\use_global=true\nspeed_limit\\use_global=true\n\n\n[Renderer]\nbackend\\use_global=true\n";

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eden-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn override_writes_flag_and_value_together() {
        let dir = tmp("override");
        apply(
            &dir,
            "0100000000010000",
            &EdenGameConfig { graphics_backend: Some(1), ..Default::default() },
        )
        .unwrap();

        let out = std::fs::read_to_string(config_path(&dir, "0100000000010000")).unwrap();
        assert!(out.contains("backend\\use_global = false"), "{out}");
        assert!(out.contains("\nbackend = 1"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_an_override_restores_inheritance() {
        let dir = tmp("clear");
        let path = config_path(&dir, "0100000000010000");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[Renderer]\nbackend\\use_global=false\nbackend=1\n").unwrap();

        apply(&dir, "0100000000010000", &EdenGameConfig::default()).unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("backend\\use_global = true"), "{out}");
        // The stale value must be gone, or Eden keeps applying it.
        assert!(!out.contains("\nbackend = 1"), "stale value survived:\n{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preserves_unmanaged_sections_and_keys() {
        let dir = tmp("preserve");
        let path = config_path(&dir, "01006BD001E06000");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, EXISTING).unwrap();

        apply(
            &dir,
            "01006BD001E06000",
            &EdenGameConfig { resolution_setup: Some(2), ..Default::default() },
        )
        .unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("cabinet_applet_mode\\use_global=true"), "applet keys lost:\n{out}");
        assert!(out.contains("web_applet_mode\\use_global=true"));
        assert!(out.contains("resolution_setup = 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn speed_limit_enables_its_companion_flag() {
        let dir = tmp("speed");
        apply(
            &dir,
            "0100000000010000",
            &EdenGameConfig { speed_limit: Some(150), ..Default::default() },
        )
        .unwrap();
        let out = std::fs::read_to_string(config_path(&dir, "0100000000010000")).unwrap();
        assert!(out.contains("use_speed_limit = true"), "{out}");
        assert!(out.contains("speed_limit = 150"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_land_in_the_right_sections() {
        assert_eq!(section_for("backend"), "Renderer");
        assert_eq!(section_for("resolution_setup"), "Renderer");
        assert_eq!(section_for("use_multi_core"), "Core");
        assert_eq!(section_for("speed_limit"), "Core");
        assert_eq!(section_for("use_docked_mode"), "System");
    }

    #[test]
    fn config_filename_is_uppercase_title_id() {
        let p = config_path(Path::new("data"), "01006bd001e06000");
        assert!(p.ends_with("01006BD001E06000.ini"), "{p:?}");
    }

    #[test]
    fn no_file_is_created_for_an_empty_config() {
        let dir = tmp("noop");
        apply(&dir, "0100000000010000", &EdenGameConfig::default()).unwrap();
        assert!(!config_path(&dir, "0100000000010000").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
