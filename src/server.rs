// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Hostermen

// server.rs — Search socket server for the daemon.
//
// The one-shot `--search` CLI pays a fixed cost on every query: process
// spawn, re-parsing every `.desktop` file, and re-opening the Tantivy
// index. For the launcher UI — which fires a search per keystroke — that
// overhead dominates latency. Instead, the long-running daemon exposes a
// Unix-domain search socket: the Electron main process connects, sends a
// query terminated by a newline, and reads back one line of JSON
// (the output of `search_json_cached`, which is single-line because
// `escape_json` escapes all control characters).
//
// Protocol:
//   client → server: "<query>\n"
//   server → client: "<json>\n"   (then the server closes the connection)
//
// The search socket lives at `/tmp/spotlight-search.sock` (distinct from
// the toggle socket at `/tmp/spotlight-files.sock`, which the *daemon*
// writes to and the *Electron* app owns). Each connection is handled on
// its own thread so a slow client can't block others; a read timeout
// protects against clients that connect without sending.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use crate::search;

/// Path of the search socket owned by the daemon.
pub const SEARCH_SOCKET: &str = "/tmp/spotlight-search.sock";

/// Spawn the search server on its own thread. Removes any stale socket
/// file left by a previous crash before binding.
pub fn spawn_search_server() {
    std::thread::spawn(|| {
        // Clean up a stale socket from a previous crash/exit.
        let _ = std::fs::remove_file(SEARCH_SOCKET);
        let listener = match UnixListener::bind(SEARCH_SOCKET) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("spotlight-files: search socket bind failed: {e}");
                return;
            }
        };
        // Restrict to the owner, matching the toggle socket's permissions.
        let _ = std::fs::set_permissions(SEARCH_SOCKET, std::fs::Permissions::from_mode(0o600));
        eprintln!("spotlight-files: search server listening on {SEARCH_SOCKET}");

        // Accept loop: one connection per iteration, each on its own thread.
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    std::thread::spawn(|| handle_client(s));
                }
                Err(_) => continue, // transient accept error → keep going
            }
        }
    });
}

/// Handle a single client: read one line (the query), run the cached
/// search, and write the JSON response followed by a newline.
fn handle_client(mut stream: UnixStream) {
    // Don't let a client that connects but never sends hang this thread.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));

    // Read the query line while the reader borrows the stream, then write
    // the response after the borrow ends.
    let json = {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return; // malformed/empty request → drop the connection
        }
        // `search_json_cached` handles empty/whitespace queries itself.
        search::search_json_cached(line.trim())
    };

    let _ = stream.write_all(json.as_bytes());
    let _ = stream.write_all(b"\n");
    // `stream` drops here, closing the connection and signalling EOF.
}
