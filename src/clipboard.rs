//! Lazy, serialized native clipboard access with a bounded UI wait.
use std::{sync::mpsc, thread, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Empty,
    Unavailable(String),
}

pub trait Backend: Send {
    fn get_text(&mut self) -> Result<String, Error>;
    fn set_text(&mut self, text: &str) -> Result<(), Error>;
}

pub struct Clipboard {
    backend: Box<dyn Backend>,
    internal: String,
    fallback: Option<String>,
}

impl Clipboard {
    pub fn internal() -> Self {
        Self::with_backend(Box::new(InternalOnly))
    }
    pub fn remote() -> Self {
        let mut clipboard = Self::internal();
        clipboard.fallback = Some("SSH session uses internal clipboard; use terminal paste".into());
        clipboard
    }
    pub fn system() -> Self {
        Self::with_backend(Box::new(Worker::new(
            Box::new(|| Box::new(Native { clipboard: None })),
            Duration::from_millis(350),
        )))
    }
    pub fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            internal: String::new(),
            fallback: None,
        }
    }

    /// A native failure or remote session has switched this session to the
    /// internal clipboard. UI feedback must make that capability change clear.
    pub fn uses_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// Retain source text internally even if the system clipboard fails.
    pub fn copy(&mut self, text: &str) -> String {
        self.internal = text.to_owned();
        if let Some(reason) = &self.fallback {
            return format!("Copied to internal clipboard only · {reason}");
        }
        match self.backend.set_text(text) {
            Ok(()) => "Copied source to system clipboard".into(),
            Err(error) => {
                let reason = describe(error);
                self.fallback = Some(reason.clone());
                format!("Copied to internal clipboard only · {reason}")
            }
        }
    }

    pub fn paste(&mut self) -> (Option<String>, String) {
        if let Some(reason) = &self.fallback {
            return self.internal_paste(reason);
        }
        match self.backend.get_text() {
            Ok(text) if !text.is_empty() => (Some(text), "Pasted from system clipboard".into()),
            Ok(_) | Err(Error::Empty) => (
                None,
                "System clipboard has no text; selection unchanged".into(),
            ),
            Err(error) => {
                let reason = describe(error);
                self.fallback = Some(reason.clone());
                self.internal_paste(&reason)
            }
        }
    }

    fn internal_paste(&self, reason: &str) -> (Option<String>, String) {
        if self.internal.is_empty() {
            (
                None,
                format!("Internal clipboard is empty; use terminal paste · {reason}"),
            )
        } else {
            (
                Some(self.internal.clone()),
                format!("Pasted from internal clipboard · {reason}"),
            )
        }
    }
}

pub fn remote_session() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .into_iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

fn describe(error: Error) -> String {
    match error {
        Error::Empty => "System clipboard has no text".into(),
        Error::Unavailable(message) => message,
    }
}

struct InternalOnly;
impl Backend for InternalOnly {
    fn get_text(&mut self) -> Result<String, Error> {
        Err(Error::Unavailable(
            "System clipboard unavailable in this session".into(),
        ))
    }
    fn set_text(&mut self, _: &str) -> Result<(), Error> {
        self.get_text().map(|_| ())
    }
}

struct Native {
    clipboard: Option<arboard::Clipboard>,
}
impl Native {
    fn get(&mut self) -> Result<&mut arboard::Clipboard, Error> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(native_error)?);
        }
        Ok(self.clipboard.as_mut().unwrap())
    }
}
impl Backend for Native {
    fn get_text(&mut self) -> Result<String, Error> {
        self.get()?.get_text().map_err(native_error)
    }
    fn set_text(&mut self, text: &str) -> Result<(), Error> {
        self.get()?.set_text(text).map_err(native_error)
    }
}
fn native_error(error: arboard::Error) -> Error {
    if matches!(error, arboard::Error::ContentNotAvailable) {
        Error::Empty
    } else {
        Error::Unavailable(format!("System clipboard unavailable: {error}"))
    }
}

type Factory = Box<dyn FnOnce() -> Box<dyn Backend> + Send>;
enum Action {
    Read,
    Write(String),
}
struct Request {
    action: Action,
    reply: mpsc::Sender<Result<Option<String>, Error>>,
}

struct Worker {
    factory: Option<Factory>,
    requests: Option<mpsc::Sender<Request>>,
    disabled: Option<String>,
    timeout: Duration,
    completed: Option<mpsc::Receiver<()>>,
}

impl Worker {
    fn new(factory: Factory, timeout: Duration) -> Self {
        Self {
            factory: Some(factory),
            requests: None,
            disabled: None,
            timeout,
            completed: None,
        }
    }

    fn request(&mut self, action: Action) -> Result<Option<String>, Error> {
        if let Some(reason) = &self.disabled {
            return Err(Error::Unavailable(reason.clone()));
        }
        if self.requests.is_none() {
            let (sender, receiver) = mpsc::channel::<Request>();
            let (complete, completed) = mpsc::channel();
            let factory = self
                .factory
                .take()
                .ok_or_else(|| Error::Unavailable("System clipboard worker unavailable".into()))?;
            thread::Builder::new()
                .name("marklane-clipboard".into())
                .spawn(move || {
                    // The worker owns the native handle for its lifetime, including X11 selection ownership.
                    let mut backend = factory();
                    while let Ok(request) = receiver.recv() {
                        let result = match request.action {
                            Action::Read => backend.get_text().map(Some),
                            Action::Write(text) => backend.set_text(&text).map(|()| None),
                        };
                        let _ = request.reply.send(result);
                    }
                    drop(backend);
                    let _ = complete.send(());
                })
                .map_err(|error| {
                    Error::Unavailable(format!("Cannot start system clipboard worker: {error}"))
                })?;
            self.requests = Some(sender);
            self.completed = Some(completed);
        }
        let (reply, response) = mpsc::channel();
        let sent = self
            .requests
            .as_ref()
            .unwrap()
            .send(Request { action, reply });
        if sent.is_ok() {
            match response.recv_timeout(self.timeout) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => self.disabled = Some("System clipboard timed out (completion unknown); native access disabled for this session".into()),
                Err(mpsc::RecvTimeoutError::Disconnected) => self.disabled = Some("System clipboard worker stopped; native access disabled for this session".into()),
            }
        } else {
            self.disabled = Some(
                "System clipboard worker stopped; native access disabled for this session".into(),
            );
        }
        // Never spawn another worker after a timeout or queue more operations on the stalled one.
        self.requests = None;
        Err(Error::Unavailable(self.disabled.clone().unwrap()))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.requests = None;
        if let Some(completed) = self.completed.take() {
            // Allow normal native teardown to finish, but never hang quitting.
            let _ = completed.recv_timeout(self.timeout.min(Duration::from_millis(150)));
        }
    }
}
impl Backend for Worker {
    fn get_text(&mut self) -> Result<String, Error> {
        self.request(Action::Read)?.ok_or(Error::Empty)
    }
    fn set_text(&mut self, text: &str) -> Result<(), Error> {
        self.request(Action::Write(text.to_owned())).map(|_| ())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    pub struct State {
        pub text: String,
        pub writes: Vec<String>,
        pub reads: usize,
        pub error: Option<Error>,
    }
    pub struct Fake(pub Arc<Mutex<State>>);
    impl Backend for Fake {
        fn get_text(&mut self) -> Result<String, Error> {
            let mut state = self.0.lock().unwrap();
            state.reads += 1;
            if let Some(error) = &state.error {
                Err(error.clone())
            } else {
                Ok(state.text.clone())
            }
        }
        fn set_text(&mut self, text: &str) -> Result<(), Error> {
            let mut state = self.0.lock().unwrap();
            state.writes.push(text.to_owned());
            if let Some(error) = &state.error {
                Err(error.clone())
            } else {
                state.text = text.into();
                Ok(())
            }
        }
    }

    #[test]
    fn empty_native_paste_does_not_resurrect_internal_copy() {
        let state = Arc::new(Mutex::new(State::default()));
        let mut clipboard = Clipboard::with_backend(Box::new(Fake(state.clone())));
        clipboard.copy("older internal text");
        state.lock().unwrap().text.clear();
        assert_eq!(clipboard.paste().0, None);
        state.lock().unwrap().error = Some(Error::Empty);
        assert_eq!(clipboard.paste().0, None);
        state.lock().unwrap().error = Some(Error::Unavailable("offline".into()));
        let (text, message) = clipboard.paste();
        assert_eq!(text.as_deref(), Some("older internal text"));
        assert!(message.contains("internal clipboard"));
    }

    #[test]
    fn worker_starts_only_on_request_and_serializes_native_access() {
        let starts = Arc::new(AtomicUsize::new(0));
        let count = starts.clone();
        let state = Arc::new(Mutex::new(State::default()));
        let fake = state.clone();
        let mut worker = Worker::new(
            Box::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Box::new(Fake(fake))
            }),
            Duration::from_secs(1),
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        worker.set_text("**source**\r\n").unwrap();
        assert_eq!(worker.get_text().unwrap(), "**source**\r\n");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(state.lock().unwrap().reads, 1);
    }

    #[test]
    fn failed_native_copy_switches_to_internal_instead_of_pasting_old_native_text() {
        let state = Arc::new(Mutex::new(State {
            text: "stale system value".into(),
            error: Some(Error::Unavailable("busy".into())),
            ..State::default()
        }));
        let mut clipboard = Clipboard::with_backend(Box::new(Fake(state.clone())));
        assert!(
            clipboard
                .copy("new retained copy")
                .contains("internal clipboard only")
        );
        state.lock().unwrap().error = None;
        let (text, message) = clipboard.paste();
        assert_eq!(text.as_deref(), Some("new retained copy"));
        assert!(message.contains("internal clipboard"));
        assert_eq!(state.lock().unwrap().reads, 0);
    }

    #[test]
    fn headless_and_remote_backends_are_internal_only() {
        for mut clipboard in [Clipboard::internal(), Clipboard::remote()] {
            assert!(clipboard.copy("source").contains("internal clipboard only"));
            assert_eq!(clipboard.paste().0.as_deref(), Some("source"));
        }
        assert!(Clipboard::remote().paste().1.contains("SSH"));
    }

    #[test]
    fn normal_shutdown_waits_for_backend_drop() {
        struct DropSpy(Arc<AtomicUsize>);
        impl Backend for DropSpy {
            fn get_text(&mut self) -> Result<String, Error> {
                Ok("text".into())
            }
            fn set_text(&mut self, _: &str) -> Result<(), Error> {
                Ok(())
            }
        }
        impl Drop for DropSpy {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let spy = dropped.clone();
        let mut worker = Worker::new(
            Box::new(move || Box::new(DropSpy(spy))),
            Duration::from_secs(1),
        );
        worker.get_text().unwrap();
        drop(worker);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn timeout_disables_worker_without_retry_or_unbounded_ui_wait() {
        struct Blocked(mpsc::Receiver<()>);
        impl Backend for Blocked {
            fn get_text(&mut self) -> Result<String, Error> {
                let _ = self.0.recv();
                Ok("late".into())
            }
            fn set_text(&mut self, _: &str) -> Result<(), Error> {
                self.get_text().map(|_| ())
            }
        }
        let (release, wait) = mpsc::channel();
        let mut worker = Worker::new(
            Box::new(move || Box::new(Blocked(wait))),
            Duration::from_millis(10),
        );
        assert!(
            matches!(worker.get_text(), Err(Error::Unavailable(message)) if message.contains("timed out"))
        );
        assert!(worker.requests.is_none());
        assert!(worker.get_text().is_err());
        release.send(()).unwrap();
    }
}
