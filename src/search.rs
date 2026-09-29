// search.rs — Aggregate and serialize search results.
//
// This module ties together the three result sources (calculator, app
// search, file search) into a single ordered `Vec<DisplayItem>`, and
// provides a hand-rolled JSON serializer (`search_json`) that the
// Electron frontend consumes via `spotlight-files --search <query>`.

use crate::app_search;
use crate::calculator;
use crate::content_index;
use crate::file_search;
use crate::model::{Action, AppEntry, ContentHit, DisplayItem, FileHit};
use std::path::Path;

/// Map a file path to a freedesktop MIME-type icon name based on its
/// extension. The Electron frontend resolves these via GTK3's icon
/// theme to show proper file-type icons (Python, PDF, C, etc.).
fn file_icon(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        // Programming languages
        "py" => "text-x-python",
        "c" => "text-x-csrc",
        "h" => "text-x-chdr",
        "cpp" | "cc" | "cxx" => "text-x-c++src",
        "hpp" | "hh" | "hxx" => "text-x-c++hdr",
        "rs" => "text-x-rust",
        "go" => "text-x-go",
        "js" | "mjs" => "application-javascript",
        "ts" => "text-typescript",
        "java" => "text-x-java",
        "rb" => "text-x-ruby",
        "php" => "text-x-php",
        "sh" | "bash" => "application-x-shellscript",
        "pl" => "text-x-perl",
        "lua" => "text-x-lua",
        "asm" | "s" => "text-x-asm",
        "swift" => "text-x-swift",
        "kt" | "kts" => "text-x-kotlin",
        "scala" => "text-x-scala",
        "cs" => "text-x-csharp",
        // Web / markup
        "html" | "htm" => "text-html",
        "css" | "scss" | "sass" => "text-css",
        "xml" => "text-xml",
        "md" | "markdown" => "text-markdown",
        "json" => "application-json",
        "yaml" | "yml" => "text-x-yaml",
        "toml" => "text-x-toml",
        "ini" | "cfg" | "conf" => "text-x-generic",
        // Documents
        "pdf" => "application-pdf",
        "doc" | "docx" => "application-msword",
        "odt" => "application-vnd.oasis.opendocument.text",
        "xls" | "xlsx" => "application-vnd.ms-excel",
        "ods" => "application-vnd.oasis.opendocument.spreadsheet",
        "ppt" | "pptx" => "application-vnd.ms-powerpoint",
        "odp" => "application-vnd.oasis.opendocument.presentation",
        "tex" | "latex" => "text-x-tex",
        "epub" => "application-epub+zip",
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tiff" | "tif" | "ico" => "image-x-generic",
        "svg" => "image-svg+xml",
        // Audio
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "wma" => "audio-x-generic",
        // Video
        "mp4" | "mkv" | "avi" | "webm" | "mov" | "wmv" | "flv" => "video-x-generic",
        // Archives
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => "application-x-archive",
        "deb" => "application-x-deb",
        "rpm" => "application-x-rpm",
        "appimage" => "application-x-executable",
        // Data
        "csv" => "text-csv",
        "tsv" => "text-x-generic",
        "db" | "sqlite" | "sqlite3" => "application-x-sqlite3",
        // Executables / libraries
        "exe" | "bin" => "application-x-executable",
        "so" | "dll" => "application-x-sharedlib",
        // Log
        "log" => "text-x-log",
        // Default: generic text file
        _ => "text-x-generic",
    }
    .to_string()
}

/// Combine apps + file hits for a query into one display list.
///
/// Ordering:
///   1. Calculator result (if the query looks like math)
///   2. Fuzzy-matched applications (best score first, then alphabetical)
///   3. plocate file hits
///   4. Full-text content matches (with snippet)
///
/// The final list is capped at 30 rows for a snappy UI.
pub fn build_results(
    query: &str,
    apps: &[AppEntry],
    files: &[FileHit],
    content: &[ContentHit],
) -> Vec<DisplayItem> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();             // nothing to show for empty input
    }

    let mut items = Vec::new();

    // ── Calculator ────────────────────────────────────────────────
    // If the query looks like math, evaluate it and offer "= result".
    if calculator::looks_like_math(q) {
        if let Some(r) = calculator::eval(q) {
            let f = calculator::format_result(r);
            items.push(DisplayItem {
                icon: "accessories-calculator".to_string(),
                title: format!("= {}", f),                         // e.g. "= 8"
                subtitle: "Calculator  ·  Enter to copy".to_string(),
                action: Action::CopyResult(f),                     // Enter → clipboard
                is_content: false,
            });
        }
    }

    // ── Applications ─────────────────────────────────────────────
    // Fuzzy-match the query against each app's display name; keep pairs of
    // (score, app) so we can rank by match quality then by name.
    let mut app_matches: Vec<(i32, &AppEntry)> = apps
        .iter()
        .filter_map(|a| fuzzy(q, &a.name).map(|s| (s, a)))
        .collect();
    // Higher score first; ties broken alphabetically by app name.
    app_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    // Show at most 8 apps to leave room for file + content results.
    for (_, a) in app_matches.iter().take(8) {
        items.push(DisplayItem {
            // Fall back to a generic icon if the .desktop had no Icon=.
            icon: a
                .icon
                .clone()
                .unwrap_or_else(|| "application-x-executable".to_string()),
            title: a.name.clone(),
            subtitle: "Application".to_string(),
            action: Action::LaunchApp(a.app_id.clone()),
            is_content: false,
        });
    }

    // ── Files ────────────────────────────────────────────────────
    // Each file hit becomes one row: the base name as the title, the parent
    // directory as the subtitle, and `xdg-open <path>` as the action.
    for f in files.iter().take(8) {
        let name = Path::new(&f.path)
            .file_name()                                  // last path component
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| f.path.clone());
        let dir = Path::new(&f.path)
            .parent()                                     // everything but the name
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        items.push(DisplayItem {
            icon: file_icon(&f.path),
            title: name,
            subtitle: dir,
            action: Action::OpenFile(f.path.clone()),
            is_content: false,
        });
    }

    // ── Content matches ─────────────────────────────────────────
    // Files whose *contents* match the query (via the Tantivy index). The
    // snippet from the matching region is shown as the subtitle so the user
    // can see the context without opening the file.
    for c in content.iter().take(12) {
        let name = Path::new(&c.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| c.path.clone());
        items.push(DisplayItem {
            icon: file_icon(&c.path),
            title: name,
            subtitle: c.snippet.clone(),
            action: Action::OpenFile(c.path.clone()),
            is_content: true,
        });
    }

    // Hard cap so very large result sets never bog down the UI.
    items.truncate(30);
    items
}

/// Convenience wrapper: load apps + run file + content search, then build results.
pub fn search(query: &str) -> Vec<DisplayItem> {
    let apps = app_search::load_apps();                 // parse .desktop files
    // Only hit plocate / content index for queries of >= 2 chars.
    let (file_hits, content_hits) = if query.trim().len() >= 2 {
        (
            file_search::search_files(query, 100),
            content_index::search_content(query, 20),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    build_results(query, &apps, &file_hits, &content_hits)
}

/// Escape a string for safe inclusion inside a JSON string literal.
/// We hand-roll JSON (no serde) to keep the binary tiny, so we must escape.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),     // quote → \"
            '\\' => out.push_str("\\\\"),   // backslash → \\
            '\n' => out.push_str("\\n"),     // newline
            '\t' => out.push_str("\\t"),     // tab
            '\r' => out.push_str("\\r"),     // carriage return
            // Other control characters → \uXXXX.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),                // normal character, copied verbatim
        }
    }
    out
}

/// Map an `Action` to the string the frontend expects in `action_type`.
fn action_type(a: &Action) -> &'static str {
    match a {
        Action::LaunchApp(_) => "launch_app",
        Action::OpenFile(_) => "open_file",
        Action::CopyResult(_) => "copy",
    }
}

/// The payload string for an action — the app id, file path, or text to copy.
fn action_data(a: &Action) -> &str {
    match a {
        Action::LaunchApp(id) => id,
        Action::OpenFile(p) => p,
        Action::CopyResult(t) => t,
    }
}

/// Produce the JSON the Electron frontend renders. Shape:
///   {"items":[{"title":..,"subtitle":..,"icon":..,"action_type":..,"action_data":..}, ...]}
pub fn search_json(query: &str) -> String {
    let items = search(query);
    // Serialize each item as its own JSON object.
    let parts: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                r#"{{"title":"{}","subtitle":"{}","icon":"{}","action_type":"{}","action_data":"{}","is_content":{}}}"#,
                escape_json(&i.title),
                escape_json(&i.subtitle),
                escape_json(&i.icon),
                action_type(&i.action),
                escape_json(action_data(&i.action)),
                i.is_content,
            )
        })
        .collect();
    // Join all items into the outer object.
    format!(r#"{{"items":[{}]}}"#, parts.join(","))
}

/// Fuzzy match: does `query` appear in `target` as a subsequence (case-insensitive)?
/// Returns `Some(score)` if it matches, where higher score = better match.
/// Boundary starts of words and consecutive matches score higher.
fn fuzzy(query: &str, target: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);                 // empty query "matches" everything, score 0
    }
    let mut qi = 0usize;                // cursor into the query
    let mut score = 0i32;
    let mut prev_matched = false;       // was the previous target char a match?
    for (i, &tc) in t.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            // A word boundary (start, or after a non-alphanumeric char) is a strong match.
            let boundary = i == 0 || !t[i - 1].is_alphanumeric();
            if boundary {
                score += 10;            // matched at the start of a word
            } else if prev_matched {
                score += 5;             // continuation of a run of matches
            }
            score += 1;                 // base score for any match
            qi += 1;                    // consume one query character
            prev_matched = true;
        } else {
            prev_matched = false;       // broke a run
        }
    }
    // Only a match if the entire query was consumed.
    if qi == q.len() {
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_basic() {
        // Subsequence "fir" matches "Firefox".
        assert!(fuzzy("fir", "Firefox").is_some());
        // "xyz" is not a subsequence of "Firefox".
        assert!(fuzzy("xyz", "Firefox").is_none());
    }

    #[test]
    fn json_empty_query() {
        let j = search_json("");
        assert!(j.contains("\"items\":[]"));          // empty query → no items
    }

    #[test]
    fn json_has_structure() {
        let j = search_json("firefox");
        // Must be a well-formed wrapper object.
        assert!(j.starts_with("{\"items\":["));
        assert!(j.ends_with("]}"));
    }

    #[test]
    fn json_escapes_quotes() {
        // Embedded quotes must be backslash-escaped.
        let j = escape_json(r#"he said "hi""#);
        assert!(j.contains(r#"\"hi\""#));
    }
}
