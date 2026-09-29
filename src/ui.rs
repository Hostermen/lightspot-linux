use crate::app_search;
use crate::file_search;
use crate::model::{Action, AppEntry, FileHit};
use crate::search::build_results;
use gtk4::glib;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{
    AccessibleRole, Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, Entry,
    EventControllerKey, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode,
};
use std::cell::{Cell, RefCell};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CSS: &str = r#"
window {
    background-color: rgba(22, 22, 28, 0.42);
    border-radius: 22px;
    border: 1px solid rgba(255, 255, 255, 0.18);
    box-shadow: 0 30px 90px rgba(0, 0, 0, 0.65), inset 0 1px 1px rgba(255, 255, 255, 0.10);
}
.bar { padding: 18px 22px; }
.magnifier { color: rgba(255, 255, 255, 0.65); }
.search-entry {
    font-size: 23px;
    background: transparent;
    color: #ffffff;
    border: none;
    box-shadow: none;
    caret-color: #ffffff;
}
.search-entry:focus { border: none; box-shadow: none; outline: none; }
.search-entry text placeholder { color: rgba(255, 255, 255, 0.42); }
.separator { background: rgba(255, 255, 255, 0.12); min-height: 1px; }
.results { background: transparent; }
row {
    border-radius: 10px;
    padding: 9px 10px;
    margin: 3px 10px;
}
row:selected { background-color: #0a84ff; }
row:hover:not(:selected) { background-color: rgba(255, 255, 255, 0.08); }
.title { color: #ffffff; font-weight: 600; }
row:selected .title { color: #ffffff; }
.subtitle { color: rgba(255, 255, 255, 0.42); font-size: 12px; }
row:selected .subtitle { color: rgba(255, 255, 255, 0.88); }
scrolledwindow { background: transparent; }
"#;

struct State {
    apps: Vec<AppEntry>,
    file_hits: Vec<FileHit>,
    query: String,
    actions: Vec<Action>,
}

pub struct Launcher {
    window: ApplicationWindow,
    entry: Entry,
    listbox: ListBox,
    state: Rc<RefCell<State>>,
    just_shown: Rc<Cell<Option<Instant>>>,
}

pub fn build(app: &Application) -> Rc<Launcher> {
    let apps = app_search::load_apps();
    let state = Rc::new(RefCell::new(State {
        apps,
        file_hits: Vec::new(),
        query: String::new(),
        actions: Vec::new(),
    }));

    let window = ApplicationWindow::new(app);
    window.set_title(Some("Spotlight"));
    window.set_default_size(680, -1);
    window.set_resizable(false);
    window.set_decorated(false);

    let css = CssProvider::new();
    css.load_from_data(CSS);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let root = GtkBox::new(Orientation::Vertical, 0);

    // Top bar: magnifier + search entry
    let bar = GtkBox::new(Orientation::Horizontal, 12);
    bar.add_css_class("bar");
    bar.set_halign(Align::Fill);
    let mag = Image::from_icon_name("system-search");
    mag.set_pixel_size(22);
    mag.add_css_class("magnifier");
    let entry = Entry::new();
    entry.set_placeholder_text(Some("Search apps, files, or calculate…"));
    entry.add_css_class("search-entry");
    entry.set_hexpand(true);
    entry.set_halign(Align::Fill);
    entry.set_accessible_role(AccessibleRole::SearchBox);
    bar.append(&mag);
    bar.append(&entry);
    root.append(&bar);

    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.add_css_class("separator");
    root.append(&sep);

    let scrolled = ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(PolicyType::Never);
    scrolled.set_vscrollbar_policy(PolicyType::Automatic);
    scrolled.set_min_content_height(0);
    scrolled.set_max_content_height(380);
    scrolled.set_propagate_natural_height(true);
    let listbox = ListBox::new();
    listbox.set_selection_mode(SelectionMode::Single);
    listbox.set_activate_on_single_click(true);
    listbox.add_css_class("results");
    listbox.set_accessible_role(AccessibleRole::ListBox);
    scrolled.set_child(Some(&listbox));
    root.append(&scrolled);

    window.set_child(Some(&root));

    // --- file-search worker channel + main-thread poll ---
    let (fs_tx, fs_rx) = mpsc::channel::<Vec<FileHit>>();
    let gen = Rc::new(Cell::new(0u64));
    {
        let state = state.clone();
        let listbox = listbox.clone();
        let fs_tx = fs_tx.clone();
        let gen = gen.clone();
        let entry_c = entry.clone();
        entry.connect_changed(move |_| {
            let text = entry_c.text().to_string();
            {
                let mut s = state.borrow_mut();
                s.query = text.clone();
                s.file_hits.clear();
            }
            rebuild(&state, &listbox);
            let g = gen.get().wrapping_add(1);
            gen.set(g);
            if text.trim().len() >= 2 {
                let s = fs_tx.clone();
                let gen2 = gen.clone();
                glib::timeout_add_local_once(Duration::from_millis(150), move || {
                    if gen2.get() == g {
                        let q = text.clone();
                        std::thread::spawn(move || {
                            let hits = file_search::search_files(&q, 100);
                            let _ = s.send(hits);
                        });
                    }
                });
            }
        });
    }
    {
        let state = state.clone();
        let listbox = listbox.clone();
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let mut latest: Option<Vec<FileHit>> = None;
            while let Ok(hits) = fs_rx.try_recv() {
                latest = Some(hits);
            }
            if let Some(hits) = latest {
                {
                    let mut s = state.borrow_mut();
                    s.file_hits = hits;
                }
                rebuild(&state, &listbox);
            }
            glib::ControlFlow::Continue
        });
    }

    let just_shown = Rc::new(Cell::new(None::<Instant>));

    // --- keyboard navigation (on the entry) ---
    let key = EventControllerKey::new();
    let state_k = state.clone();
    let listbox_k = listbox.clone();
    let window_k = window.clone();
    let app_k = app.clone();
    key.connect_key_pressed(move |_, keyval, _, mods| {
        use gtk4::gdk::Key;
        if keyval == Key::q && mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK) {
            app_k.quit();
            return glib::Propagation::Stop;
        }
        match keyval {
            Key::Escape => {
                window_k.set_visible(false);
                glib::Propagation::Stop
            }
            Key::Down | Key::Tab => {
                if let Some(first) = first_row(&listbox_k) {
                    listbox_k.select_row(Some(&first));
                    first.grab_focus();
                }
                glib::Propagation::Stop
            }
            Key::Return | Key::KP_Enter => {
                let s = state_k.borrow();
                if let Some(action) = s.actions.first() {
                    let action = action.clone();
                    drop(s);
                    run_action(&action);
                    window_k.set_visible(false);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    entry.add_controller(key);

    // --- row activation ---
    let state_a = state.clone();
    let window_a = window.clone();
    listbox.connect_row_activated(move |_, row| {
        let idx = row.index() as usize;
        let s = state_a.borrow();
        if let Some(action) = s.actions.get(idx) {
            let action = action.clone();
            drop(s);
            run_action(&action);
            window_a.set_visible(false);
        }
    });

    // --- keyboard navigation (on the list, Up/Esc returns to entry) ---
    let key2 = EventControllerKey::new();
    let listbox_u = listbox.clone();
    let entry_u = entry.clone();
    key2.connect_key_pressed(move |_, keyval, _, _| {
        use gtk4::gdk::Key;
        match keyval {
            Key::Up => {
                let selected = listbox_u.selected_row();
                if let Some(row) = selected {
                    if row.index() == 0 {
                        entry_u.grab_focus();
                        listbox_u.select_row(None::<&ListBoxRow>);
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                } else {
                    glib::Propagation::Proceed
                }
            }
            Key::Escape => {
                entry_u.grab_focus();
                glib::Propagation::Stop
            }
            Key::Return | Key::KP_Enter => glib::Propagation::Proceed,
            _ => glib::Propagation::Proceed,
        }
    });
    listbox.add_controller(key2);

    // --- hide when the window loses focus (click outside / app launched) ---
    let win_hide = window.clone();
    let js = just_shown.clone();
    window.connect_notify_local(Some("is-active"), move |w, _| {
        if !w.is_active() && w.is_visible() {
            if let Some(t) = js.get() {
                if t.elapsed() < Duration::from_millis(150) {
                    return;
                }
            }
            win_hide.set_visible(false);
        }
    });

    Rc::new(Launcher {
        window,
        entry,
        listbox,
        state,
        just_shown,
    })
}

impl Launcher {
    pub fn show_window(&self) {
        self.entry.set_text("");
        {
            let mut s = self.state.borrow_mut();
            s.query.clear();
            s.file_hits.clear();
        }
        rebuild(&self.state, &self.listbox);
        self.window.set_visible(true);
        self.window.present();
        self.entry.grab_focus();
        self.just_shown.set(Some(Instant::now()));
    }

    pub fn hide_window(&self) {
        self.window.set_visible(false);
    }

    pub fn toggle(&self) {
        if self.window.is_visible() {
            self.hide_window();
        } else {
            self.show_window();
        }
    }
}

fn first_row(listbox: &ListBox) -> Option<ListBoxRow> {
    let child = listbox.first_child()?;
    child.dynamic_cast_ref::<ListBoxRow>().cloned()
}

fn rebuild(state: &Rc<RefCell<State>>, listbox: &ListBox) {
    let items = {
        let s = state.borrow();
        build_results(&s.query, &s.apps, &s.file_hits)
    };

    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let mut actions = Vec::with_capacity(items.len());
    for item in &items {
        let row_box = GtkBox::new(Orientation::Horizontal, 12);
        row_box.set_margin_start(10);
        row_box.set_margin_end(10);

        let icon = Image::from_icon_name(&item.icon);
        icon.set_pixel_size(28);

        let texts = GtkBox::new(Orientation::Vertical, 2);
        let title = Label::new(Some(&item.title));
        title.set_halign(Align::Start);
        title.set_xalign(0.0);
        title.add_css_class("title");
        let sub = Label::new(Some(&item.subtitle));
        sub.set_halign(Align::Start);
        sub.set_xalign(0.0);
        sub.add_css_class("subtitle");
        sub.set_ellipsize(EllipsizeMode::End);
        sub.set_max_width_chars(60);

        texts.append(&title);
        texts.append(&sub);
        row_box.append(&icon);
        row_box.append(&texts);

        let row = ListBoxRow::new();
        row.set_child(Some(&row_box));
        listbox.append(&row);
        actions.push(item.action.clone());
    }

    let mut s = state.borrow_mut();
    s.actions = actions;
}

fn run_action(action: &Action) {
    match action {
        Action::LaunchApp(id) => {
            let _ = Command::new("gtk-launch")
                .arg(id)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        Action::OpenFile(path) => {
            let _ = Command::new("xdg-open")
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        Action::CopyResult(text) => {
            if let Some(d) = gtk4::gdk::Display::default() {
                let clipboard = d.clipboard();
                clipboard.set_text(text);
            }
        }
    }
}
