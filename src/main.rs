// main.rs — Entry point for the `spotlight-files` Rust backend.
//
// The backend has two runtime modes:
//   1. `--search <query>` — print search results as JSON and exit. This is
//      how the Electron frontend fetches results: it spawns the binary with
//      `--search <query>` and parses stdout.
//   2. (default, no args) — daemon mode. Listens for the double-Shift hotkey
//      via evdev and, on each trigger, writes `toggle` to the Unix socket at
//      /tmp/spotlight-files.sock so the Electron app shows/hides its window.

mod app_search;
mod calculator;
mod content_index;
mod file_search;
mod keywatch;
mod model;
mod search;

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

/// Path of the Unix-domain socket the Electron app listens on.
const SOCKET_PATH: &str = "/tmp/spotlight-files.sock";

/// Send a one-word command ("toggle" / "show" / "hide") to the Electron app
/// via the socket. Failure is silently ignored (app may not be running yet).
fn toggle_extension() {
    if let Ok(mut sock) = UnixStream::connect(SOCKET_PATH) {
        let _ = sock.write_all(b"toggle");
    }
}

/// `--search` mode: evaluate the query and print JSON to stdout.
fn run_search_cli(query: &str) {
    let json = search::search_json(query);
    println!("{json}");
}

/// Daemon mode: warn if plocate is missing, then block forever listening
/// for double-Shift presses and forwarding each one to the Electron app.
fn run_daemon() {
    // File search is optional; warn (don't crash) if plocate isn't installed.
    if !file_search::plocate_available() {
        eprintln!(
            "spotlight-files: 'plocate' not found — file search disabled.\n\
             Install it:  sudo apt install plocate && sudo updatedb.plocate"
        );
    }
    // Start the background content indexer (initial full build + watcher).
    // Runs in its own thread so it never blocks the hotkey listener.
    content_index::spawn_indexer();
    // Channel: the keywatch thread sends `()` on every double-Shift; the
    // main loop receives it and triggers the Electron app over the socket.
    let (tx, rx) = mpsc::channel::<()>();
    keywatch::spawn_keywatch(tx);
    eprintln!("spotlight-files: daemon running (double-Shift to toggle)");
    // Block forever: each received event toggles the Electron window.
    loop {
        if rx.recv().is_ok() {
            toggle_extension();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Inspect argv for a subcommand flag.
    if args.len() >= 2 {
        match args[1].as_str() {
            // JSON search mode — used by the Electron frontend via IPC.
            "--search" => {
                let query = args.get(2).cloned().unwrap_or_default();
                run_search_cli(&query);
                return;
            }
            // Rebuild the full-text content index and exit.
            "--index" => {
                let n = content_index::build_index();
                println!("indexed {n} documents");
                return;
            }
            // Help text.
            "--help" | "-h" => {
                println!("spotlight-files — Spotlight-style launcher for Linux");
                println!();
                println!("USAGE:");
                println!("  spotlight-files              Run as daemon (double-Shift hotkey → Electron UI)");
                println!("  spotlight-files --search Q   Output JSON search results for query Q and exit");
                println!("  spotlight-files --index      (Re)build the full-text content index and exit");
                println!("  spotlight-files --help       Show this help");
                return;
            }
            _ => {}   // unknown flag → fall through to daemon mode
        }
    }

    // Default: run the hotkey daemon.
    run_daemon();
}
