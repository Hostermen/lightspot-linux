// SPDX-License-Identifier-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Hostermen

// keywatch.rs — Double-Shift hotkey detector via the Linux evdev interface.
//
// The daemon mode of the backend uses this module to listen for keyboard
// events directly from the kernel (`/dev/input/event*`). Reading at the
// kernel level means the hotkey works on both Wayland and X11 regardless
// of which application currently has focus. When two Shift presses occur
// within DOUBLE_TAP_MS milliseconds, a `()` is sent over the channel so
// the main loop can tell the Electron UI to toggle.
//
// Hotplug handling: keyboards are not only enumerated once at startup.
// A supervisor thread watches `/dev/input` for new device nodes (and, as
// a safety net, periodically re-enumerates) and spawns a fresh watcher
// thread for every keyboard that appears later — e.g. a USB hub or
// Bluetooth keyboard connected after the daemon started, which is exactly
// the case where a single-startup enumeration would silently miss it.
// Each watcher thread removes its device identity from the shared set on
// exit, so an unplugged-and-replugged keyboard is picked up again.

use evdev::{Device, EventSummary, InputId, KeyCode};
use notify::Watcher;
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::Sender; // channel to notify the main loop on a trigger
use std::sync::{Arc, Mutex};
use std::thread; // each keyboard device gets its own thread
use std::time::{Duration, Instant};

/// Maximum time between two Shift presses for them to count as a double-tap.
pub const DOUBLE_TAP_MS: u64 = 280;

/// How long the supervisor waits after the last `/dev/input` event before
/// re-enumerating keyboards. Coalesces bursts (e.g. several nodes created
/// at once when a dock is plugged in) into a single scan.
const HOTPLUG_DEBOUNCE: Duration = Duration::from_millis(500);

/// Upper bound on how often the supervisor re-enumerates even if no notify
/// event is received. A safety net in case the inotify watch on `/dev/input`
/// misses a creation (it shouldn't, but cheap insurance for a hotkey daemon
/// that must stay reliable for the whole session).
const FALLBACK_RESCAN: Duration = Duration::from_secs(15);

// Pure, testable predicate for "is this a double-tap?".
// Kept here (cfg(test)) so production code uses the Instant-based version
// in `watch_one`, while unit tests can drive it with plain integers.
#[cfg(test)]
pub fn is_double_tap(prev: Option<u64>, now: u64, window_ms: u64) -> (Option<u64>, bool) {
    match prev {
        // A previous press exists and is within the window → trigger and reset.
        Some(t) if now.saturating_sub(t) <= window_ms => (None, true),
        // Otherwise, remember this press as the potential first of a pair.
        _ => (Some(now), false),
    }
}

/// Returns true if the given key code is either Shift (left or right).
fn is_shift(k: KeyCode) -> bool {
    k == KeyCode::KEY_LEFTSHIFT || k == KeyCode::KEY_RIGHTSHIFT
}

/// Build a stable identity string for a device, so we can tell "already
/// watched" from "new" across re-enumerations even when the kernel reuses
/// an `eventN` minor number for a different physical device. Combines the
/// kernel name, physical path, unique name, and the input id (bustype /
/// vendor / product / version), all of which are stable for a given piece
/// of hardware.
fn device_id(d: &Device) -> String {
    let id: InputId = d.input_id();
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        d.name().unwrap_or(""),
        d.physical_path().unwrap_or(""),
        d.unique_name().unwrap_or(""),
        id.bus_type(),
        id.vendor(),
        id.product(),
        id.version(),
    )
}

/// Returns true if the device reports a Shift key, i.e. it is (or behaves
/// like) a keyboard. This filters out mice, touchpads, and other input
/// devices that also expose event nodes.
fn is_keyboard_like(d: &Device) -> bool {
    d.supported_keys().is_some_and(|ks| {
        ks.contains(KeyCode::KEY_LEFTSHIFT) || ks.contains(KeyCode::KEY_RIGHTSHIFT)
    })
}

// Enumerate all evdev devices and keep only those that report a Shift key,
// i.e. devices that are (or behave like) keyboards. This filters out
// mice, touchpads, and other input devices that also expose event nodes.
// Returns `(identity, device)` pairs so the caller can dedupe by identity.
fn usable_keyboard_devices() -> Vec<(String, Device)> {
    evdev::enumerate()
        .filter_map(|(_, d)| {
            if is_keyboard_like(&d) {
                Some((device_id(&d), d))
            } else {
                None
            }
        })
        .collect()
}

/// Shared set of keyboard identities that currently have a live watcher
/// thread. The supervisor adds an identity before spawning a watcher; the
/// watcher removes it when its device goes away, so a replug (same
/// identity) is detected on the next scan and watched again.
type Watched = Arc<Mutex<HashSet<String>>>;

/// Spawn the double-Shift watcher. Performs an initial enumeration of
/// keyboards, then starts a supervisor thread that re-enumerates whenever
/// a new input device node appears in `/dev/input` (and periodically as a
/// fallback). Each usable keyboard device gets its own thread that blocks
/// on `fetch_events()`. Any trigger sends `()` on `toggle_tx`.
pub fn spawn_keywatch(toggle_tx: Sender<()>) {
    let watched: Watched = Arc::new(Mutex::new(HashSet::new()));

    let initial = usable_keyboard_devices();
    if initial.is_empty() {
        // No readable keyboard devices right now — usually a permissions
        // issue. We still start the supervisor so devices appearing later
        // (after the user is added to the 'input' group and logs out/in,
        // or after a keyboard is plugged in) are picked up automatically.
        eprintln!(
            "lightspot-files: no readable keyboard devices yet — file hotkey \
             disabled until one appears.\n\
             Add yourself to the 'input' group and log out/in:\n  \
             sudo usermod -aG input $USER\n\
             Or set a custom GNOME shortcut to: lightspot-files"
        );
    }

    let count = spawn_new(initial, &toggle_tx, &watched);
    if count > 0 {
        eprintln!(
            "lightspot-files: listening for double-Shift on {} keyboard device(s).",
            count
        );
    }

    // Supervisor: keeps watching for newly-appeared keyboards.
    let tx = toggle_tx.clone();
    let watched_sup = Arc::clone(&watched);
    thread::spawn(move || supervisor(tx, watched_sup));
}

/// Spawn a watcher thread for every keyboard in `devs` whose identity is
/// not already in `watched`. Each newly spawned identity is inserted into
/// `watched` first so a concurrent re-scan can't double-spawn it. Returns
/// the number of watchers actually spawned this call. Consumes `devs`.
fn spawn_new(devs: Vec<(String, Device)>, tx: &Sender<()>, watched: &Watched) -> usize {
    let mut spawned = 0;
    for (id, d) in devs {
        let mut set = watched.lock().expect("watched mutex poisoned");
        if set.contains(&id) {
            continue; // already being watched
        }
        set.insert(id.clone());
        drop(set);
        let tx = tx.clone();
        let watched = Arc::clone(watched);
        let id = id.clone();
        thread::spawn(move || watch_one(d, tx, watched, id));
        spawned += 1;
    }
    spawned
}

/// Re-enumerate keyboards now and spawn watchers for any new ones. Logs a
/// line per newly discovered device so hotplug is visible in the journal.
fn rescan(tx: &Sender<()>, watched: &Watched) {
    for (id, d) in usable_keyboard_devices() {
        let mut set = watched.lock().expect("watched mutex poisoned");
        if set.contains(&id) {
            continue;
        }
        set.insert(id.clone());
        drop(set);
        let name = d.name().unwrap_or("?").to_string();
        let tx = tx.clone();
        let watched = Arc::clone(watched);
        let id = id.clone();
        eprintln!("lightspot-files: new keyboard detected ({name}) — watching for double-Shift.");
        thread::spawn(move || watch_one(d, tx, watched, id));
    }
}

/// Supervisor loop. Watches `/dev/input` for new device nodes; on any
/// event it waits HOTPLUG_DEBOUNCE (draining further events) then
/// re-enumerates. If no event arrives within FALLBACK_RESCAN it
/// re-enumerates anyway as a safety net.
fn supervisor(tx: Sender<()>, watched: Watched) {
    // notify forwards "something changed in /dev/input" as () on this channel.
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<()>();

    // Best-effort inotify watch on /dev/input. If it can't be created or
    // registered, `watcher` is None and the loop below simply falls back to
    // periodic rescan (ev_rx never receives, so every iteration hits the
    // timeout arm) — the hotkey stays functional, just less promptly.
    let mut watcher: Option<notify::RecommendedWatcher> =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            // Only creation/modify of entries in /dev/input can introduce a new
            // keyboard; ignore everything else (and ignore errors) but trigger a rescan.
            if let Ok(ev) = res {
                let relevant = ev.paths.iter().any(|p| {
                    p.parent()
                        .map(|parent| parent == Path::new("/dev/input"))
                        .unwrap_or(false)
                });
                if relevant {
                    let _ = ev_tx.send(());
                }
            }
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!(
                    "lightspot-files: /dev/input watcher init failed ({e}); periodic rescan only."
                );
                None
            }
        };

    if let Some(w) = watcher.as_mut() {
        if let Err(e) = w.watch(Path::new("/dev/input"), notify::RecursiveMode::NonRecursive) {
            eprintln!("lightspot-files: cannot watch /dev/input ({e}); periodic rescan only.");
        }
    }

    loop {
        match ev_rx.recv_timeout(FALLBACK_RESCAN) {
            Ok(()) => {
                // Drain any further bursts, then rescan once.
                drain(&ev_rx, HOTPLUG_DEBOUNCE);
                rescan(&tx, &watched);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Safety-net periodic rescan (also the only path when there is
                // no working inotify watcher).
                rescan(&tx, &watched);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Discard everything currently queued on `rx`, sleeping `window` after the
/// last received item so a burst coalesces into one rescan.
fn drain(rx: &std::sync::mpsc::Receiver<()>, window: Duration) {
    while rx.recv_timeout(window).is_ok() {
        // keep draining; each recv waits up to `window` for the next
    }
}

/// Per-device event loop. Runs forever, reading events in a blocking fashion.
/// On exit (device gone) it removes `id` from `watched` so the supervisor
/// re-spawns a watcher if the same keyboard reappears.
fn watch_one(mut d: Device, tx: Sender<()>, watched: Watched, id: String) {
    // Timestamp of the most recent Shift press that hasn't yet been paired.
    let mut last: Option<Instant> = None;
    let window = Duration::from_millis(DOUBLE_TAP_MS);
    loop {
        // `fetch_events()` blocks until at least one event is available.
        match d.fetch_events() {
            Ok(events) => {
                for ev in events {
                    // We only care about key events.
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        // value == 1 → key press; value == 2 → auto-repeat (ignored),
                        // so holding Shift doesn't generate spurious triggers.
                        if is_shift(code) && value == 1 {
                            let now = Instant::now();
                            let fire = match last {
                                // Second press within the window → fire and reset.
                                Some(t) if now.duration_since(t) <= window => {
                                    last = None;
                                    true
                                }
                                // First press (or too late) → remember it, don't fire.
                                _ => {
                                    last = Some(now);
                                    false
                                }
                            };
                            // Notify the main loop. Ignore send errors (receiver gone).
                            if fire {
                                let _ = tx.send(());
                            }
                        }
                    }
                }
            }
            // Non-blocking device opened without O_NONBLOCK can still return
            // WouldBlock in some configurations — just retry.
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            // Any other error means the device is gone — exit this thread.
            Err(_) => break,
        }
    }
    // Device disappeared (unplug, BT disconnect). Free our identity so the
    // supervisor can watch the keyboard again when it reappears.
    let mut set = watched.lock().expect("watched mutex poisoned");
    set.remove(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_press_does_not_trigger() {
        let (p, t) = is_double_tap(None, 100, 280); // no prior press
        assert!(!t); // not a double-tap
        assert_eq!(p, Some(100)); // first press remembered
    }

    #[test]
    fn double_press_within_window_triggers() {
        let (p, t) = is_double_tap(Some(100), 200, 280); // 100ms gap
        assert!(t); // triggers
        assert_eq!(p, None); // state reset after firing
    }

    #[test]
    fn double_press_outside_window_does_not_trigger() {
        let (p, t) = is_double_tap(Some(100), 500, 280); // 400ms gap, > 280
        assert!(!t); // too late
        assert_eq!(p, Some(500)); // restart the window
    }

    #[test]
    fn after_trigger_resets() {
        // First pair triggers.
        let (p, t) = is_double_tap(Some(100), 200, 280);
        assert!(t);
        // A third press immediately after must NOT trigger (state was reset).
        let (p2, t2) = is_double_tap(p, 250, 280);
        assert!(!t2);
        assert_eq!(p2, Some(250));
    }

    #[test]
    fn boundary_inclusive() {
        // Exactly 280ms apart → triggers (<= is inclusive).
        let (_, t) = is_double_tap(Some(100), 380, 280);
        assert!(t);
        // 281ms apart → does not trigger.
        let (_, t) = is_double_tap(Some(100), 381, 280);
        assert!(!t);
    }
}
