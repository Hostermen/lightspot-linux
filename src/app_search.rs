use crate::model::AppEntry;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn load_apps() -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in app_dirs() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map(|s| s == "desktop").unwrap_or(false) {
                    if let Some(app) = parse_desktop(&p) {
                        let id = p.file_name().unwrap().to_string_lossy().to_string();
                        if seen.insert(id.clone()) {
                            apps.push(AppEntry {
                                app_id: id,
                                name: app.name,
                                icon: app.icon,
                            });
                        }
                    }
                }
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn app_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{}/.local/share/applications", home));
    }
    let xdg = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/share/applications:/usr/local/share/applications".to_string());
    for d in xdg.split(':') {
        if !d.is_empty() {
            dirs.push(d.to_string());
        }
    }
    if !dirs.iter().any(|d| d == "/usr/share/applications") {
        dirs.push("/usr/share/applications".to_string());
    }
    dirs
}

struct Parsed {
    name: String,
    icon: Option<String>,
}

fn parse_desktop(p: &Path) -> Option<Parsed> {
    let content = fs::read_to_string(p).ok()?;
    let mut in_entry = false;
    let mut name: Option<String> = None;
    let mut has_exec = false;
    let mut icon: Option<String> = None;
    let mut nodisplay = false;
    let mut typ: Option<String> = None;
    for line in content.lines() {
        if let Some(br) = line.strip_prefix('[') {
            in_entry = br.trim_end_matches(']').trim() == "Desktop Entry";
            continue;
        }
        if !in_entry {
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let val = line[eq + 1..].trim();
            match key {
                "Name" => {
                    if name.is_none() {
                        name = Some(val.to_string());
                    }
                }
                "Exec" => has_exec = true,
                "Icon" => icon = Some(val.to_string()),
                "NoDisplay" => nodisplay = val.eq_ignore_ascii_case("true"),
                "Type" => typ = Some(val.to_string()),
                _ => {}
            }
        }
    }
    if nodisplay {
        return None;
    }
    if typ.as_deref() != Some("Application") {
        return None;
    }
    if !has_exec {
        return None;
    }
    let name = name?;
    Some(Parsed { name, icon })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_apps_on_this_system() {
        let apps = load_apps();
        assert!(!apps.is_empty(), "expected to find at least one application");
        // No duplicate app_ids.
        let mut ids: Vec<_> = apps.iter().map(|a| &a.app_id).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate app ids found");
        assert!(apps.iter().all(|a| !a.name.is_empty()));
    }
}

