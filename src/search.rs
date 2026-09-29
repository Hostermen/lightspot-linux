use crate::app_search;
use crate::calculator;
use crate::file_search;
use crate::model::{Action, AppEntry, DisplayItem, FileHit};
use std::path::Path;

pub fn build_results(query: &str, apps: &[AppEntry], files: &[FileHit]) -> Vec<DisplayItem> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();

    if calculator::looks_like_math(q) {
        if let Some(r) = calculator::eval(q) {
            let f = calculator::format_result(r);
            items.push(DisplayItem {
                icon: "accessories-calculator".to_string(),
                title: format!("= {}", f),
                subtitle: "Calculator  ·  Enter to copy".to_string(),
                action: Action::CopyResult(f),
            });
        }
    }

    let mut app_matches: Vec<(i32, &AppEntry)> = apps
        .iter()
        .filter_map(|a| fuzzy(q, &a.name).map(|s| (s, a)))
        .collect();
    app_matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    for (_, a) in app_matches.iter().take(8) {
        items.push(DisplayItem {
            icon: a
                .icon
                .clone()
                .unwrap_or_else(|| "application-x-executable".to_string()),
            title: a.name.clone(),
            subtitle: "Application".to_string(),
            action: Action::LaunchApp(a.app_id.clone()),
        });
    }

    for f in files.iter().take(20) {
        let name = Path::new(&f.path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| f.path.clone());
        let dir = Path::new(&f.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        items.push(DisplayItem {
            icon: "text-x-generic".to_string(),
            title: name,
            subtitle: dir,
            action: Action::OpenFile(f.path.clone()),
        });
    }

    items.truncate(30);
    items
}

pub fn search(query: &str) -> Vec<DisplayItem> {
    let apps = app_search::load_apps();
    let file_hits = if query.trim().len() >= 2 {
        file_search::search_files(query, 100)
    } else {
        Vec::new()
    };
    build_results(query, &apps, &file_hits)
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn action_type(a: &Action) -> &'static str {
    match a {
        Action::LaunchApp(_) => "launch_app",
        Action::OpenFile(_) => "open_file",
        Action::CopyResult(_) => "copy",
    }
}

fn action_data(a: &Action) -> &str {
    match a {
        Action::LaunchApp(id) => id,
        Action::OpenFile(p) => p,
        Action::CopyResult(t) => t,
    }
}

pub fn search_json(query: &str) -> String {
    let items = search(query);
    let parts: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                r#"{{"title":"{}","subtitle":"{}","icon":"{}","action_type":"{}","action_data":"{}"}}"#,
                escape_json(&i.title),
                escape_json(&i.subtitle),
                escape_json(&i.icon),
                action_type(&i.action),
                escape_json(action_data(&i.action)),
            )
        })
        .collect();
    format!(r#"{{"items":[{}]}}"#, parts.join(","))
}

fn fuzzy(query: &str, target: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev_matched = false;
    for (i, &tc) in t.iter().enumerate() {
        if qi < q.len() && tc == q[qi] {
            let boundary = i == 0 || !t[i - 1].is_alphanumeric();
            if boundary {
                score += 10;
            } else if prev_matched {
                score += 5;
            }
            score += 1;
            qi += 1;
            prev_matched = true;
        } else {
            prev_matched = false;
        }
    }
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
        assert!(fuzzy("fir", "Firefox").is_some());
        assert!(fuzzy("xyz", "Firefox").is_none());
    }

    #[test]
    fn json_empty_query() {
        let j = search_json("");
        assert!(j.contains("\"items\":[]"));
    }

    #[test]
    fn json_has_structure() {
        let j = search_json("firefox");
        assert!(j.starts_with("{\"items\":["));
        assert!(j.ends_with("]}"));
    }

    #[test]
    fn json_escapes_quotes() {
        let j = escape_json(r#"he said "hi""#);
        assert!(j.contains(r#"\"hi\""#));
    }
}
