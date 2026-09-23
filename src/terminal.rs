use crate::workspace::Workspace as App;
#[cfg(unix)]
use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        PopKeyboardEnhancementFlags,
    },
    execute, queue,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, Write},
    panic::PanicHookInfo,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

const RAW: u8 = 1;
const ALTERNATE: u8 = 2;
const KEYBOARD: u8 = 4;
const MOUSE: u8 = 8;
const PASTE: u8 = 16;
const CURSOR: u8 = 32;

#[derive(Default)]
struct TerminalState(AtomicU8);

impl TerminalState {
    fn mark(&self, mode: u8) {
        self.0.fetch_or(mode, Ordering::SeqCst);
    }

    fn enter(
        &self,
        output: &mut impl Write,
        enable_raw: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        enable_raw()?;
        self.mark(RAW);

        // These commands can fail after writing part of their escape sequences.
        self.mark(ALTERNATE);
        execute!(output, EnterAlternateScreen)?;

        // No synchronous capability query: Crossterm 0.28 can wait indefinitely
        // for a missing reply. Unsupported ANSI terminals retain legacy input.
        // This command is unavailable with Crossterm's legacy Windows backend.
        #[cfg(unix)]
        {
            queue!(
                output,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            // Only a complete push may be popped. Record it before flushing,
            // since a failed flush may already have sent it to the terminal.
            self.mark(KEYBOARD);
            output.flush()?;
        }

        self.mark(MOUSE);
        execute!(output, EnableMouseCapture)?;
        self.mark(PASTE);
        execute!(output, EnableBracketedPaste)?;
        // Typing always inserts, so the caret reads as an insertion bar.
        self.mark(CURSOR);
        execute!(output, SetCursorStyle::SteadyBar)?;
        Ok(())
    }

    fn restore(&self, output: &mut impl Write, disable_raw: impl FnOnce() -> io::Result<()>) {
        // The panic hook runs before unwinding drops the guard. Claim cleanup
        // once, including on initialization errors, so a second pop cannot
        // touch the main screen's independent keyboard stack.
        let modes = self.0.swap(0, Ordering::SeqCst);
        if modes & PASTE != 0 {
            let _ = execute!(output, DisableBracketedPaste);
        }
        if modes & MOUSE != 0 {
            let _ = execute!(output, DisableMouseCapture);
        }
        if modes & KEYBOARD != 0 {
            let _ = execute!(output, PopKeyboardEnhancementFlags);
        }
        if modes & CURSOR != 0 {
            let _ = execute!(output, SetCursorStyle::DefaultUserShape);
        }
        if modes & ALTERNATE != 0 {
            let _ = execute!(output, LeaveAlternateScreen);
            let _ = execute!(output, Show);
        }
        if modes & RAW != 0 {
            let _ = disable_raw();
        }
    }
}

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Send + Sync + 'static>;

struct TerminalGuard {
    state: Arc<TerminalState>,
    previous_hook: Option<Arc<PanicHook>>,
}

impl TerminalGuard {
    fn new() -> Self {
        let state = Arc::new(TerminalState::default());
        let previous_hook = Arc::new(std::panic::take_hook());
        let hook_state = state.clone();
        let hook_previous = previous_hook.clone();
        std::panic::set_hook(Box::new(move |info| {
            hook_state.restore(&mut io::stdout(), disable_raw_mode);
            hook_previous(info);
        }));
        Self {
            state,
            previous_hook: Some(previous_hook),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.state.restore(&mut io::stdout(), disable_raw_mode);
        // Rust forbids replacing the hook while this thread is panicking.
        // The installed hook's state is already empty in that case.
        if !std::thread::panicking() {
            drop(std::panic::take_hook());
            if let Some(previous) = self.previous_hook.take() {
                let hook = Arc::try_unwrap(previous)
                    .unwrap_or_else(|shared| Box::new(move |info| shared(info)));
                std::panic::set_hook(hook);
            }
        }
    }
}

pub fn run(app: &mut App) -> io::Result<()> {
    let guard = TerminalGuard::new();
    let mut output = io::stdout();
    guard.state.enter(&mut output, enable_raw_mode)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(output))?;
    let mut redraw = true;
    while !app.should_exit {
        if redraw {
            // Present each frame atomically where the terminal supports it.
            queue!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
            terminal.draw(|frame| app.draw(frame))?;
            execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
        }
        redraw = false;
        if event::poll(std::time::Duration::from_millis(200))? {
            app.handle_event(event::read()?);
            redraw = true;
        }
        redraw |= app.tick();
    }
    app.finish_state()
}

#[cfg(all(test, unix))]
#[path = "terminal_tests.rs"]
mod tests;
