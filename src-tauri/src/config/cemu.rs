//! Cemu per-game configuration.
//!
//! Cemu splits per-game settings across two mechanisms, and this is the
//! "messier case" the architecture was meant to accommodate:
//!
//! * `gameProfiles/<titleId>.ini` — CPU mode, shader accuracy, controller
//!   profile. Cemu ships read-only defaults in `<install>/gameProfiles/default/`
//!   and reads user overrides from `<data>/gameProfiles/`. We only ever write
//!   the latter.
//! * **Graphic packs** — resolution scaling is not a setting at all; it is a
//!   downloadable pack with named presets, enabled through `settings.xml`.
//!   Modelled in [`graphic_packs`] rather than pretended to be a dropdown.
//!
//! Existing profiles are merged, never rewritten: this machine's Splatoon
//! profile already contains a hand-set `controller1 = ps4`, and clobbering it
//! would silently unbind the user's gamepad.

use super::ini_merge::{self, Edit};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Per-game overrides. Every field optional: `None` = inherit Cemu's default.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CemuGameConfig {
    /// 0 = single-core interpreter, 1 = dual-core recompiler,
    /// 3 = triple-core recompiler, 4 = auto (Cemu's own numbering).
    pub cpu_mode: Option<u32>,
    pub thread_quantum: Option<u32>,
    /// 0 = false, 1 = true, 2 = auto.
    pub accurate_shader_mul: Option<u32>,
    pub precompiled_shaders: Option<u32>,
    pub load_shared_libraries: Option<bool>,
    pub start_with_pad_view: Option<bool>,
}

fn bool_str(v: bool) -> String {
    if v { "true".into() } else { "false".into() }
}

impl CemuGameConfig {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Convert to INI edits, relative to the previously saved overrides.
    ///
    /// Three cases per key, and the distinction matters:
    ///   * set now              -> write it
    ///   * set before, not now  -> **remove** the line ("use global default")
    ///   * never set by us      -> leave alone
    ///
    /// The middle case was missing, so choosing "Use global default" left the
    /// old line in the file and the setting looked stuck. But removing every
    /// unset key is equally wrong: it deleted `threadQuantum = 45000`, a value
    /// Cemu itself had written and the user never touched through this app.
    /// Only `previous` distinguishes the two.
    fn edits_from(&self, previous: &Self) -> Vec<Edit> {
        let mut out: Vec<Edit> = Vec::new();
        let mut push = |section: &str, key: &str, now: Option<String>, before: bool| {
            match now {
                Some(v) => out.push((section.to_string(), key.to_string(), Some(v))),
                None if before => out.push((section.to_string(), key.to_string(), None)),
                None => {}
            }
        };

        push("General", "loadSharedLibraries", self.load_shared_libraries.map(bool_str),
             previous.load_shared_libraries.is_some());
        push("General", "startWithPadView", self.start_with_pad_view.map(bool_str),
             previous.start_with_pad_view.is_some());
        push("CPU", "cpuMode", self.cpu_mode.map(|v| v.to_string()),
             previous.cpu_mode.is_some());
        push("CPU", "threadQuantum", self.thread_quantum.map(|v| v.to_string()),
             previous.thread_quantum.is_some());
        push("Graphics", "accurateShaderMul", self.accurate_shader_mul.map(|v| v.to_string()),
             previous.accurate_shader_mul.is_some());
        push("Graphics", "precompiledShaders", self.precompiled_shaders.map(|v| v.to_string()),
             previous.precompiled_shaders.is_some());
        out
    }
}

/// User profile path. Note this is Cemu's *data* directory, not the install:
/// the install's `gameProfiles/default/` holds Cemu's own shipped defaults and
/// must never be written to.
pub fn profile_path(cemu_data_dir: &Path, title_id: &str) -> PathBuf {
    cemu_data_dir
        .join("gameProfiles")
        .join(format!("{}.ini", title_id.to_lowercase()))
}

/// Write per-game overrides, preserving any settings already in the file.
///
/// `previous` is the last set of overrides this app saved for the game; it is
/// what lets a cleared setting be removed without touching keys we never owned.
pub fn apply(
    cemu_data_dir: &Path,
    title_id: &str,
    previous: &CemuGameConfig,
    config: &CemuGameConfig,
) -> Result<()> {
    let path = profile_path(cemu_data_dir, title_id);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let merged = ini_merge::merge(&existing, &config.edits_from(previous));

    if config.is_empty() {
        // Everything inherited now. Delete the file only if what remains is
        // ours alone -- a profile also holding the user's own keys (a
        // controller binding, say) must survive with those keys intact.
        if merged.lines().all(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with('#') || t.starts_with(';') || t.starts_with('[')
        }) {
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
            return Ok(());
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, merged).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Graphic packs available for a title.
///
/// Resolution scaling lives here rather than in [`CemuGameConfig`] because
/// Cemu genuinely models it this way: a pack declares the title IDs it applies
/// to and offers named presets. Exposing a fake "resolution" dropdown would
/// misrepresent what the emulator can actually do.
pub mod graphic_packs {
    use anyhow::{Context, Result};
    use serde::{Deserialize, Serialize};
    use std::path::{Path, PathBuf};

    /// Presets are grouped: a pack offers several categories (Resolution,
    /// Aspect Ratio, Anti-Aliasing), each with one active choice.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
    #[serde(rename_all = "camelCase")]
    pub struct PresetCategory {
        /// Empty for presets declared without a category.
        pub name: String,
        pub presets: Vec<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct GraphicPack {
        pub name: String,
        /// Path to `rules.txt` relative to the Cemu data directory, using
        /// forward slashes — the exact form `settings.xml` records.
        pub rules_path: String,
        pub description: String,
        pub categories: Vec<PresetCategory>,
        /// An `<Entry>` in settings.xml means enabled; there is no flag.
        pub enabled: bool,
        /// Active `(category, preset)` selections.
        pub active_presets: Vec<(String, String)>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EnabledEntry {
        pub rules_path: String,
        pub presets: Vec<(String, String)>,
    }

    /// Parsed `rules.txt`.
    pub struct Rules {
        pub title_ids: Vec<String>,
        pub name: String,
        pub description: String,
        pub categories: Vec<PresetCategory>,
    }

    /// Parse a pack's `rules.txt`.
    ///
    /// `[Preset]` blocks carry `name` and `category` in either order, so a block
    /// is only committed once the next section starts or the file ends.
    pub fn parse_rules(text: &str) -> Option<Rules> {
        let mut title_ids = Vec::new();
        let mut name = String::new();
        let mut description = String::new();
        let mut categories: Vec<PresetCategory> = Vec::new();

        let mut in_preset = false;
        let mut cur_name: Option<String> = None;
        let mut cur_cat = String::new();

        let commit = |cur_name: &mut Option<String>, cur_cat: &mut String, cats: &mut Vec<PresetCategory>| {
            if let Some(p) = cur_name.take() {
                match cats.iter_mut().find(|c| c.name == *cur_cat) {
                    Some(c) => c.presets.push(p),
                    None => cats.push(PresetCategory {
                        name: cur_cat.clone(),
                        presets: vec![p],
                    }),
                }
            }
            cur_cat.clear();
        };

        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                if in_preset {
                    commit(&mut cur_name, &mut cur_cat, &mut categories);
                }
                in_preset = t.eq_ignore_ascii_case("[Preset]");
                continue;
            }
            let Some((k, v)) = t.split_once('=') else { continue };
            let (k, v) = (k.trim().to_lowercase(), v.trim());
            match k.as_str() {
                "titleids" => {
                    title_ids = v
                        .split(',')
                        .map(|s| s.trim().to_uppercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                }
                "name" if in_preset => cur_name = Some(v.to_string()),
                "category" if in_preset => cur_cat = v.to_string(),
                "name" => name = v.to_string(),
                "description" => description = v.to_string(),
                _ => {}
            }
        }
        if in_preset {
            commit(&mut cur_name, &mut cur_cat, &mut categories);
        }

        (!title_ids.is_empty()).then_some(Rules {
            title_ids,
            name,
            description,
            categories,
        })
    }

    /// Read the `<GraphicPack>` section of settings.xml.
    pub fn parse_enabled(xml: &str) -> Vec<EnabledEntry> {
        let Some(section) = slice_section(xml) else {
            return Vec::new();
        };
        let mut out = Vec::new();

        for chunk in section.split("<Entry ").skip(1) {
            let Some(fname) = between(chunk, "filename=\"", "\"") else {
                continue;
            };
            // Self-closing entries carry no presets.
            let body_end = chunk.find("</Entry>").unwrap_or(0);
            let body = &chunk[..body_end];
            let mut presets = Vec::new();
            for p in body.split("<Preset>").skip(1) {
                let category = between(p, "<category>", "</category>").unwrap_or_default();
                if let Some(preset) = between(p, "<preset>", "</preset>") {
                    presets.push((category, preset));
                }
            }
            out.push(EnabledEntry {
                rules_path: fname,
                presets,
            });
        }
        out
    }

    fn between(s: &str, open: &str, close: &str) -> Option<String> {
        let a = s.find(open)? + open.len();
        let b = s[a..].find(close)? + a;
        Some(s[a..b].to_string())
    }

    fn slice_section(xml: &str) -> Option<&str> {
        let a = xml.find("<GraphicPack>")?;
        let b = xml[a..].find("</GraphicPack>")? + a + "</GraphicPack>".len();
        Some(&xml[a..b])
    }

    fn render_section(entries: &[EnabledEntry]) -> String {
        let mut s = String::from("<GraphicPack>");
        for e in entries {
            if e.presets.is_empty() {
                s.push_str(&format!("\n        <Entry filename=\"{}\"/>", e.rules_path));
            } else {
                s.push_str(&format!("\n        <Entry filename=\"{}\">", e.rules_path));
                for (cat, preset) in &e.presets {
                    s.push_str("\n            <Preset>");
                    if !cat.is_empty() {
                        s.push_str(&format!("\n                <category>{cat}</category>"));
                    }
                    s.push_str(&format!("\n                <preset>{preset}</preset>"));
                    s.push_str("\n            </Preset>");
                }
                s.push_str("\n        </Entry>");
            }
        }
        s.push_str("\n    </GraphicPack>");
        s
    }

    /// Enable/disable a pack and set its presets, returning the new settings.xml.
    ///
    /// Only the `<GraphicPack>` element is rewritten. settings.xml is the user's
    /// entire global Cemu configuration — window geometry, game paths, accounts
    /// — so everything outside that element is passed through untouched.
    pub fn apply_to_settings(
        xml: &str,
        rules_path: &str,
        enabled: bool,
        presets: &[(String, String)],
    ) -> String {
        let mut entries = parse_enabled(xml);
        entries.retain(|e| e.rules_path != rules_path);
        if enabled {
            entries.push(EnabledEntry {
                rules_path: rules_path.to_string(),
                presets: presets.to_vec(),
            });
        }
        let rendered = render_section(&entries);

        match slice_section(xml) {
            Some(existing) => xml.replacen(existing, &rendered, 1),
            // No section yet: insert one before the closing root tag.
            None => match xml.rfind("</content>") {
                Some(i) => format!("{}    {}\n{}", &xml[..i], rendered, &xml[i..]),
                None => format!("{xml}\n{rendered}\n"),
            },
        }
    }

    /// Find packs applying to `title_id`, with their current enabled state.
    pub fn for_title(cemu_data_dir: &Path, title_id: &str) -> Vec<GraphicPack> {
        let root = cemu_data_dir.join("graphicPacks");
        let want = title_id.to_uppercase();
        let settings = std::fs::read_to_string(settings_path(cemu_data_dir)).unwrap_or_default();
        let enabled = parse_enabled(&settings);
        let mut out = Vec::new();

        for entry in walkdir::WalkDir::new(&root)
            .max_depth(6)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_name() != "rules.txt" {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some(rules) = parse_rules(&text) else {
                continue;
            };
            if !rules.title_ids.contains(&want) {
                continue;
            }
            let rules_path = entry
                .path()
                .strip_prefix(cemu_data_dir)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            let active = enabled.iter().find(|e| e.rules_path == rules_path);
            out.push(GraphicPack {
                name: rules.name,
                rules_path,
                description: rules.description,
                categories: rules.categories,
                enabled: active.is_some(),
                active_presets: active.map(|e| e.presets.clone()).unwrap_or_default(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Where enabled packs are recorded.
    pub fn settings_path(cemu_data_dir: &Path) -> PathBuf {
        cemu_data_dir.join("settings.xml")
    }

    /// Write a pack change to settings.xml.
    ///
    /// Refuses while Cemu is running: Cemu holds settings.xml in memory and
    /// rewrites it wholesale on exit, so an edit made now would be silently
    /// discarded the moment the user closes it.
    pub fn set_enabled(
        cemu_data_dir: &Path,
        rules_path: &str,
        enabled: bool,
        presets: &[(String, String)],
    ) -> Result<()> {
        if cemu_is_running() {
            anyhow::bail!(
                "Cemu is running. It rewrites settings.xml when it closes, which \
                 would discard this change — close Cemu and try again."
            );
        }
        let path = settings_path(cemu_data_dir);
        let xml = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let updated = apply_to_settings(&xml, rules_path, enabled, presets);
        std::fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    #[cfg(windows)]
    pub fn cemu_is_running() -> bool {
        // Cheap check via the process list; avoids a Win32 dependency here.
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Cemu.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("cemu.exe"))
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    pub fn cemu_is_running() -> bool {
        std::process::Command::new("pgrep")
            .args(["-x", "Cemu"])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real profile on this machine.
    const EXISTING: &str = "[General]\nloadSharedLibraries = true\nstartWithPadView = false\n\n[CPU]\ncpuMode = 4\nthreadQuantum = 45000\n\n[Graphics]\naccurateShaderMul = 1\nprecompiledShaders = 0\n\n[Controller]\ncontroller1 =  ps4\n";

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cemu-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn nothing_set_and_nothing_previously_set_means_no_edits() {
        let d = CemuGameConfig::default();
        assert!(d.is_empty());
        assert!(d.edits_from(&d).is_empty(), "must not touch keys we never owned");
    }

    #[test]
    fn clearing_only_removes_keys_we_previously_set() {
        let previous = CemuGameConfig { cpu_mode: Some(1), ..Default::default() };
        let edits = CemuGameConfig::default().edits_from(&previous);
        // Exactly one removal: cpuMode. threadQuantum and the rest were never
        // ours, so they must not appear.
        assert_eq!(edits.len(), 1, "{edits:?}");
        assert_eq!(edits[0].1, "cpuMode");
        assert!(edits[0].2.is_none(), "should be a removal");
    }

    #[test]
    fn preserves_the_users_controller_binding() {
        let dir = tmp("preserve");
        let path = profile_path(&dir, "0005000010176A00");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, EXISTING).unwrap();

        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig::default(),
            &CemuGameConfig { cpu_mode: Some(1), ..Default::default() },
        )
        .unwrap();

        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("cpuMode = 1"), "override not applied:\n{out}");
        assert!(
            out.contains("controller1 =  ps4"),
            "user's controller binding was destroyed:\n{out}"
        );
        assert!(out.contains("threadQuantum = 45000"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_filename_is_lowercase_title_id() {
        let p = profile_path(Path::new("data"), "0005000010176A00");
        assert!(p.ends_with("0005000010176a00.ini"), "{p:?}");
    }

    #[test]
    fn clearing_an_override_removes_the_line() {
        let dir = tmp("clear");
        let path = profile_path(&dir, "0005000010176A00");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        // Set an override...
        apply(&dir, "0005000010176A00", &CemuGameConfig::default(),
            &CemuGameConfig { cpu_mode: Some(1), ..Default::default() }).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("cpuMode = 1"));

        // ...then choose "use global default" for it. The line must go, or the
        // setting stays stuck on its old value.
        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig { cpu_mode: Some(1), ..Default::default() },
            &CemuGameConfig::default(),
        )
        .unwrap();
        let after = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!after.contains("cpuMode"), "stale override survived:\n{after}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_one_override_leaves_the_others() {
        let dir = tmp("clear-one");
        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig::default(),
            &CemuGameConfig { cpu_mode: Some(1), start_with_pad_view: Some(true), ..Default::default() },
        )
        .unwrap();

        // Drop only cpu_mode.
        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig { cpu_mode: Some(1), start_with_pad_view: Some(true), ..Default::default() },
            &CemuGameConfig { start_with_pad_view: Some(true), ..Default::default() },
        )
        .unwrap();

        let after = std::fs::read_to_string(profile_path(&dir, "0005000010176A00")).unwrap();
        assert!(!after.contains("cpuMode"), "{after}");
        assert!(after.contains("startWithPadView = true"), "{after}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn can_set_a_boolean_override_to_false() {
        let dir = tmp("boolfalse");
        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig::default(),
            &CemuGameConfig { start_with_pad_view: Some(false), ..Default::default() },
        )
        .unwrap();
        let out = std::fs::read_to_string(profile_path(&dir, "0005000010176A00")).unwrap();
        // Explicit false must be written, not treated as "unset".
        assert!(out.contains("startWithPadView = false"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_a_profile_that_holds_user_only_keys() {
        let dir = tmp("keep");
        let path = profile_path(&dir, "0005000010176A00");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, EXISTING).unwrap();

        // Clearing every managed override must not delete a file that also
        // carries the user's controller setting.
        apply(&dir, "0005000010176A00", &CemuGameConfig::default(),
            &CemuGameConfig::default()).unwrap();
        assert!(path.exists(), "profile with user keys must survive");
        assert!(std::fs::read_to_string(&path).unwrap().contains("ps4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_a_profile_that_is_entirely_ours() {
        let dir = tmp("remove");
        let path = profile_path(&dir, "0005000010176A00");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[CPU]\ncpuMode = 1\n").unwrap();

        // We previously set cpuMode, and the user has now cleared it. With
        // nothing of ours left and no user keys in the file, it goes.
        apply(
            &dir,
            "0005000010176A00",
            &CemuGameConfig { cpu_mode: Some(1), ..Default::default() },
            &CemuGameConfig::default(),
        )
        .unwrap();
        assert!(!path.exists(), "fully-managed profile should be removed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_alone_a_value_we_never_set() {
        let dir = tmp("untouched");
        let path = profile_path(&dir, "0005000010176A00");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Written by Cemu itself, never through this app.
        std::fs::write(&path, "[CPU]\ncpuMode = 4\nthreadQuantum = 45000\n").unwrap();

        apply(&dir, "0005000010176A00", &CemuGameConfig::default(), &CemuGameConfig::default())
            .unwrap();

        let after = std::fs::read_to_string(&path).expect("file must survive");
        assert!(after.contains("cpuMode = 4"), "deleted a value we never owned:\n{after}");
        assert!(after.contains("threadQuantum = 45000"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_uncategorised_presets() {
        // Shape taken from Bayonetta_Resolution/rules.txt on this machine.
        let rules = "[Definition]\ntitleIds = 000500001014DB00,0005000010157E00\nname = Resolution\ndescription = Changes the resolution of the game.\nversion = 4\n\n[Preset]\nname = 1280x720 (Default)\n$width = 1280\n\n[Preset]\nname = 640x360\n$width = 640\n";
        let r = graphic_packs::parse_rules(rules).expect("should parse");
        assert_eq!(r.title_ids.len(), 2);
        assert_eq!(r.name, "Resolution");
        // Preset names must not be confused with the pack's own name.
        assert_eq!(r.categories.len(), 1);
        assert_eq!(r.categories[0].name, "");
        assert_eq!(r.categories[0].presets, vec!["1280x720 (Default)", "640x360"]);
    }

    #[test]
    fn groups_presets_by_category_regardless_of_key_order() {
        // SkylandersGiants/Graphics puts `name` before `category` in some
        // blocks and after it in others.
        let rules = "[Definition]\ntitleIds = 000500001010D700\nname = Graphics Options\n\n[Preset]\nname = 16:9 (Default)\ncategory = Aspect Ratio\n\n[Preset]\nname = 4:3\ncategory = Aspect Ratio\n\n[Preset]\ncategory = Resolution\nname = 640x360\n\n[Preset]\ncategory = Resolution\nname = 2560x1440\n";
        let r = graphic_packs::parse_rules(rules).expect("should parse");
        assert_eq!(r.categories.len(), 2, "{:?}", r.categories);
        let ar = r.categories.iter().find(|c| c.name == "Aspect Ratio").unwrap();
        assert_eq!(ar.presets, vec!["16:9 (Default)", "4:3"]);
        let res = r.categories.iter().find(|c| c.name == "Resolution").unwrap();
        assert_eq!(res.presets, vec!["640x360", "2560x1440"]);
    }

    #[test]
    fn rules_without_title_ids_are_rejected() {
        assert!(graphic_packs::parse_rules("[Definition]\nname = Broken\n").is_none());
    }

    /// Shape taken from this machine's settings.xml.
    const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<content>
    <language>0</language>
    <GraphicPack>
        <Entry filename="graphicPacks/downloadedGraphicPacks/KirbyRainbow_InvisibleLineFix/rules.txt"/>
        <Entry filename="graphicPacks/downloadedGraphicPacks/SkylandersGiants/Graphics/rules.txt">
            <Preset>
                <category>Resolution</category>
                <preset>2560x1440</preset>
            </Preset>
        </Entry>
    </GraphicPack>
    <fullscreen>false</fullscreen>
</content>"#;

    #[test]
    fn reads_enabled_packs_and_their_presets() {
        let e = graphic_packs::parse_enabled(SETTINGS);
        assert_eq!(e.len(), 2);
        assert!(e[0].presets.is_empty(), "self-closing entry has no presets");
        assert_eq!(
            e[1].presets,
            vec![("Resolution".to_string(), "2560x1440".to_string())]
        );
    }

    #[test]
    fn enabling_a_pack_leaves_the_rest_of_settings_untouched() {
        let out = graphic_packs::apply_to_settings(
            SETTINGS,
            "graphicPacks/downloadedGraphicPacks/Splatoon/Graphics/rules.txt",
            true,
            &[("Resolution".into(), "1920x1080".into())],
        );
        // The new pack is present with its preset.
        assert!(out.contains("Splatoon/Graphics/rules.txt"));
        assert!(out.contains("<preset>1920x1080</preset>"));
        // Pre-existing packs survive.
        assert!(out.contains("KirbyRainbow_InvisibleLineFix"));
        assert!(out.contains("<preset>2560x1440</preset>"));
        // Everything outside <GraphicPack> is untouched -- this is the user's
        // whole global config.
        assert!(out.contains("<language>0</language>"));
        assert!(out.contains("<fullscreen>false</fullscreen>"));
        assert!(out.trim_start().starts_with("<?xml"));
    }

    #[test]
    fn disabling_removes_only_that_entry() {
        let out = graphic_packs::apply_to_settings(
            SETTINGS,
            "graphicPacks/downloadedGraphicPacks/SkylandersGiants/Graphics/rules.txt",
            false,
            &[],
        );
        assert!(!out.contains("SkylandersGiants"));
        assert!(out.contains("KirbyRainbow_InvisibleLineFix"), "sibling removed");
        assert!(out.contains("<language>0</language>"));
    }

    #[test]
    fn changing_a_preset_replaces_rather_than_duplicates() {
        let out = graphic_packs::apply_to_settings(
            SETTINGS,
            "graphicPacks/downloadedGraphicPacks/SkylandersGiants/Graphics/rules.txt",
            true,
            &[("Resolution".into(), "1920x1080".into())],
        );
        assert_eq!(out.matches("SkylandersGiants").count(), 1, "duplicated entry");
        assert!(out.contains("<preset>1920x1080</preset>"));
        assert!(!out.contains("<preset>2560x1440</preset>"));
    }

    #[test]
    fn creates_the_section_when_absent() {
        let bare = "<?xml version=\"1.0\"?>\n<content>\n    <language>0</language>\n</content>";
        let out = graphic_packs::apply_to_settings(bare, "packs/x/rules.txt", true, &[]);
        assert!(out.contains("<GraphicPack>"));
        assert!(out.contains("packs/x/rules.txt"));
        assert!(out.contains("<language>0</language>"));
    }
}
