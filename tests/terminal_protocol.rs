//! Real executable/PTY regression tests. These inspect protocol bytes and saved
//! source, not a second screen renderer or synthetic Crossterm KeyEvents.
#![cfg(unix)]

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const ENTER: &[u8] = b"\x1b[?1049h";
const PUSH: &[u8] = b"\x1b[>1u";
const POP: &[u8] = b"\x1b[<1u";
const LEAVE: &[u8] = b"\x1b[?1049l";
const SAVE: &[u8] = b"\x1b[115;5u";
const QUIT: &[u8] = b"\x1b[113;5u";
const EXITED: &[u8] = b"MARKLANE_EXIT=0";

fn count(bytes: &[u8], sequence: &[u8]) -> usize {
    bytes
        .windows(sequence.len())
        .filter(|part| *part == sequence)
        .count()
}

fn position(bytes: &[u8], sequence: &[u8]) -> usize {
    bytes
        .windows(sequence.len())
        .position(|part| part == sequence)
        .unwrap_or_else(|| panic!("Missing {sequence:?} in {bytes:?}"))
}

fn termios(file: &File) -> libc::termios {
    let mut result = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(file.as_raw_fd(), result.as_mut_ptr()) },
        0,
        "tcgetattr: {}",
        io::Error::last_os_error()
    );
    unsafe { result.assume_init() }
}

struct Session {
    child: Child,
    master: File,
    slave: File,
    original_termios: libc::termios,
    transcript: Vec<u8>,
    fixture: PathBuf,
    _directory: tempfile::TempDir,
}

impl Session {
    fn start(source: &str) -> Self {
        Self::start_with_state(source, None)
    }

    fn start_with_state(source: &str, state: Option<&std::path::Path>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let fixture = directory.path().join("protocol.md");
        fs::write(&fixture, source).unwrap();
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 32,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        for file in [&master, &slave] {
            assert_eq!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0);
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0
        );
        let original_termios = termios(&slave);
        // Keep the session leader alive after the editor exits: macOS revokes
        // its PTY when the leader exits, preventing a termios restoration check.
        // This noninteractive shell does not configure terminal modes. It only
        // waits for the child and for our final newline acknowledging the check.
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "\"$@\"\neditor_status=$?\nprintf '\\nMARKLANE_EXIT=%s\\n' \"$editor_status\"\nIFS= read -r release_line\nexit \"$editor_status\"",
            "marklane-protocol",
            env!("CARGO_BIN_EXE_marklane"),
        ]);
        if let Some(state) = state {
            command
                .env("XDG_CONFIG_HOME", state.join("config"))
                .env("XDG_STATE_HOME", state.join("state"));
        } else {
            command.arg("--no-state");
        }
        command
            .arg(&fixture)
            .env("TERM", "xterm-ghostty")
            // Force the existing internal clipboard path without any network
            // connection or changes to the editor's clipboard implementation.
            .env("SSH_CONNECTION", "terminal-protocol-test")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut session = Self {
            child: command.spawn().unwrap(),
            master,
            slave,
            original_termios,
            transcript: Vec::new(),
            fixture,
            _directory: directory,
        };
        session.until("first frame", |s| count(&s.transcript, b"\x1b[?25h") > 0);
        assert!(position(&session.transcript, ENTER) < position(&session.transcript, PUSH));
        assert_eq!(count(&session.transcript, PUSH), 1);
        assert_eq!(
            count(&session.transcript, b"\x1b[?u"),
            0,
            "startup must not wait for capability replies"
        );
        assert_ne!(
            termios(&session.slave).c_lflag & libc::ICANON,
            session.original_termios.c_lflag & libc::ICANON
        );
        session
    }

    fn pump(&mut self) {
        let mut buffer = [0; 8192];
        loop {
            match self.master.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => self.transcript.extend_from_slice(&buffer[..size]),
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.raw_os_error() == Some(libc::EIO) =>
                {
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("PTY read: {error}"),
            }
        }
    }

    fn until(&mut self, description: &str, mut ready: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pump();
            if ready(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for {description}; transcript: {:?}",
                String::from_utf8_lossy(
                    &self.transcript[self.transcript.len().saturating_sub(4000)..]
                )
            );
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "Child exited before {description}; transcript: {:?}",
                String::from_utf8_lossy(
                    &self.transcript[self.transcript.len().saturating_sub(4000)..]
                )
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }

    fn save_and_expect(&mut self, expected: &str) {
        self.send(SAVE);
        self.until("saved source", |s| {
            fs::read_to_string(&s.fixture).unwrap() == expected
        });
    }

    fn finish(mut self) {
        self.send(QUIT);
        self.until("editor exit", |s| count(&s.transcript, EXITED) == 1);
        assert_eq!(count(&self.transcript, POP), 1);
        assert_eq!(count(&self.transcript, LEAVE), 1);
        assert!(position(&self.transcript, POP) < position(&self.transcript, LEAVE));
        let restored = termios(&self.slave);
        let original = &self.original_termios;
        assert_eq!(
            (
                restored.c_iflag,
                restored.c_oflag,
                restored.c_cflag,
                restored.c_lflag,
                restored.c_cc
            ),
            (
                original.c_iflag,
                original.c_oflag,
                original.c_cflag,
                original.c_lflag,
                original.c_cc
            )
        );
        self.send(b"\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pump();
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status}: {:?}", self.transcript);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Child did not exit: {:?}",
                String::from_utf8_lossy(
                    &self.transcript[self.transcript.len().saturating_sub(4000)..]
                )
            );
            thread::sleep(Duration::from_millis(5));
        }
        self.pump();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // The pre-exec setsid gives this test its own process group. Kill
            // both the wrapper and editor if an assertion fails mid-session.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.wait();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum InputProfile {
    GhosttyNegotiated,
    GhosttyDefault,
    Legacy,
    LegacyPrevious,
}

impl InputProfile {
    fn previous(self, output: &[u8]) -> &'static [u8] {
        match self {
            Self::GhosttyNegotiated if count(output, PUSH) > count(output, POP) => b"\x1b[13;2u",
            Self::GhosttyNegotiated | Self::GhosttyDefault => b"\x1b[27;2;13~",
            Self::Legacy => b"\r",
            Self::LegacyPrevious => b"\x1b[13;2~",
        }
    }
}

#[test]
fn search_navigation_uses_negotiated_bytes_and_retains_legacy_fallback() {
    // Three matches distinguish previous, next, and an ignored key. The forced
    // default/legacy profiles deliberately ignore the application's opt-in.
    for (profile, selected) in [
        (InputProfile::GhosttyNegotiated, 0),
        (InputProfile::GhosttyDefault, 1),
        (InputProfile::Legacy, 2),
        (InputProfile::LegacyPrevious, 0),
    ] {
        let mut session = Session::start("needle one\nneedle two\nneedle three\n");
        session.send(b"\x1b[102;5uneedle\r");
        session.send(profile.previous(&session.transcript));
        // Esc closes Find; typing replaces the selected match. Saving makes the
        // navigation result observable without parsing the rendered screen.
        session.send(b"\x1b[27upicked");
        let mut lines = ["needle one", "needle two", "needle three"].map(String::from);
        lines[selected] = lines[selected].replacen("needle", "picked", 1);
        session.save_and_expect(&(lines.join("\n") + "\n"));
        session.finish();
    }
}

#[test]
fn ordinary_typing_enter_and_bracketed_paste_preserve_source() {
    let mut session = Session::start("");
    assert_eq!(count(&session.transcript, b"\x1b[?2004h"), 1);
    session.send(b"typed\r");
    session.send("\x1b[200~line one\n- [ ] task\t終\n\x1b[201~".as_bytes());
    session.save_and_expect("typed\nline one\n- [ ] task\t終\n");
    session.finish();
}

#[test]
fn word_selection_formatting_indentation_and_typing_undo_reach_the_core() {
    let source = "alpha beta\n";
    let mut session = Session::start(source);
    // Modified arrow transport selects one source word; CSI-u delivers the
    // formatting and indentation commands without legacy control aliases.
    session.send(b"\x1b[1;6C\x1b[98;5u");
    session.save_and_expect("**alpha** beta\n");
    session.send(b"\x1b[122;5u");
    session.save_and_expect(source);
    session.send(b"\x1b[93;5u");
    session.save_and_expect("    alpha beta\n");
    session.send(b"\x1b[122;5u");
    session.save_and_expect(source);
    // Literal terminal text is a run of keyboard events, not bracketed paste.
    session.send(b"\x1b[1;5Fdraft\x1b[122;5u");
    session.save_and_expect(source);
    // Alt+I is the portable italic shortcut; plain Tab remains indentation.
    session.send(b"\x1b[1;5H\x1b[1;6C\x1b[105;3u");
    session.save_and_expect("*alpha* beta\n");
    session.send(b"\x1b[122;5u");
    session.save_and_expect(source);
    // Even a no-op tab command must end a typing group in the workspace.
    session.send(b"\x1b[1;5Ffirst\x1b[19~second\x1b[122;5u");
    session.save_and_expect("alpha beta\nfirst");
    session.send(b"\x1b[122;5u");
    session.save_and_expect(source);
    session.finish();
}

#[test]
fn crash_recovery_and_saved_themes_survive_a_real_process_restart() {
    let state = tempfile::tempdir().unwrap();
    let mut crashed = Session::start_with_state("disk baseline\n", Some(state.path()));
    // Apply a familiar built-in theme through both searchable pickers.
    crashed.send(b"\x10theme\r");
    crashed.until("theme picker", |s| count(&s.transcript, b"Preview") > 0);
    crashed.send(b"\x1b[200~Catppuccin Mocha\x1b[201~\r");
    let settings = state.path().join("config/marklane/settings.conf");
    crashed.until("persisted theme", |_| {
        fs::read_to_string(&settings).is_ok_and(|text| text.contains("theme=catppuccin-mocha"))
    });
    crashed.send("\x1b[200~unsaved 界\n\x1b[201~".as_bytes());
    let recovery = state.path().join("state/marklane/recovery");
    crashed.until("durable recovery checkpoint", |_| {
        fs::read_dir(&recovery).is_ok_and(|sessions| {
            sessions.filter_map(Result::ok).any(|entry| {
                fs::read_dir(entry.path()).is_ok_and(|files| {
                    files.filter_map(Result::ok).any(|file| {
                        if file.path().extension().is_none_or(|ext| ext != "snapshot") {
                            return false;
                        }
                        fs::read(file.path()).is_ok_and(|bytes| {
                            bytes
                                .windows("unsaved 界".len())
                                .any(|window| window == "unsaved 界".as_bytes())
                        })
                    })
                })
            })
        })
    });
    // Abruptly terminate the entire isolated PTY process group. No clean-exit
    // code runs, and the source file was never saved.
    unsafe {
        libc::kill(-(crashed.child.id() as i32), libc::SIGKILL);
    }
    crashed.child.wait().unwrap();
    assert_eq!(
        fs::read_to_string(&crashed.fixture).unwrap(),
        "disk baseline\n"
    );
    fs::write(&crashed.fixture, "newer disk contents\n").unwrap();
    let mut restored = Session::start_with_state("another document\n", Some(state.path()));
    restored.until("recovery notice", |s| {
        count(&s.transcript, b"recoverable") > 0
    });
    restored.send(b"\x10recover\r");
    restored.until("recovery picker", |s| {
        count(&s.transcript, b"recover copy") > 0
    });
    restored.send(b"\r");
    restored.until("recovered tab", |s| {
        count(&s.transcript, b"as an unsaved copy.") > 0
    });
    let saved = state.path().join("recovered.md");
    restored.send(SAVE);
    restored.until("save as", |s| {
        count(&s.transcript, b"Save As") + count(&s.transcript, b"Save as") > 0
    });
    restored.send(b"\x01");
    restored.send(format!("\x1b[200~{}\x1b[201~\r", saved.display()).as_bytes());
    restored.until("saved recovered copy", |_| {
        fs::read_to_string(&saved).is_ok_and(|text| text == "unsaved 界\ndisk baseline\n")
    });
    assert_eq!(
        fs::read_to_string(&crashed.fixture).unwrap(),
        "newer disk contents\n"
    );
    restored.finish();
    assert!(
        fs::read_to_string(settings)
            .unwrap()
            .contains("theme=catppuccin-mocha")
    );
}
