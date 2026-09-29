#[derive(Clone)]
pub struct AppEntry {
    pub app_id: String,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Clone)]
pub struct FileHit {
    pub path: String,
}

#[derive(Clone)]
pub enum Action {
    LaunchApp(String),
    OpenFile(String),
    CopyResult(String),
}

#[derive(Clone)]
pub struct DisplayItem {
    pub icon: String,
    pub title: String,
    pub subtitle: String,
    pub action: Action,
}
