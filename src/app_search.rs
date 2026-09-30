// app_search.rs — Discover launchable applications from `.desktop` files.
//
// The freedesktop.org spec says application launchers live as `*.desktop`
// files in a set of well-known directories (per-user, system, and anything
// in `$XDG_DATA_DIRS`). This module scans those directories, parses each
// `.desktop` file just enough to extract Name/Icon/Exec/Type/NoDisplay, and
// returns a deduplicated, alphabetically sorted list of `AppEntry` values
// that the search module then fuzzy-matches against the user's query.

use crate::model::AppEntry;
use std::collections::HashSet;   // deduplicates by .desktop file id
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Load and parse every visible `.desktop` application entry across all
/// application directories. Returns apps sorted by name (case-insensitive).
pub fn load_apps() -> Vec<AppEntry> {
    let mut seen = HashSet::new(); // tracks .desktop ids we've already added
    let mut apps = Vec::new();
    for dir in app_dirs() {         // iterate every application directory
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {                  // skip unreadable entries
                let p = e.path();
                // Only consider files ending in `.desktop`.
                if p.extension().map(|s| s == "desktop").unwrap_or(false) {
                    if let Some(app) = parse_desktop(&p) {
                        // The id is the file name (e.g. `firefox.desktop`).
                        let id = p.file_name().unwrap().to_string_lossy().to_string();
                        // First occurrence wins (user dir usually shadows system dir).
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
    // Deterministic ordering for the UI.
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

// ── Cached app list for the long-running daemon ────────────────────────
//
// The search daemon answers many queries over its lifetime; re-scanning and
// re-parsing every `.desktop` file on the system (160+ files) on each query
// is pure waste. We cache the parsed list and only refresh it at most once
// per TTL window, so newly installed apps show up within `APP_TTL` without
// paying the re-scan cost on every keystroke.

/// Process-global cache: (apps, when last refreshed).
static APP_CACHE: Mutex<Option<(Vec<AppEntry>, Instant)>> = Mutex::new(None);
/// How long a cached app list is considered fresh before a re-scan.
const APP_TTL: Duration = Duration::from_secs(30);

/// Like `load_apps` but served from a process-wide cache refreshed at most
/// every `APP_TTL`. Used by the daemon's search server; the one-shot
/// `--search` CLI keeps using the uncached `load_apps` (a fresh process
/// pays the scan once anyway).
pub fn load_apps_cached() -> Vec<AppEntry> {
    let mut guard = APP_CACHE.lock().unwrap();
    let need_refresh = match guard.as_ref() {
        Some((_, t)) => t.elapsed() >= APP_TTL,
        None => true,
    };
    if need_refresh {
        let apps = load_apps();
        *guard = Some((apps.clone(), Instant::now()));
        apps
    } else {
        guard.as_ref().unwrap().0.clone()
    }
}

/// Build the ordered list of directories to scan for `.desktop` files.
/// User-local dir comes first so it shadows system-wide entries on dedup.
fn app_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    // Per-user applications directory (~/.local/share/applications).
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{}/.local/share/applications", home));
    }
    // Everything in XDG_DATA_DIRS, falling back to the standard two.
    let xdg = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/share/applications:/usr/local/share/applications".to_string());
    for d in xdg.split(':') {
        if !d.is_empty() {
            dirs.push(d.to_string());
        }
    }
    // Guarantee the most common system dir is always covered.
    if !dirs.iter().any(|d| d == "/usr/share/applications") {
        dirs.push("/usr/share/applications".to_string());
    }
    dirs
}

// Parsed fields we actually care about from a `.desktop` file.
struct Parsed {
    name: String,
    icon: Option<String>,
}

/// Parse one `.desktop` file. Returns `None` if the entry is hidden,
/// not an Application, or has no `Exec=` line.
fn parse_desktop(p: &Path) -> Option<Parsed> {
    let content = fs::read_to_string(p).ok()?;
    let mut in_entry = false;   // true while inside the [Desktop Entry] group
    let mut name: Option<String> = None;
    let mut has_exec = false;   // an Application must have an Exec=
    let mut icon: Option<String> = None;
    let mut nodisplay = false;  // NoDisplay=true → hide from menus/launchers
    let mut typ: Option<String> = None;
    for line in content.lines() {
        // A line starting with '[' begins a new group header.
        if let Some(br) = line.strip_prefix('[') {
            // We only read keys from the [Desktop Entry] group.
            in_entry = br.trim_end_matches(']').trim() == "Desktop Entry";
            continue;
        }
        if !in_entry {
            continue;          // ignore keys outside the main group
        }
        if line.starts_with('#') || line.is_empty() {
            continue;          // comments and blank lines
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let val = line[eq + 1..].trim();
            match key {
                // First Name= wins (locale-specific Name[xx]= lines are ignored).
                "Name" => {
                    if name.is_none() {
                        name = Some(val.to_string());
                    }
                }
                "Exec" => has_exec = true,            // presence is enough
                "Icon" => icon = Some(val.to_string()), // store raw icon name
                "NoDisplay" => nodisplay = val.eq_ignore_ascii_case("true"),
                "Type" => typ = Some(val.to_string()),
                _ => {}                                // ignore unknown keys
            }
        }
    }
    // Filter out hidden / non-application / exec-less entries.
    if nodisplay {
        return None;
    }
    if typ.as_deref() != Some("Application") {
        return None;
    }
    if !has_exec {
        return None;
    }
    let name = name?;          // an app without a Name is unusable
    Some(Parsed { name, icon })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_apps_on_this_system() {
        let apps = load_apps();
        assert!(!apps.is_empty(), "expected to find at least one application");
        // No duplicate app_ids (the dedup set did its job).
        let mut ids: Vec<_> = apps.iter().map(|a| &a.app_id).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate app ids found");
        // Every app must have a non-empty display name.
        assert!(apps.iter().all(|a| !a.name.is_empty()));
    }
}
