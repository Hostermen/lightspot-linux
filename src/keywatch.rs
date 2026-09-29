use evdev::{Device, EventSummary, KeyCode};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

pub const DOUBLE_TAP_MS: u64 = 280;

#[cfg(test)]
pub fn is_double_tap(prev: Option<u64>, now: u64, window_ms: u64) -> (Option<u64>, bool) {
    match prev {
        Some(t) if now.saturating_sub(t) <= window_ms => (None, true),
        _ => (Some(now), false),
    }
}

fn is_shift(k: KeyCode) -> bool {
    k == KeyCode::KEY_LEFTSHIFT || k == KeyCode::KEY_RIGHTSHIFT
}

fn usable_keyboard_devices() -> Vec<Device> {
    evdev::enumerate()
        .filter_map(|(_, d)| {
            let has_shift = d
                .supported_keys()
                .is_some_and(|ks| ks.contains(KeyCode::KEY_LEFTSHIFT) || ks.contains(KeyCode::KEY_RIGHTSHIFT));
            if has_shift {
                Some(d)
            } else {
                None
            }
        })
        .collect()
}

pub fn spawn_keywatch(toggle_tx: Sender<()>) {
    let devices = usable_keyboard_devices();
    if devices.is_empty() {
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
    for d in devices {
        let tx = toggle_tx.clone();
        thread::spawn(move || watch_one(d, tx));
    }
}

fn watch_one(mut d: Device, tx: Sender<()>) {
    let mut last: Option<Instant> = None;
    let window = Duration::from_millis(DOUBLE_TAP_MS);
    loop {
        match d.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        // value == 1 is a real press; 2 is auto-repeat (ignored).
                        if is_shift(code) && value == 1 {
                            let now = Instant::now();
                            let fire = match last {
                                Some(t) if now.duration_since(t) <= window => {
                                    last = None;
                                    true
                                }
                                _ => {
                                    last = Some(now);
                                    false
                                }
                            };
                            if fire {
                                let _ = tx.send(());
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_press_does_not_trigger() {
        let (p, t) = is_double_tap(None, 100, 280);
        assert!(!t);
        assert_eq!(p, Some(100));
    }

    #[test]
    fn double_press_within_window_triggers() {
        let (p, t) = is_double_tap(Some(100), 200, 280);
        assert!(t);
        assert_eq!(p, None);
    }

    #[test]
    fn double_press_outside_window_does_not_trigger() {
        let (p, t) = is_double_tap(Some(100), 500, 280);
        assert!(!t);
        assert_eq!(p, Some(500));
    }

    #[test]
    fn after_trigger_resets() {
        let (p, t) = is_double_tap(Some(100), 200, 280);
        assert!(t);
        let (p2, t2) = is_double_tap(p, 250, 280);
        assert!(!t2);
        assert_eq!(p2, Some(250));
    }

    #[test]
    fn boundary_inclusive() {
        let (_, t) = is_double_tap(Some(100), 380, 280);
        assert!(t);
        let (_, t) = is_double_tap(Some(100), 381, 280);
        assert!(!t);
    }
}
