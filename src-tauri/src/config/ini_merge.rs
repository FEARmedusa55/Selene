//! Line-preserving INI merge.
//!
//! Both Cemu and Eden keep per-game config files that the user may already have
//! populated by hand — this machine has a Cemu profile holding a PS4 controller
//! binding, and three Eden per-game configs. Rewriting those files from a
//! struct would silently discard every setting this app does not model.
//!
//! So edits are applied as *line surgery* on the original text: keys we manage
//! are updated in place, keys we do not are copied through untouched, and
//! comments, ordering and blank lines survive. A key that does not exist yet is
//! appended to its section, and a section that does not exist is appended to
//! the file.

use std::collections::BTreeMap;

/// One edit: `(section, key, value)`. `None` removes the key.
pub type Edit = (String, String, Option<String>);

fn section_of(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
        Some(&t[1..t.len() - 1])
    } else {
        None
    }
}

fn key_of(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') || t.starts_with(';') || t.starts_with('[') || t.is_empty() {
        return None;
    }
    t.split('=').next().map(str::trim)
}

/// Apply `edits` to `original`, returning the new file text.
///
/// Key comparison is case-insensitive: Cemu writes `cpuMode`, and a user may
/// have typed `cpumode`.
pub fn merge(original: &str, edits: &[Edit]) -> String {
    // Group edits by section, preserving a stable order.
    let mut pending: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();
    for (section, key, value) in edits {
        pending
            .entry(section.clone())
            .or_default()
            .push((key.clone(), value.clone()));
    }

    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    // Sections seen in the file, so we know which edits still need appending.
    let mut seen_sections: Vec<String> = Vec::new();

    let lines: Vec<&str> = original.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        if let Some(sec) = section_of(line) {
            // Before leaving a section, append any of its keys that were not
            // found in it.
            flush_missing(&mut out, &current, &mut pending);
            current = sec.to_string();
            seen_sections.push(current.clone());
            out.push(line.to_string());
            i += 1;
            continue;
        }

        if let Some(key) = key_of(line) {
            if let Some(entries) = pending.get_mut(&current) {
                if let Some(pos) = entries
                    .iter()
                    .position(|(k, _)| k.eq_ignore_ascii_case(key))
                {
                    let (k, v) = entries.remove(pos);
                    match v {
                        // Replace in place, keeping the original indentation.
                        Some(val) => {
                            let indent: String =
                                line.chars().take_while(|c| c.is_whitespace()).collect();
                            out.push(format!("{indent}{k} = {val}"));
                        }
                        // Removal: drop the line entirely.
                        None => {}
                    }
                    i += 1;
                    continue;
                }
            }
        }

        out.push(line.to_string());
        i += 1;
    }

    // End of file: flush the final section, then any wholly new sections.
    flush_missing(&mut out, &current, &mut pending);

    for (section, entries) in pending {
        let additions: Vec<_> = entries.into_iter().filter(|(_, v)| v.is_some()).collect();
        if additions.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        out.push(format!("[{section}]"));
        for (k, v) in additions {
            out.push(format!("{k} = {}", v.unwrap()));
        }
    }

    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Append keys belonging to `section` that were never found within it.
fn flush_missing(
    out: &mut Vec<String>,
    section: &str,
    pending: &mut BTreeMap<String, Vec<(String, Option<String>)>>,
) {
    let Some(entries) = pending.get_mut(section) else {
        return;
    };
    let additions: Vec<_> = entries
        .iter()
        .filter(|(_, v)| v.is_some())
        .cloned()
        .collect();
    if additions.is_empty() {
        entries.clear();
        return;
    }

    // Insert before trailing blank lines so the section stays visually tidy.
    let mut insert_at = out.len();
    while insert_at > 0 && out[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }
    for (k, v) in additions.into_iter().rev() {
        out.insert(insert_at, format!("{k} = {}", v.unwrap()));
    }
    entries.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(s: &str, k: &str, v: Option<&str>) -> Edit {
        (s.into(), k.into(), v.map(str::to_string))
    }

    /// The real Cemu profile on this machine. Losing `controller1 = ps4` here
    /// would mean silently unbinding the user's gamepad.
    const CEMU_PROFILE: &str = "[General]\nloadSharedLibraries = true\nstartWithPadView = false\n\n[CPU]\ncpuMode = 4\nthreadQuantum = 45000\n\n[Graphics]\naccurateShaderMul = 1\nprecompiledShaders = 0\n\n[Controller]\ncontroller1 =  ps4\n";

    #[test]
    fn updates_a_key_without_touching_the_rest() {
        let out = merge(CEMU_PROFILE, &[edit("CPU", "cpuMode", Some("1"))]);
        assert!(out.contains("cpuMode = 1"));
        assert!(!out.contains("cpuMode = 4"));
        // Everything else survives.
        assert!(out.contains("controller1 =  ps4"), "user controller lost:\n{out}");
        assert!(out.contains("threadQuantum = 45000"));
        assert!(out.contains("loadSharedLibraries = true"));
        assert!(out.contains("precompiledShaders = 0"));
    }

    #[test]
    fn adds_a_key_to_an_existing_section() {
        let out = merge(CEMU_PROFILE, &[edit("Graphics", "vsync", Some("2"))]);
        assert!(out.contains("vsync = 2"));
        assert!(out.contains("accurateShaderMul = 1"));
        // Must land inside [Graphics], before [Controller].
        let g = out.find("[Graphics]").unwrap();
        let c = out.find("[Controller]").unwrap();
        let v = out.find("vsync = 2").unwrap();
        assert!(g < v && v < c, "vsync landed outside [Graphics]:\n{out}");
    }

    #[test]
    fn adds_a_missing_section_at_the_end() {
        let out = merge(CEMU_PROFILE, &[edit("Audio", "volume", Some("50"))]);
        assert!(out.contains("[Audio]"));
        assert!(out.contains("volume = 50"));
        assert!(out.contains("controller1 =  ps4"));
    }

    #[test]
    fn removes_a_key_when_value_is_none() {
        let out = merge(CEMU_PROFILE, &[edit("CPU", "cpuMode", None)]);
        assert!(!out.contains("cpuMode"));
        assert!(out.contains("threadQuantum = 45000"), "sibling key lost");
    }

    #[test]
    fn preserves_comments_and_blank_lines() {
        let src = "# hand written\n\n[General]\n# why this is set\nfoo = 1\n\n[Other]\nbar = 2\n";
        let out = merge(&src, &[edit("General", "foo", Some("9"))]);
        assert!(out.contains("# hand written"));
        assert!(out.contains("# why this is set"));
        assert!(out.contains("foo = 9"));
        assert!(out.contains("bar = 2"));
    }

    #[test]
    fn key_matching_is_case_insensitive() {
        let out = merge(CEMU_PROFILE, &[edit("CPU", "CPUMODE", Some("7"))]);
        assert_eq!(out.matches("cpuMode").count() + out.matches("CPUMODE").count(), 1);
        assert!(out.contains("= 7"));
    }

    #[test]
    fn handles_edens_backslash_suffixed_keys() {
        // Eden marks inheritance with `key\use_global`; the backslash must not
        // confuse key parsing.
        let src = "[Core]\nuse_multi_core\\use_global=true\n";
        let out = merge(
            src,
            &[
                edit("Core", "use_multi_core\\use_global", Some("false")),
                edit("Core", "use_multi_core", Some("true")),
            ],
        );
        assert!(out.contains("use_multi_core\\use_global = false"), "{out}");
        assert!(out.contains("\nuse_multi_core = true"), "{out}");
    }

    #[test]
    fn empty_input_produces_valid_output() {
        let out = merge("", &[edit("General", "a", Some("1"))]);
        assert!(out.contains("[General]"));
        assert!(out.contains("a = 1"));
    }

    #[test]
    fn no_edits_is_a_faithful_round_trip() {
        let out = merge(CEMU_PROFILE, &[]);
        assert_eq!(out.trim_end(), CEMU_PROFILE.trim_end());
    }
}
