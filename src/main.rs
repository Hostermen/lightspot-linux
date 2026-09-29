mod app_search;
mod calculator;
mod file_search;
mod keywatch;
mod model;
mod search;
mod ui;

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

const SOCKET_PATH: &str = "/tmp/spotlight-files.sock";

fn toggle_extension() {
    if let Ok(mut sock) = UnixStream::connect(SOCKET_PATH) {
        let _ = sock.write_all(b"toggle");
    }
}

fn run_search_cli(query: &str) {
    let json = search::search_json(query);
    println!("{json}");
}

fn run_daemon() {
    if !file_search::plocate_available() {
        eprintln!(
            "spotlight-files: 'plocate' not found — file search disabled.\n\
             Install it:  sudo apt install plocate && sudo updatedb.plocate"
        );
    }
    let (tx, rx) = mpsc::channel::<()>();
    keywatch::spawn_keywatch(tx);
    eprintln!("spotlight-files: daemon running (double-Shift to toggle)");
    loop {
        if rx.recv().is_ok() {
            toggle_extension();
        }
    }
}

fn run_gtk() {
    use gtk4::gio::ApplicationHoldGuard;
    use gtk4::glib;
    use gtk4::prelude::*;
    use gtk4::Application;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    if !file_search::plocate_available() {
        eprintln!(
            "spotlight-files: 'plocate' not found — file search disabled.\n\
             Install it:  sudo apt install plocate && sudo updatedb.plocate"
        );
    }

    let app = Application::new(Some("dev.spotlightfiles.Launcher"), Default::default());

    let launcher: Rc<RefCell<Option<Rc<ui::Launcher>>>> = Rc::new(RefCell::new(None));
    let guard: Rc<RefCell<Option<ApplicationHoldGuard>>> = Rc::new(RefCell::new(None));
    let held = Rc::new(std::cell::Cell::new(false));

    let launcher_c = launcher.clone();
    let guard_c = guard.clone();
    let held_c = held.clone();
    app.connect_activate(move |app| {
        if !held_c.get() {
            *guard_c.borrow_mut() = Some(app.hold());
            held_c.set(true);
        }
        let l = {
            let cell = launcher_c.borrow();
            cell.as_ref().map(|l| l.clone())
        };
        match l {
            Some(l) => l.toggle(),
            None => {
                let l = ui::build(app);
                l.toggle();
                *launcher_c.borrow_mut() = Some(l);
            }
        }
    });

    let use_evdev = std::env::var("SPOTLIGHT_USE_EVDEV")
        .ok()
        .map(|v| v != "0")
        .unwrap_or(true);
    if use_evdev {
        let (tx, rx) = mpsc::channel::<()>();
        keywatch::spawn_keywatch(tx);
        let launcher_c = launcher.clone();
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let mut got = false;
            while let Ok(()) = rx.try_recv() {
                got = true;
            }
            if got {
                let cell = launcher_c.borrow();
                if let Some(l) = cell.as_ref() {
                    l.toggle();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    app.run();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() >= 2 {
        match args[1].as_str() {
            "--search" => {
                let query = args.get(2).cloned().unwrap_or_default();
                run_search_cli(&query);
                return;
            }
            "--gtk" => {
                run_gtk();
                return;
            }
            "--help" | "-h" => {
                println!("spotlight-files — Spotlight-style launcher for Linux");
                println!();
                println!("USAGE:");
                println!("  spotlight-files              Run as daemon (double-Shift hotkey → GNOME extension)");
                println!("  spotlight-files --search Q    Output JSON search results for query Q and exit");
                println!("  spotlight-files --gtk         Run standalone GTK4 window (fallback)");
                println!("  spotlight-files --help        Show this help");
                return;
            }
            _ => {}
        }
    }

    run_daemon();
}
