// keywatch.rs — Double-Shift hotkey detector via the Linux evdev interface.
//
// The daemon mode of the backend uses this module to listen for keyboard
// events directly from the kernel (`/dev/input/event*`). Reading at the
// kernel level means the hotkey works on both Wayland and X11 regardless
// of which application currently has focus. When two Shift presses occur
// within DOUBLE_TAP_MS milliseconds, a `()` is sent over the channel so
// the main loop can tell the Electron UI to toggle.

use evdev::{Device, EventSummary, KeyCode};
use std::sync::mpsc::Sender;   // channel to notify the main loop on a trigger
use std::thread;               // each keyboard device gets its own thread
use std::time::{Duration, Instant};

/// Maximum time between two Shift presses for them to count as a double-tap.
pub const DOUBLE_TAP_MS: u64 = 280;

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

// Enumerate all evdev devices and keep only those that report a Shift key,
// i.e. devices that are (or behave like) keyboards. This filters out
// mice, touchpads, and other input devices that also expose event nodes.
fn usable_keyboard_devices() -> Vec<Device> {
    evdev::enumerate()
        .filter_map(|(_, d)| {
            // `supported_keys()` is None on devices without a key event capability.
            let has_shift = d
                .supported_keys()
                .is_some_and(|ks| ks.contains(KeyCode::KEY_LEFTSHIFT) || ks.contains(KeyCode::KEY_RIGHTSHIFT));
            // Keep only keyboard-like devices.
            if has_shift {
                Some(d)
            } else {
                None
            }
        })
        .collect()
}

/// Spawn the double-Shift watcher. Each usable keyboard device gets its own
/// thread that blocks on `fetch_events()`. Any trigger sends `()` on `toggle_tx`.
pub fn spawn_keywatch(toggle_tx: Sender<()>) {
    let devices = usable_keyboard_devices();
    if devices.is_empty() {
        // No readable keyboard devices — usually a permissions issue.
        eprintln!(
            "spotlight-files: no readable keyboard devices — file hotkey disabled.\n\
             Add yourself to the 'input' group and log out/in:\n  \
             sudo usermod -aG input $USER\n\
             Or set a custom GNOME shortcut to: spotlight-files"
        );
        return;
    }
    eprintln!(
        "spotlight-files: listening for double-Shift on {} keyboard device(s).",
        devices.len()
    );
    // One watcher thread per device so a single blocked read can't starve others.
    for d in devices {
        let tx = toggle_tx.clone();     // clone the sender for this thread
        thread::spawn(move || watch_one(d, tx));
    }
}

/// Per-device event loop. Runs forever, reading events in a blocking fashion.
fn watch_one(mut d: Device, tx: Sender<()>) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_press_does_not_trigger() {
        let (p, t) = is_double_tap(None, 100, 280);   // no prior press
        assert!(!t);                                   // not a double-tap
        assert_eq!(p, Some(100));                      // first press remembered
    }

    #[test]
    fn double_press_within_window_triggers() {
        let (p, t) = is_double_tap(Some(100), 200, 280); // 100ms gap
        assert!(t);                                      // triggers
        assert_eq!(p, None);                              // state reset after firing
    }

    #[test]
    fn double_press_outside_window_does_not_trigger() {
        let (p, t) = is_double_tap(Some(100), 500, 280); // 400ms gap, > 280
        assert!(!t);                                       // too late
        assert_eq!(p, Some(500));                         // restart the window
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
