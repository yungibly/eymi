//! Source-preserving local file I/O. Atomic replacement is not a cross-process lock.
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalChange {
    Unchanged,
    Changed,
    Missing,
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: Option<SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64, u32, u64),
}

impl Fingerprint {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                    metadata.mode(),
                    metadata.nlink(),
                )
            },
        }
    }
}

pub struct FileState {
    pub path: PathBuf,
    baseline: Option<Vec<u8>>,
    observed: Option<Fingerprint>,
    external: ExternalChange,
}

impl FileState {
    #[cfg(test)]
    pub fn open(path: PathBuf) -> io::Result<(String, Self)> {
        let baseline = read_optional(&path)?;
        Self::from_baseline(path, baseline)
    }

    /// Open like `open`, accepting at most `max_bytes` of UTF-8 source. The
    /// bounded read probes at most one extra byte, even if the file grows after
    /// metadata inspection. Missing paths retain the new-empty-file behavior.
    pub fn open_bounded(path: PathBuf, max_bytes: usize) -> io::Result<(String, Self)> {
        let baseline = read_optional_with_limit(&path, Some(max_bytes))?;
        Self::from_baseline(path, baseline)
    }

    fn from_baseline(path: PathBuf, baseline: Option<Vec<u8>>) -> io::Result<(String, Self)> {
        let text = String::from_utf8(baseline.clone().unwrap_or_default()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Only valid UTF-8 files are supported; the file was not changed",
            )
        })?;
        if text.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "This file contains binary NUL bytes; choose a UTF-8 text file",
            ));
        }
        Ok((
            text,
            Self {
                path,
                baseline,
                observed: None,
                external: ExternalChange::Unchanged,
            },
        ))
    }

    pub fn new_target(path: PathBuf) -> io::Result<Self> {
        if fs::symlink_metadata(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Save As requires a new filename; existing files are protected",
            ));
        }
        Ok(Self {
            path,
            baseline: None,
            observed: None,
            external: ExternalChange::Unchanged,
        })
    }

    /// Advisory periodic check. Unchanged metadata avoids a content read; save
    /// always compares exact bytes regardless of this cache. Read errors are
    /// retried so access can recover without a detectable metadata change.
    pub fn external_change(&mut self) -> ExternalChange {
        let metadata = match fs::metadata(&self.path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                self.observed = None;
                self.external = ExternalChange::Unreadable(non_regular_error().to_string());
                return self.external.clone();
            }
            Err(error) => {
                self.observed = None;
                self.external = if error.kind() == io::ErrorKind::NotFound {
                    if self.baseline.is_none() {
                        ExternalChange::Unchanged
                    } else {
                        ExternalChange::Missing
                    }
                } else {
                    ExternalChange::Unreadable(error.to_string())
                };
                return self.external.clone();
            }
        };
        let fingerprint = Fingerprint::of(&metadata);
        if self.observed.as_ref() == Some(&fingerprint)
            && !matches!(self.external, ExternalChange::Unreadable(_))
        {
            return self.external.clone();
        }
        self.external = match open_regular_file(&self.path) {
            Ok(file) => match &self.baseline {
                Some(baseline) => match matches_baseline(file, baseline) {
                    Ok(true) => ExternalChange::Unchanged,
                    Ok(false) => ExternalChange::Changed,
                    Err(error) => ExternalChange::Unreadable(error.to_string()),
                },
                None => ExternalChange::Changed,
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if self.baseline.is_none() {
                    ExternalChange::Unchanged
                } else {
                    ExternalChange::Missing
                }
            }
            Err(error) => ExternalChange::Unreadable(error.to_string()),
        };
        self.observed = Some(fingerprint);
        self.external.clone()
    }

    /// Read an existing regular file and stage a replacement FileState. A failed
    /// reload never changes the accepted baseline. Detect replacement or writes
    /// during the read and ask the caller to retry rather than accepting a mix.
    pub fn reload_bounded(&self, max_bytes: usize) -> io::Result<(String, Self)> {
        let mut file = open_regular_file(&self.path)?;
        let before = Fingerprint::of(&file.metadata()?);
        let bytes = read_limited(&mut file, max_bytes)?;
        let after = Fingerprint::of(&file.metadata()?);
        let path = fs::metadata(&self.path)?;
        if !path.is_file() || before != after || after != Fingerprint::of(&path) {
            return Err(io::Error::other(
                "File changed while reloading; your text is intact. Try Reload again",
            ));
        }
        Self::from_baseline(self.path.clone(), Some(bytes))
    }

    pub fn save(&mut self, text: &str) -> io::Result<()> {
        self.check_baseline()?;
        // A no-edit save must preserve both bytes and file metadata.
        if self.baseline.as_deref() == Some(text.as_bytes()) {
            return Ok(());
        }
        let metadata = writable_target_metadata(&self.path)?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut builder = tempfile::Builder::new();
        // A replacement starts private, then takes the target's permissions.
        // A new file gets the usual mode after the umask, like other editors.
        #[cfg(unix)]
        if metadata.is_none() {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o666));
        }
        let mut temporary = builder.tempfile_in(parent).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
            ) {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Saving writes a new file into the folder, but the folder is read-only. Your edits are intact; use Save As",
                )
            } else {
                error
            }
        })?;
        temporary.write_all(text.as_bytes())?;
        if let Some(metadata) = metadata {
            #[cfg(unix)]
            keep_owner(temporary.as_file(), &metadata)?;
            #[cfg(target_os = "macos")]
            keep_attributes(&self.path, temporary.as_file());
            temporary
                .as_file()
                .set_permissions(metadata.permissions())?;
        }
        temporary.as_file().sync_all()?;
        self.check_baseline()?;
        // Permissions and link type can change while the temporary file is
        // written. Recheck them as well as bytes before replacing the target.
        writable_target_metadata(&self.path)?;
        if self.baseline.is_none() {
            temporary
                .persist_noclobber(&self.path)
                .map_err(|e| e.error)?;
        } else {
            temporary.persist(&self.path).map_err(|e| e.error)?;
        }
        self.baseline = Some(text.as_bytes().to_vec());
        self.observed = None;
        self.external = ExternalChange::Unchanged;
        Ok(())
    }

    fn check_baseline(&self) -> io::Result<()> {
        // A file may have grown arbitrarily since opening it. Compare a bounded
        // stream with the accepted baseline instead of allocating the new file.
        let unchanged = match open_regular_file(&self.path) {
            Ok(file) => match &self.baseline {
                Some(baseline) => matches_baseline(file, baseline)?,
                None => false,
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.baseline.is_none(),
            Err(error) => return Err(error),
        };
        if !unchanged {
            return Err(io::Error::other(
                "File changed or was removed on disk. Your edits are intact; use Ctrl+Shift+S or F4 to Save As",
            ));
        }
        Ok(())
    }
}

fn matches_baseline(mut reader: impl Read, baseline: &[u8]) -> io::Result<bool> {
    let mut buffer = [0; 8192];
    for expected in baseline.chunks(buffer.len()) {
        let actual = &mut buffer[..expected.len()];
        match reader.read_exact(actual) {
            Ok(()) if actual == expected => {}
            Ok(()) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    match reader.read_exact(&mut [0]) {
        Ok(()) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(true),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    read_optional_with_limit(path, None)
}

fn read_optional_with_limit(path: &Path, max_bytes: Option<usize>) -> io::Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(non_regular_error()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let result = match max_bytes {
        Some(limit) => open_regular_file(path).and_then(|file| read_limited(file, limit)),
        None => read_regular_file(path),
    };
    match result {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open_regular_file(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn open_regular_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular path can become a FIFO after the path metadata check.
        // O_NONBLOCK avoids waiting for a writer before we can check its type.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    // Validate the opened handle, not another pathname lookup, before reading.
    if !file.metadata()?.is_file() {
        return Err(non_regular_error());
    }
    Ok(file)
}

fn read_limited(reader: impl Read, max_bytes: usize) -> io::Result<Vec<u8>> {
    // Do not reserve based on metadata or the caller's potentially huge limit.
    // Saturation also makes usize::MAX a valid bound without addition overflow.
    let probe_limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::new();
    reader.take(probe_limit).read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("File exceeds the {max_bytes}-byte open limit; the file was not changed"),
        ));
    }
    Ok(bytes)
}

fn non_regular_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Only regular files are supported; the file was not changed",
    )
}

fn writable_target_metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::other(
            "Saving through symbolic links or non-regular files is unsupported; use Save As",
        ));
    }
    let read_only = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The file is read-only. Your edits are intact; use Save As",
        )
    };
    if metadata.permissions().readonly() {
        return Err(read_only());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(io::Error::other(
                "This file has hard links; use Save As to avoid breaking them",
            ));
        }
        // Replacing through a writable folder would bypass the file's own
        // owner, ACLs, and flags, which write bits alone do not show.
        may_write(path).map_err(|error| match error.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => read_only(),
            _ => error,
        })?;
    }
    Ok(Some(metadata))
}

#[cfg(unix)]
fn may_write(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let result =
        unsafe { libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::W_OK, libc::AT_EACCESS) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A replacement must not hand someone else's file to this user. Root, as
/// under sudo, keeps the owner; anyone keeps a group they belong to.
#[cfg(unix)]
fn keep_owner(replacement: &fs::File, target: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, fchown};
    let created = replacement.metadata()?;
    if (created.uid(), created.gid()) == (target.uid(), target.gid()) {
        return Ok(());
    }
    match fchown(replacement, Some(target.uid()), Some(target.gid())) {
        Ok(()) => Ok(()),
        Err(_) if created.uid() != target.uid() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "This file belongs to another user, and saving would make it yours. Your edits are intact; use Save As",
        )),
        // Outside the file's group, the copy keeps this user's default group.
        Err(_) => Ok(()),
    }
}

/// Finder tags, ACLs, and other extended attributes belong to the file, so
/// a replacement copies them when it can. Failing to copy never blocks a save.
#[cfg(target_os = "macos")]
fn keep_attributes(target: &Path, replacement: &fs::File) {
    use std::os::fd::AsRawFd;
    let Ok(original) = open_regular_file(target) else {
        return;
    };
    // SAFETY: both descriptors stay open for the call, which accepts a null state.
    unsafe {
        libc::fcopyfile(
            original.as_raw_fd(),
            replacement.as_raw_fd(),
            std::ptr::null_mut(),
            libc::COPYFILE_ACL | libc::COPYFILE_XATTR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_status_tracks_writes_removal_reappearance_and_nonregular_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.md");
        fs::write(&path, "base").unwrap();
        let (_, mut state) = FileState::open(path.clone()).unwrap();
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        assert!(state.observed.is_some());
        fs::write(&path, "external text").unwrap();
        assert_eq!(state.external_change(), ExternalChange::Changed);
        assert_eq!(state.external_change(), ExternalChange::Changed);
        assert!(state.save("local").is_err());
        fs::remove_file(&path).unwrap();
        assert_eq!(state.external_change(), ExternalChange::Missing);
        assert!(state.reload_bounded(1024).is_err());
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            state.external_change(),
            ExternalChange::Unreadable(_)
        ));
        fs::remove_dir(&path).unwrap();
        fs::write(&path, "base").unwrap();
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        state.save("local").unwrap();
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        assert_eq!(state.baseline.as_deref(), Some(b"local".as_slice()));
    }

    #[test]
    fn metadata_only_changes_and_noop_save_preserve_file_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.md");
        fs::write(&path, "base").unwrap();
        let (_, mut state) = FileState::open(path.clone()).unwrap();
        state.external_change();
        let writable = fs::metadata(&path).unwrap().permissions();
        let mut readonly = writable.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        let before = Fingerprint::of(&fs::metadata(&path).unwrap());
        let status = state.external_change();
        let saved = state.save("base");
        let after = Fingerprint::of(&fs::metadata(&path).unwrap());
        fs::set_permissions(&path, writable).unwrap();
        assert_eq!(status, ExternalChange::Unchanged);
        saved.unwrap();
        assert_eq!(before, after);
        // Even if the advisory fingerprint were fooled, save still checks bytes.
        fs::write(&path, "evil").unwrap();
        state.observed = Some(Fingerprint::of(&fs::metadata(&path).unwrap()));
        state.external = ExternalChange::Unchanged;
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        assert!(state.save("base").is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "evil");
    }

    #[test]
    fn reload_requires_existing_bounded_text_and_leaves_old_state_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.md");
        fs::write(&path, "base").unwrap();
        let (_, mut state) = FileState::open(path.clone()).unwrap();
        for bytes in [vec![0xff], b"null\0byte".to_vec(), b"oversized".to_vec()] {
            fs::write(&path, bytes).unwrap();
            assert!(state.reload_bounded(8).is_err());
            assert_eq!(state.baseline.as_deref(), Some(b"base".as_slice()));
        }
        fs::remove_file(&path).unwrap();
        assert!(state.reload_bounded(8).is_err());
        assert!(!path.exists());
        fs::write(&path, "é\r\n").unwrap();
        let (text, mut fresh) = state.reload_bounded(8).unwrap();
        assert_eq!(text, "é\r\n");
        fresh.save(&text).unwrap();
        assert!(state.save("base").is_err());
        fresh.save("new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn missing_new_target_is_unchanged_until_created_externally() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.md");
        let mut state = FileState::new_target(path.clone()).unwrap();
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        assert!(state.reload_bounded(10).is_err());
        fs::write(path, "").unwrap();
        assert_eq!(state.external_change(), ExternalChange::Changed);
    }

    #[cfg(unix)]
    #[test]
    fn external_unreadability_recovers_and_reload_preserves_link_rules() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.md");
        fs::write(&path, "base").unwrap();
        let (_, mut state) = FileState::open(path.clone()).unwrap();
        state.external_change();
        let writable = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let status = state.external_change();
        let reload = state.reload_bounded(20);
        fs::set_permissions(&path, writable).unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert!(matches!(status, ExternalChange::Unreadable(_)));
            assert!(reload.is_err());
        }
        assert_eq!(state.external_change(), ExternalChange::Unchanged);
        let link = dir.path().join("link.md");
        symlink(&path, &link).unwrap();
        let (_, linked) = FileState::open(link).unwrap();
        fs::write(&path, "fresh").unwrap();
        let (text, mut linked) = linked.reload_bounded(20).unwrap();
        linked.save(&text).unwrap();
        assert!(linked.save("changes").is_err());
        let hard = dir.path().join("hard.md");
        fs::hard_link(&path, &hard).unwrap();
        let (_, hard) = FileState::open(hard).unwrap();
        let (text, mut hard) = hard.reload_bounded(20).unwrap();
        hard.save(&text).unwrap();
        assert!(hard.save("changes").is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "fresh");
    }

    #[test]
    fn baseline_comparison_is_exact_and_reads_at_most_one_extra_byte() {
        use std::{cell::Cell, io::Cursor};
        struct Counted<'a> {
            read: &'a Cell<usize>,
            reader: Cursor<Vec<u8>>,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let count = self.reader.read(buffer)?;
                self.read.set(self.read.get() + count);
                Ok(count)
            }
        }
        let baseline = vec![b'x'; 9000];
        let read = Cell::new(0);
        assert!(
            !matches_baseline(
                Counted {
                    read: &read,
                    reader: Cursor::new(vec![b'x'; 100_000])
                },
                &baseline,
            )
            .unwrap()
        );
        assert_eq!(read.get(), baseline.len() + 1);
        assert!(matches_baseline(Cursor::new(&baseline), &baseline).unwrap());
        assert!(!matches_baseline(Cursor::new(&baseline[..8999]), &baseline).unwrap());
        assert!(!matches_baseline(Cursor::new(b"different"), b"original!").unwrap());
        assert!(matches_baseline(io::empty(), b"").unwrap());
        assert!(!matches_baseline(Cursor::new(b"x"), b"").unwrap());
    }

    #[test]
    fn bytes_and_noop_saves_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let bytes = b"\xef\xbb\xbf# Heading\r\n\r\nbody  \nlast\t";
        fs::write(&path, bytes).unwrap();
        let (text, mut file) = FileState::open(path.clone()).unwrap();
        file.save(&text).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn bounded_open_accepts_exact_byte_limit_and_preserves_utf8_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let source = "\u{feff}# e\u{301}界\r\nbody  \nlast\t";
        fs::write(&path, source).unwrap();
        let (text, mut file) = FileState::open_bounded(path.clone(), source.len()).unwrap();
        assert_eq!(text.as_bytes(), source.as_bytes());
        assert_eq!(file.baseline.as_deref(), Some(source.as_bytes()));
        file.save(&text).unwrap();
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        let (text, _) = FileState::open_bounded(path, usize::MAX).unwrap();
        assert_eq!(text, source);
    }

    #[test]
    fn bounded_open_rejects_oversized_bytes_without_changing_unbounded_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "12345").unwrap();
        let error = FileState::open_bounded(path.clone(), 4).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("4-byte open limit"));
        assert_eq!(fs::read(&path).unwrap(), b"12345");
        assert_eq!(FileState::open(path).unwrap().0, "12345");
    }

    #[test]
    fn bounded_open_zero_limit_accepts_only_empty_or_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.md");
        fs::write(&empty, []).unwrap();
        let (text, file) = FileState::open_bounded(empty, 0).unwrap();
        assert!(text.is_empty());
        assert_eq!(file.baseline, Some(Vec::new()));
        let missing = dir.path().join("missing.md");
        let (text, file) = FileState::open_bounded(missing.clone(), 0).unwrap();
        assert!(text.is_empty());
        assert_eq!(file.baseline, None);
        assert!(!missing.exists());
        let nonempty = dir.path().join("nonempty.md");
        fs::write(&nonempty, "x").unwrap();
        assert!(FileState::open_bounded(nonempty, 0).is_err());
    }

    #[test]
    fn bounded_open_validates_utf8_after_enforcing_the_byte_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("text.md");
        fs::write(&path, "é").unwrap();
        let error = FileState::open_bounded(path.clone(), 1).err().unwrap();
        assert!(error.to_string().contains("open limit"));
        assert_eq!(FileState::open_bounded(path.clone(), 2).unwrap().0, "é");
        fs::write(&path, [0xff]).unwrap();
        let error = FileState::open_bounded(path.clone(), 1).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("UTF-8"));
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
    }

    #[test]
    fn bounded_read_stops_after_limit_plus_one_when_file_grows_after_open() {
        use std::io::Seek;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("growing.md");
        fs::write(&path, "base").unwrap();
        let mut file = open_regular_file(&path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 4);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[b'x'; 32_768])
            .unwrap();
        let error = read_limited(&mut file, 4).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(file.stream_position().unwrap(), 5);
        assert_eq!(fs::metadata(path).unwrap().len(), 32_772);
    }

    #[test]
    fn bounded_open_rejects_directories() {
        let dir = tempfile::tempdir().unwrap();
        let error = FileState::open_bounded(dir.path().to_owned(), usize::MAX)
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn bounded_open_preserves_link_and_nonblocking_fifo_behavior() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        let link = dir.path().join("link.md");
        let hardlink = dir.path().join("hardlink.md");
        fs::write(&target, "source").unwrap();
        symlink(&target, &link).unwrap();
        fs::hard_link(&target, &hardlink).unwrap();
        for path in [link, hardlink] {
            let (text, mut file) = FileState::open_bounded(path, 6).unwrap();
            assert_eq!(text, "source");
            file.save(&text).unwrap();
            assert!(file.save("changed").is_err());
        }
        assert_eq!(fs::read(&target).unwrap(), b"source");
        let fifo = dir.path().join("pipe");
        make_fifo(&fifo);
        let fifo_link = dir.path().join("pipe-link");
        symlink(&fifo, &fifo_link).unwrap();
        for path in [fifo.clone(), fifo_link] {
            assert_rejected_promptly(move || {
                FileState::open_bounded(path, 8 * 1024 * 1024).map(|_| ())
            });
        }
        // Exercise replacement after pathname preflight: validating the opened
        // handle must still reject the FIFO without waiting for a writer.
        assert_rejected_promptly(move || {
            open_regular_file(&fifo)
                .and_then(|file| read_limited(file, 8))
                .map(|_| ())
        });
    }

    #[test]
    fn external_change_or_deletion_is_never_silently_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "base").unwrap();
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        fs::write(&path, "external").unwrap();
        assert!(file.save("local").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        fs::remove_file(&path).unwrap();
        assert!(file.save("local").is_err());
        assert!(!path.exists());
    }

    #[test]
    fn new_targets_refuse_overwrite_and_invalid_utf8_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let mut file = FileState::new_target(path.clone()).unwrap();
        fs::write(&path, [0xff]).unwrap();
        assert!(FileState::open(path.clone()).is_err());
        assert!(file.save("new").is_err());
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
    }

    #[test]
    fn atomic_save_preserves_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        }
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        file.save("new\r\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new\r\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn saving_needs_write_access_rather_than_any_write_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "base").unwrap();
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        // Its group may write, but not its owner, this user. The folder
        // alone would still allow replacing it.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o464)).unwrap();
        let saved = file.save("changes");
        let bytes = fs::read(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(saved.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(bytes, b"base");
            file.save("changes").unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), b"changes");
    }

    #[cfg(unix)]
    #[test]
    fn replacements_keep_their_group_and_new_files_follow_the_umask() {
        use std::os::unix::fs::{MetadataExt, chown};
        let dir = tempfile::tempdir().unwrap();
        let created = dir.path().join("created.md");
        fs::File::create(&created).unwrap();
        let path = dir.path().join("note.md");
        FileState::new_target(path.clone())
            .unwrap()
            .save("new")
            .unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().mode() & 0o7777;
        assert_eq!(mode(&path), mode(&created));
        // Another group this user belongs to, if any.
        let default = fs::metadata(&path).unwrap().gid();
        let mut groups = vec![0; 256];
        // SAFETY: the buffer holds as many groups as the call is told.
        let count = unsafe { libc::getgroups(groups.len() as i32, groups.as_mut_ptr()) };
        groups.truncate(count.max(0) as usize);
        let Some(group) = groups.into_iter().find(|group| *group != default) else {
            return;
        };
        chown(&path, None, Some(group)).unwrap();
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        file.save("changed").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"changed");
        assert_eq!(fs::metadata(&path).unwrap().gid(), group);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replacements_keep_extended_attributes() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "base").unwrap();
        let xattr = |args: &[&str]| {
            let output = Command::new("xattr")
                .args(args)
                .arg(&path)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout).unwrap()
        };
        xattr(&["-w", "org.eymi.test", "kept"]);
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        file.save("changed").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"changed");
        assert_eq!(xattr(&["-p", "org.eymi.test"]).trim(), "kept");
    }

    #[test]
    fn read_only_targets_allow_noop_but_reject_edits_without_advancing_baseline() {
        // Cover files already read-only at open and chmod after opening.
        for readonly_at_open in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("note.md");
            let original = b"\xef\xbb\xbfbase\r\nlast  ";
            fs::write(&path, original).unwrap();
            let writable = fs::metadata(&path).unwrap().permissions();
            let mut readonly = writable.clone();
            readonly.set_readonly(true);
            if readonly_at_open {
                fs::set_permissions(&path, readonly.clone()).unwrap();
            }
            let (text, mut file) = FileState::open(path.clone()).unwrap();
            fs::set_permissions(&path, readonly).unwrap();
            let noop = file.save(&text);
            let changed = file.save("local changes");
            let disk_bytes = fs::read(&path).unwrap();
            let remains_readonly = fs::metadata(&path).unwrap().permissions().readonly();
            // Restore before assertions so Windows can clean up the temp file.
            fs::set_permissions(&path, writable).unwrap();

            assert!(noop.is_ok());
            assert_eq!(changed.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(disk_bytes, original);
            assert!(remains_readonly);
            assert_eq!(file.baseline.as_deref(), Some(original.as_slice()));
            // A rejected save leaves the accepted disk baseline usable.
            file.save("local changes").unwrap();
            assert_eq!(fs::read(path).unwrap(), b"local changes");
        }
    }

    #[test]
    fn directory_targets_are_rejected_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let error = FileState::open(dir.path().to_owned()).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) {
        assert!(
            std::process::Command::new("mkfifo")
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
    }

    #[cfg(unix)]
    fn assert_rejected_promptly(action: impl FnOnce() -> io::Result<()> + Send + 'static) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender.send(action()).unwrap();
        });
        let error = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("file operation blocked on a non-regular target")
            .unwrap_err();
        worker.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn fifo_open_and_stale_disk_save_are_rejected_without_waiting_for_writer() {
        use std::os::unix::fs::FileTypeExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "baseline").unwrap();
        let (_, mut file) = FileState::open(path.clone()).unwrap();
        fs::remove_file(&path).unwrap();
        make_fifo(&path);

        let open_path = path.clone();
        assert_rejected_promptly(move || FileState::open(open_path).map(|_| ()));
        assert_rejected_promptly(move || {
            let result = file.save("local changes");
            assert_eq!(file.baseline.as_deref(), Some(b"baseline".as_slice()));
            result
        });
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
    }

    #[cfg(unix)]
    #[test]
    fn opened_handle_check_rejects_fifo_even_without_path_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pipe");
        make_fifo(&path);
        // Exercise the path used if a regular file becomes a FIFO between the
        // metadata preflight and open. No writer exists to release a blocking open.
        assert_rejected_promptly(move || read_regular_file(&path).map(|_| ()));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_saves_keep_conservative_behavior() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.md");
        let link = dir.path().join("link.md");
        let hardlink = dir.path().join("hardlink.md");
        fs::write(&target, "original").unwrap();
        symlink(&target, &link).unwrap();
        let (text, mut linked) = FileState::open(link.clone()).unwrap();
        linked.save(&text).unwrap();
        assert!(linked.save("edits").is_err());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::hard_link(&target, &hardlink).unwrap();
        let (text, mut hardlinked) = FileState::open(hardlink.clone()).unwrap();
        hardlinked.save(&text).unwrap();
        assert!(hardlinked.save("edits").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert_eq!(fs::read(&hardlink).unwrap(), b"original");

        let fifo = dir.path().join("fifo");
        let fifo_link = dir.path().join("fifo-link");
        make_fifo(&fifo);
        symlink(&fifo, &fifo_link).unwrap();
        assert_rejected_promptly(move || FileState::open(fifo_link).map(|_| ()));
    }
}
