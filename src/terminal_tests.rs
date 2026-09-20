//! Fault injection and isolated panic-hook checks; no real terminal is required.
use super::*;
use std::{cell::Cell, process::Command};

const ENTER: &[u8] = b"\x1b[?1049h";
const PUSH: &[u8] = b"\x1b[>1u";
const POP: &[u8] = b"\x1b[<1u";
const LEAVE: &[u8] = b"\x1b[?1049l";
const DISABLE_PASTE: &[u8] = b"\x1b[?2004l";

fn position(output: &[u8], sequence: &[u8]) -> usize {
    output
        .windows(sequence.len())
        .position(|s| s == sequence)
        .unwrap_or_else(|| {
            panic!(
                "Missing {sequence:?} in {}",
                String::from_utf8_lossy(output)
            )
        })
}

fn count(output: &[u8], sequence: &[u8]) -> usize {
    output
        .windows(sequence.len())
        .filter(|s| *s == sequence)
        .count()
}

#[derive(Default)]
struct FaultWriter {
    bytes: Vec<u8>,
    remaining: Option<usize>,
    fail_flush: Option<usize>,
    flushes: usize,
}

impl Write for FaultWriter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.remaining == Some(0) {
            self.remaining = None;
            return Err(io::Error::other("injected write failure"));
        }
        let size = self
            .remaining
            .map_or(input.len(), |left| left.min(input.len()));
        self.bytes.extend_from_slice(&input[..size]);
        if let Some(left) = &mut self.remaining {
            *left -= size;
        }
        Ok(size)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush == Some(self.flushes) {
            return Err(io::Error::other("injected flush failure"));
        }
        Ok(())
    }
}

#[test]
fn cleanup_is_ordered_and_claimed_once() {
    let state = TerminalState::default();
    let mut output = Vec::new();
    state.enter(&mut output, || Ok(())).unwrap();
    let restored = Cell::new(0);
    for _ in 0..2 {
        state.restore(&mut output, || {
            restored.set(restored.get() + 1);
            Ok(())
        });
    }
    assert!(position(&output, ENTER) < position(&output, PUSH));
    assert!(position(&output, DISABLE_PASTE) < position(&output, POP));
    assert!(position(&output, POP) < position(&output, LEAVE));
    assert_eq!(count(&output, PUSH), 1);
    assert_eq!(count(&output, POP), 1);
    assert_eq!(count(&output, LEAVE), 1);
    assert_eq!(restored.get(), 1);
}

#[test]
fn failed_raw_mode_does_not_restore_unacquired_modes() {
    let state = TerminalState::default();
    let mut output = Vec::new();
    assert!(
        state
            .enter(&mut output, || Err(io::Error::other("raw failed")))
            .is_err()
    );
    state.restore(&mut output, || panic!("raw mode was never enabled"));
    assert!(output.is_empty());
}

#[test]
fn partial_push_is_not_popped_but_failed_flush_is() {
    for (remaining, fail_flush, expected_pops) in [
        (Some(ENTER.len() + PUSH.len() - 1), None, 0),
        (None, Some(2), 1),
    ] {
        let state = TerminalState::default();
        let mut output = FaultWriter {
            remaining,
            fail_flush,
            ..Default::default()
        };
        assert!(state.enter(&mut output, || Ok(())).is_err());
        let restored = Cell::new(false);
        state.restore(&mut output, || {
            restored.set(true);
            Ok(())
        });
        assert_eq!(count(&output.bytes, POP), expected_pops);
        assert_eq!(count(&output.bytes, LEAVE), 1);
        assert_eq!(count(&output.bytes, DISABLE_PASTE), 0);
        assert!(restored.get());
    }
}

#[test]
fn partial_mouse_setup_restores_mouse_keyboard_and_raw_mode() {
    let state = TerminalState::default();
    let mut output = FaultWriter {
        remaining: Some(ENTER.len() + PUSH.len() + b"\x1b[?1000h".len()),
        ..Default::default()
    };
    assert!(state.enter(&mut output, || Ok(())).is_err());
    let restored = Cell::new(false);
    state.restore(&mut output, || {
        restored.set(true);
        Ok(())
    });
    assert!(position(&output.bytes, b"\x1b[?1000l") < position(&output.bytes, POP));
    assert!(position(&output.bytes, POP) < position(&output.bytes, LEAVE));
    assert_eq!(count(&output.bytes, DISABLE_PASTE), 0);
    assert!(restored.get());
}

#[test]
fn cleanup_write_failure_does_not_skip_other_modes_or_raw_restore() {
    let state = TerminalState::default();
    let mut output = FaultWriter::default();
    state.enter(&mut output, || Ok(())).unwrap();
    output.bytes.clear();
    output.remaining = Some(0);
    let restored = Cell::new(false);
    state.restore(&mut output, || {
        restored.set(true);
        Ok(())
    });
    assert!(position(&output.bytes, b"\x1b[?1000l") < position(&output.bytes, POP));
    assert!(position(&output.bytes, POP) < position(&output.bytes, LEAVE));
    assert!(restored.get());
}

#[test]
fn panic_cleanup_and_previous_hook_are_isolated_from_other_tests() {
    for mode in ["panic", "return", "error"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal::tests::panic_hook_probe",
                "--nocapture",
            ])
            .env("MARKLANE_TERMINAL_HOOK_PROBE", mode)
            .output()
            .unwrap();
        assert!(result.status.success(), "{mode}: {result:?}");
        assert_eq!(count(&result.stdout, POP), 1, "{mode}: {result:?}");
        assert_eq!(count(&result.stdout, LEAVE), 1, "{mode}: {result:?}");
        assert!(position(&result.stdout, POP) < position(&result.stdout, LEAVE));
        assert!(position(&result.stdout, LEAVE) < position(&result.stdout, b"PREVIOUS-HOOK"));
    }
}

// Invoked in a fresh test process so replacing its global panic hook cannot
// interfere with concurrently running application tests.
#[test]
fn panic_hook_probe() {
    let Ok(mode) = std::env::var("MARKLANE_TERMINAL_HOOK_PROBE") else {
        return;
    };
    std::panic::set_hook(Box::new(|_| {
        writeln!(io::stdout(), "PREVIOUS-HOOK").unwrap();
    }));
    let guard = TerminalGuard::new();
    let weak_state = Arc::downgrade(&guard.state);
    if mode == "error" {
        let mut writer = FaultWriter {
            fail_flush: Some(2),
            ..Default::default()
        };
        assert!(guard.state.enter(&mut writer, || Ok(())).is_err());
    } else {
        guard.state.enter(&mut io::stdout(), || Ok(())).unwrap();
    }
    if mode == "panic" {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = guard;
            panic!("exercise hook then guard cleanup");
        }));
    } else {
        drop(guard);
        assert!(
            weak_state.upgrade().is_none(),
            "installed hook retained terminal state"
        );
        let _ = std::panic::catch_unwind(|| panic!("exercise restored hook"));
    }
}
