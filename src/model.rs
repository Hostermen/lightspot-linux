// model.rs — Shared data types used across the search backend.
//
// These structs are the "vocabulary" passed between the app-search,
// file-search, calculator, and search-aggregation modules. Keeping them
// in one place avoids circular imports between those modules.

/// A launchable application discovered from a `.desktop` file.
#[derive(Clone)]
pub struct AppEntry {
    /// The `.desktop` file name (e.g. `firefox.desktop`) — used as the id
    /// passed to `gtk-launch` to actually start the app.
    pub app_id: String,
    /// Human-readable name (the `Name=` field), shown in the UI.
    pub name: String,
    /// Optional freedesktop icon name from the `Icon=` field, if present.
    pub icon: Option<String>,
}

/// A single file path returned by `plocate`.
#[derive(Clone)]
pub struct FileHit {
    /// Absolute path of the matched file or directory.
    pub path: String,
}

/// A full-text content match returned by the Tantivy content index.
#[derive(Clone, Debug)]
pub struct ContentHit {
    /// Absolute path of the file whose contents matched.
    pub path: String,
    /// A short snippet of the matching text (single-line, length-capped).
    pub snippet: String,
}

/// What should happen when the user activates a result row.
/// Carried alongside display data so the UI/frontend knows how to act.
#[derive(Clone)]
pub enum Action {
    /// Launch the app with this `.desktop` id (via `gtk-launch`).
    LaunchApp(String),
    /// Open this file/path (via `xdg-open`).
    OpenFile(String),
    /// Copy this string to the clipboard (calculator result).
    CopyResult(String),
}

/// One row to display in the launcher results list.
#[derive(Clone)]
pub struct DisplayItem {
    /// Freedesktop icon name (or a calculator-specific marker).
    pub icon: String,
    /// Primary line shown to the user (app name, file name, or `= result`).
    pub title: String,
    /// Secondary line (e.g. "Application" or the file's directory).
    pub subtitle: String,
    /// The action to run when this row is activated.
    pub action: Action,
    /// True if this row is a content (full-text) match rather than a file hit.
    pub is_content: bool,
}
