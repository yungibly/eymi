//! Source-preserving local file I/O. Atomic replacement is not a cross-process lock.
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub struct FileState {
    pub path: PathBuf,
    baseline: Option<Vec<u8>>,
}

impl FileState {
    pub fn open(path: PathBuf) -> io::Result<(String, Self)> {
        let baseline = read_optional(&path)?;
        let text = String::from_utf8(baseline.clone().unwrap_or_default()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Only valid UTF-8 files are supported; the file was not changed",
            )
        })?;
        Ok((text, Self { path, baseline }))
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
        })
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
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(text.as_bytes())?;
        if let Some(metadata) = metadata {
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
        Ok(())
    }

    fn check_baseline(&self) -> io::Result<()> {
        if read_optional(&self.path)? != self.baseline {
            return Err(io::Error::other(
                "File changed or was removed on disk. Your edits are intact; use Ctrl+Shift+S or F4 to Save As",
            ));
        }
        Ok(())
    }
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(non_regular_error()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    match read_regular_file(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A regular path can become a FIFO after the path metadata check.
        // O_NONBLOCK avoids waiting for a writer before we can check its type.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    // Validate the opened handle, not another pathname lookup, before reading.
    if !file.metadata()?.is_file() {
        return Err(non_regular_error());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
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
    if metadata.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "The file is read-only. Your edits are intact; use Save As",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(io::Error::other(
                "This file has hard links; use Save As to avoid breaking them",
            ));
        }
    }
    Ok(Some(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

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
