//! Private crash recovery. Original document paths are metadata, never write targets.
//!
//! A Store owns one locked session. Discovery leases abandoned sessions until all
//! their Candidates are dropped. Snapshot generations are published and synced
//! before older generations are retired, so failed updates retain the last good
//! checkpoint. Drop deliberately preserves snapshots; use finish_clean explicitly.
use marklane::Selection;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 64 * 1024;
const MAX_LABEL_BYTES: usize = 4096;
const MAX_RECORD_BYTES: usize = MAX_SOURCE_BYTES + MAX_PATH_BYTES + MAX_LABEL_BYTES + 128;
const MAX_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_SESSIONS: usize = 128;
const MAX_RECORDS: usize = 64;
const MAX_ENTRIES: usize = 256;
const MAGIC: &[u8; 8] = b"MLREC001";
const LOCK_NAME: &str = "session.lock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub source: String,
    pub original_path: Option<PathBuf>,
    pub selection: Selection,
    pub live: bool,
    pub markdown: bool,
    pub label: String,
}

struct Lease {
    directory: PathBuf,
    directory_handle: File,
    lock: File,
}

pub struct Candidate {
    /// Stable across checkpoint generations: session ID / tab ID.
    pub id: String,
    pub snapshot: Snapshot,
    lease: Arc<Lease>,
    records: Vec<(PathBuf, u32)>,
    issuer: String,
    issued_sequence: u64,
    source_identity: (usize, u32),
}

struct DiscoveredTab {
    generation: u64,
    snapshot: Snapshot,
    records: Vec<(PathBuf, u32)>,
}

#[derive(Default)]
pub struct Listing {
    pub candidates: Vec<Candidate>,
    pub diagnostics: Vec<String>,
}

struct Owned {
    path: PathBuf,
    generation: u64,
    sequence: u64,
}

pub struct Store {
    root: PathBuf,
    root_handle: File,
    session: PathBuf,
    session_name: String,
    session_handle: File,
    lock: Option<File>,
    owned: HashMap<u64, Owned>,
    sequence: u64,
}

impl Store {
    /// The caller chooses the state directory. Tests should always inject a temp directory.
    /// Existing state directories must already be private; they are never chmodded silently.
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        if !cfg!(unix) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Durable private recovery currently requires a Unix filesystem",
            ));
        }
        let root = directory.as_ref().to_owned();
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&root)?;
        let root_handle = open_private_directory(&root)?;
        let mut session_builder = tempfile::Builder::new();
        session_builder.prefix("session-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            session_builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let temporary = session_builder.tempdir_in(&root)?;
        let session_handle = open_private_directory(temporary.path())?;
        let lock = create_private(&temporary.path().join(LOCK_NAME))?;
        lock.try_lock().map_err(lock_error)?;
        lock.sync_all()?;
        sync_directory(&session_handle)?;
        sync_directory(&root_handle)?;
        let session = temporary.keep();
        let session_name = session.file_name().unwrap().to_string_lossy().into_owned();
        Ok(Self {
            root,
            root_handle,
            session,
            session_name,
            session_handle,
            lock: Some(lock),
            owned: HashMap::new(),
            sequence: 0,
        })
    }

    /// Checkpoint all user-visible source exactly. A failed precommit update leaves
    /// the previous good generation intact; original_path is never opened.
    pub fn checkpoint(&mut self, tab_id: u64, snapshot: &Snapshot) -> io::Result<()> {
        self.check_owned_session()?;
        let generation = self.owned.get(&tab_id).map_or(Ok(1), |record| {
            record
                .generation
                .checked_add(1)
                .ok_or_else(|| invalid("Recovery generation overflow"))
        })?;
        if session_entries(&self.session)?.len() >= MAX_ENTRIES - 1 {
            return Err(invalid(
                "Recovery directory is full; previous checkpoints retained",
            ));
        }
        let bytes = encode(tab_id, generation, snapshot)?;
        let target = self.session.join(record_name(tab_id, generation));
        let mut temporary = tempfile::Builder::new()
            .prefix(".checkpoint-")
            .tempfile_in(&self.session)?;
        private_permissions(temporary.as_file())?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        self.check_owned_session()?;
        let published = temporary
            .persist_noclobber(&target)
            .map_err(|error| error.error)?;
        // Some platforms implement no-clobber publication with link + unlink.
        // A failed unlink must not retire the old snapshot while leaving a new
        // hard-linked record that conservative discovery would refuse.
        private_metadata(&published.metadata()?, false)?;
        let visible = open_private_file(&target, false)?;
        if !same_file(&published.metadata()?, &visible.metadata()?) {
            return Err(invalid("Published recovery file identity changed"));
        }
        published.sync_all()?;
        if let Err(error) = sync_directory(&self.session_handle) {
            // Do not retire the old generation if publication could not be synced.
            // If cleanup also fails, discovery can still validate both generations.
            let _ = fs::remove_file(&target);
            return Err(error);
        }
        self.sequence = self.sequence.saturating_add(1);
        let previous = self.owned.insert(
            tab_id,
            Owned {
                path: target,
                generation,
                sequence: self.sequence,
            },
        );
        if let Some(previous) = previous {
            // Cleanup is after the durability commit point. Failure only retains
            // another valid generation, which discovery groups by stable tab ID.
            if read_record(&previous.path, MAX_RECORD_BYTES).is_ok() {
                let _ = fs::remove_file(previous.path);
                let _ = sync_directory(&self.session_handle);
            }
        }
        Ok(())
    }

    /// Remove a successfully saved or explicitly discarded buffer in this session.
    pub fn remove(&mut self, tab_id: u64) -> io::Result<()> {
        self.check_owned_session()?;
        if !self.owned.contains_key(&tab_id) {
            return Ok(());
        }
        let entries = session_entries(&self.session)?;
        let mut paths = Vec::new();
        for entry in entries {
            if let Some((id, _)) = parse_record_name(&entry)
                && id == tab_id
            {
                let path = self.session.join(entry);
                // Preflight the whole group before removing any generation.
                read_record(&path, MAX_RECORD_BYTES)?;
                paths.push(path);
            }
        }
        for path in paths {
            fs::remove_file(path)?;
        }
        sync_directory(&self.session_handle)?;
        self.owned.remove(&tab_id);
        Ok(())
    }

    /// Returns bounded snapshots plus human-readable diagnostics for retained data.
    /// Keep Candidates alive while presenting a recovery choice: each holds a lock
    /// preventing another process from offering or consuming the same session.
    pub fn list_abandoned(&self) -> io::Result<Listing> {
        self.check_owned_session()?;
        let mut listing = Listing::default();
        let mut scanned = 0;
        let mut remaining = MAX_SCAN_BYTES;
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            if index >= MAX_SESSIONS {
                listing.diagnostics.push(format!("Recovery scan stopped after {MAX_SESSIONS} state entries; remaining entries were retained."));
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    listing.diagnostics.push(error.to_string());
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str().filter(|name| valid_session_name(name)) else {
                listing.diagnostics.push(format!(
                    "Unrecognized recovery entry retained: {}",
                    entry.path().display()
                ));
                continue;
            };
            if name == self.session_name {
                continue;
            }
            let lease = match lease_session(entry.path()) {
                Ok(Some(lease)) => Arc::new(lease),
                Ok(None) => continue, // Another live editor or recovery dialog owns it.
                Err(error) => {
                    listing
                        .diagnostics
                        .push(format!("{}: {error}", entry.path().display()));
                    continue;
                }
            };
            let entries = match session_entries(&lease.directory) {
                Ok(entries) => entries,
                Err(error) => {
                    listing
                        .diagnostics
                        .push(format!("{}: {error}", lease.directory.display()));
                    continue;
                }
            };
            let mut groups = BTreeMap::<u64, DiscoveredTab>::new();
            for filename in entries {
                if filename == LOCK_NAME {
                    continue;
                }
                let Some((tab, generation)) = parse_record_name(&filename) else {
                    listing.diagnostics.push(format!(
                        "Unrecognized recovery record retained: {}",
                        lease.directory.join(filename).display()
                    ));
                    continue;
                };
                if scanned >= MAX_RECORDS || remaining == 0 {
                    listing.diagnostics.push(
                        "Recovery scan limit reached; additional records were retained.".into(),
                    );
                    break;
                }
                scanned += 1;
                let path = lease.directory.join(filename);
                let max = remaining.min(MAX_RECORD_BYTES);
                let bytes = match read_bounded(&path, max) {
                    Ok(bytes) => {
                        remaining -= bytes.len();
                        bytes
                    }
                    Err(error) => {
                        listing
                            .diagnostics
                            .push(format!("{}: {error}", path.display()));
                        continue;
                    }
                };
                let snapshot = match decode(&bytes, tab, generation) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        listing
                            .diagnostics
                            .push(format!("{}: {error}", path.display()));
                        continue;
                    }
                };
                let fingerprint = crc32(&bytes);
                match groups.get_mut(&tab) {
                    Some(group) => {
                        if generation > group.generation {
                            group.generation = generation;
                            group.snapshot = snapshot;
                        }
                        group.records.push((path, fingerprint));
                    }
                    None => {
                        groups.insert(
                            tab,
                            DiscoveredTab {
                                generation,
                                snapshot,
                                records: vec![(path, fingerprint)],
                            },
                        );
                    }
                }
            }
            for (tab, group) in groups {
                let source_identity = (
                    group.snapshot.source.len(),
                    crc32(group.snapshot.source.as_bytes()),
                );
                listing.candidates.push(Candidate {
                    id: format!("{name}/{tab:016x}"),
                    snapshot: group.snapshot,
                    lease: lease.clone(),
                    records: group.records,
                    issuer: self.session_name.clone(),
                    issued_sequence: self.sequence,
                    source_identity,
                });
            }
            if scanned >= MAX_RECORDS || remaining == 0 {
                listing
                    .diagnostics
                    .push("Recovery scan limit reached; additional records were retained.".into());
                break;
            }
        }
        Ok(listing)
    }

    /// Consume only after this Store durably checkpointed the restored source.
    /// original_path/label may change when the UI restores as a new unsaved tab.
    pub fn consume(&mut self, candidate: &Candidate) -> io::Result<()> {
        self.check_owned_session()?;
        if candidate.issuer != self.session_name {
            return Err(invalid("Recovery candidate belongs to another store"));
        }
        if (
            candidate.snapshot.source.len(),
            crc32(candidate.snapshot.source.as_bytes()),
        ) != candidate.source_identity
        {
            return Err(invalid(
                "Recovery candidate source changed in memory; original retained",
            ));
        }
        let mut checkpointed = false;
        for owned in self
            .owned
            .values()
            .filter(|owned| owned.sequence > candidate.issued_sequence)
        {
            if read_record(&owned.path, MAX_RECORD_BYTES)?.source == candidate.snapshot.source {
                checkpointed = true;
                break;
            }
        }
        if !checkpointed {
            return Err(invalid(
                "Checkpoint the restored buffer before consuming recovery data",
            ));
        }
        verify_directory(
            &candidate.lease.directory,
            &candidate.lease.directory_handle,
        )?;
        verify_lock_path(
            &candidate.lease.directory.join(LOCK_NAME),
            &candidate.lease.lock,
        )?;
        // Verify every file before removing any of them. A changed or corrupt
        // record remains on disk; the UI can report the failure without data loss.
        for (path, expected) in &candidate.records {
            let bytes = read_bounded(path, MAX_RECORD_BYTES)?;
            if crc32(&bytes) != *expected {
                return Err(invalid("Recovery record changed after discovery; retained"));
            }
        }
        for (path, _) in &candidate.records {
            fs::remove_file(path)?;
        }
        sync_directory(&candidate.lease.directory_handle)?;
        // Never remove unknown/corrupt records. Empty sessions can be retired.
        if session_entries(&candidate.lease.directory)?
            .iter()
            .all(|name| name == LOCK_NAME)
        {
            fs::remove_file(candidate.lease.directory.join(LOCK_NAME))?;
            fs::remove_dir(&candidate.lease.directory)?;
            sync_directory(&self.root_handle)?;
        }
        Ok(())
    }

    /// Explicit orderly cleanup; Drop always keeps recovery data.
    pub fn finish_clean(&mut self) -> io::Result<()> {
        self.check_owned_session()?;
        for entry in session_entries(&self.session)? {
            if entry == LOCK_NAME {
                continue;
            }
            let (tab, generation) = parse_record_name(&entry)
                .ok_or_else(|| invalid("Unexpected recovery entries retained during cleanup"))?;
            if self
                .owned
                .get(&tab)
                .is_none_or(|owned| generation > owned.generation)
            {
                return Err(invalid("Unowned recovery records retained during cleanup"));
            }
            read_record(&self.session.join(entry), MAX_RECORD_BYTES)?;
        }
        let tabs: Vec<_> = self.owned.keys().copied().collect();
        for tab in tabs {
            self.remove(tab)?;
        }
        if session_entries(&self.session)?
            .iter()
            .any(|name| name != LOCK_NAME)
        {
            return Err(invalid(
                "Unexpected recovery records retained during cleanup",
            ));
        }
        fs::remove_file(self.session.join(LOCK_NAME))?;
        fs::remove_dir(&self.session)?;
        sync_directory(&self.root_handle)?;
        self.lock = None;
        Ok(())
    }

    fn check_owned_session(&self) -> io::Result<()> {
        let lock = self
            .lock
            .as_ref()
            .ok_or_else(|| invalid("Recovery store is closed"))?;
        verify_directory(&self.root, &self.root_handle)?;
        verify_directory(&self.session, &self.session_handle)?;
        verify_lock_path(&self.session.join(LOCK_NAME), lock)
    }
}

fn valid_session_name(name: &str) -> bool {
    name.starts_with("session-")
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
fn record_name(tab: u64, generation: u64) -> String {
    format!("tab-{tab:016x}-{generation:016x}.snapshot")
}
fn parse_record_name(name: &OsString) -> Option<(u64, u64)> {
    let value = name
        .to_str()?
        .strip_prefix("tab-")?
        .strip_suffix(".snapshot")?;
    let (tab, generation) = value.split_once('-')?;
    if tab.len() != 16 || generation.len() != 16 {
        return None;
    }
    Some((
        u64::from_str_radix(tab, 16).ok()?,
        u64::from_str_radix(generation, 16).ok()?,
    ))
}
fn session_entries(directory: &Path) -> io::Result<Vec<OsString>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        if entries.len() >= MAX_ENTRIES {
            return Err(invalid("Too many recovery directory entries; all retained"));
        }
        entries.push(entry?.file_name());
    }
    entries.sort();
    Ok(entries)
}
fn lease_session(directory: PathBuf) -> io::Result<Option<Lease>> {
    let directory_handle = open_private_directory(&directory)?;
    let lock = open_private_file(&directory.join(LOCK_NAME), true)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(error)) => return Err(error),
    }
    verify_directory(&directory, &directory_handle)?;
    verify_lock_path(&directory.join(LOCK_NAME), &lock)?;
    Ok(Some(Lease {
        directory,
        directory_handle,
        lock,
    }))
}
fn lock_error(error: TryLockError) -> io::Error {
    match error {
        TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            "Recovery session is already locked",
        ),
        TryLockError::Error(error) => error,
    }
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn private_permissions(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
fn private_metadata(metadata: &fs::Metadata, directory: bool) -> io::Result<()> {
    if metadata.file_type().is_symlink()
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(invalid(
            "Recovery entries must be real directories or regular files, never symlinks",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Recovery entries must be private and owned by the current user",
            ));
        }
        if !directory && metadata.nlink() != 1 {
            return Err(invalid("Hard-linked recovery files are refused"));
        }
    }
    Ok(())
}
fn no_follow(options: &mut OpenOptions, directory: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            libc::O_NOFOLLOW | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 },
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000 | if directory { 0x02000000 } else { 0 });
    }
}
fn open_private_directory(path: &Path) -> io::Result<File> {
    private_metadata(&fs::symlink_metadata(path)?, true)?;
    let mut options = OpenOptions::new();
    options.read(true);
    no_follow(&mut options, true);
    let file = options.open(path)?;
    private_metadata(&file.metadata()?, true)?;
    Ok(file)
}
fn open_private_file(path: &Path, write: bool) -> io::Result<File> {
    private_metadata(&fs::symlink_metadata(path)?, false)?;
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    no_follow(&mut options, false);
    let file = options.open(path)?;
    private_metadata(&file.metadata()?, false)?;
    Ok(file)
}
fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    no_follow(&mut options, false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    private_metadata(&file.metadata()?, false)?;
    Ok(file)
}
fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        left.is_dir() == right.is_dir() && left.created().ok() == right.created().ok()
    }
}
fn verify_directory(path: &Path, handle: &File) -> io::Result<()> {
    let current = open_private_directory(path)?;
    if !same_file(&current.metadata()?, &handle.metadata()?) {
        return Err(invalid("Recovery directory identity changed"));
    }
    Ok(())
}
fn verify_lock_path(path: &Path, handle: &File) -> io::Result<()> {
    let current = open_private_file(path, false)?;
    if !same_file(&current.metadata()?, &handle.metadata()?) {
        return Err(invalid("Recovery session lock identity changed"));
    }
    Ok(())
}
fn sync_directory(directory: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        directory.sync_all()?;
    }
    Ok(())
}
fn read_bounded(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let file = open_private_file(path, false)?;
    if file.metadata()?.len() > max as u64 {
        return Err(invalid(
            "Recovery snapshot exceeds the bounded read limit; retained",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid(
            "Recovery snapshot grew beyond the bounded read limit; retained",
        ));
    }
    Ok(bytes)
}
fn read_record(path: &Path, max: usize) -> io::Result<Snapshot> {
    let (tab, generation) = parse_record_name(
        &path
            .file_name()
            .ok_or_else(|| invalid("Missing recovery filename"))?
            .to_owned(),
    )
    .ok_or_else(|| invalid("Invalid recovery filename"))?;
    decode(&read_bounded(path, max)?, tab, generation)
}

fn encode(tab: u64, generation: u64, snapshot: &Snapshot) -> io::Result<Vec<u8>> {
    validate_snapshot(snapshot)?;
    let (path_kind, path) = encode_path(snapshot.original_path.as_deref())?;
    let mut payload =
        Vec::with_capacity(48 + snapshot.label.len() + path.len() + snapshot.source.len());
    for value in [
        tab,
        generation,
        snapshot.selection.anchor as u64,
        snapshot.selection.head as u64,
    ] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.push(u8::from(snapshot.live) | u8::from(snapshot.markdown) << 1);
    payload.push(path_kind);
    payload.extend_from_slice(&(snapshot.label.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(path.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(snapshot.source.len() as u64).to_le_bytes());
    payload.extend_from_slice(snapshot.label.as_bytes());
    payload.extend_from_slice(&path);
    payload.extend_from_slice(snapshot.source.as_bytes());
    let mut bytes = Vec::with_capacity(20 + payload.len());
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&crc32(&payload).to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}
fn decode(bytes: &[u8], tab: u64, generation: u64) -> io::Result<Snapshot> {
    let mut input = Reader(bytes);
    if input.take(8)? != MAGIC {
        return Err(invalid("Unknown recovery format/version; retained"));
    }
    let length = input.u64()?;
    let checksum = input.u32()?;
    if length != input.0.len() as u64 || checksum != crc32(input.0) {
        return Err(invalid("Truncated or corrupt recovery checksum; retained"));
    }
    if input.u64()? != tab || input.u64()? != generation {
        return Err(invalid("Recovery record identity mismatch; retained"));
    }
    let anchor = usize::try_from(input.u64()?).map_err(|_| invalid("Selection offset overflow"))?;
    let head = usize::try_from(input.u64()?).map_err(|_| invalid("Selection offset overflow"))?;
    let flags = input.take(1)?[0];
    let kind = input.take(1)?[0];
    if flags > 3 {
        return Err(invalid("Invalid recovery flags"));
    }
    let label_length = input.u32()? as usize;
    let path_length = input.u32()? as usize;
    let source_length =
        usize::try_from(input.u64()?).map_err(|_| invalid("Source length overflow"))?;
    if label_length > MAX_LABEL_BYTES
        || path_length > MAX_PATH_BYTES
        || source_length > MAX_SOURCE_BYTES
    {
        return Err(invalid("Recovery field exceeds size limit"));
    }
    let label = std::str::from_utf8(input.take(label_length)?)
        .map_err(|_| invalid("Recovery label is not UTF-8"))?
        .to_owned();
    let original_path = decode_path(kind, input.take(path_length)?)?;
    let source = std::str::from_utf8(input.take(source_length)?)
        .map_err(|_| invalid("Recovery source is not UTF-8"))?
        .to_owned();
    if !input.0.is_empty() {
        return Err(invalid("Trailing recovery data; retained"));
    }
    let snapshot = Snapshot {
        source,
        original_path,
        selection: Selection { anchor, head },
        live: flags & 1 != 0,
        markdown: flags & 2 != 0,
        label,
    };
    validate_snapshot(&snapshot)?;
    Ok(snapshot)
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        if n > self.0.len() {
            return Err(invalid("Truncated recovery record; retained"));
        }
        let (out, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(out)
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}
fn validate_snapshot(snapshot: &Snapshot) -> io::Result<()> {
    if snapshot.source.len() > MAX_SOURCE_BYTES {
        return Err(invalid(
            "Buffer exceeds the 16 MiB recovery limit; previous checkpoint retained",
        ));
    }
    if snapshot.label.len() > MAX_LABEL_BYTES || snapshot.label.chars().any(char::is_control) {
        return Err(invalid("Invalid recovery label"));
    }
    let mut anchor = snapshot.selection.anchor == snapshot.source.len();
    let mut head = snapshot.selection.head == snapshot.source.len();
    for (offset, _) in snapshot.source.grapheme_indices(true) {
        anchor |= offset == snapshot.selection.anchor;
        head |= offset == snapshot.selection.head;
        if anchor && head {
            break;
        }
    }
    if !anchor || !head {
        return Err(invalid(
            "Recovery selection is not on source grapheme boundaries",
        ));
    }
    Ok(())
}
fn encode_path(path: Option<&Path>) -> io::Result<(u8, Vec<u8>)> {
    let Some(path) = path else {
        return Ok((0, vec![]));
    };
    #[cfg(unix)]
    let (kind, bytes) = {
        use std::os::unix::ffi::OsStrExt;
        (1, path.as_os_str().as_bytes().to_vec())
    };
    #[cfg(not(unix))]
    let (kind, bytes) = (
        2,
        path.to_str()
            .ok_or_else(|| invalid("Recovery path cannot be encoded on this platform"))?
            .as_bytes()
            .to_vec(),
    );
    if bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) {
        return Err(invalid("Recovery path is too long or contains NUL"));
    }
    Ok((kind, bytes))
}
fn decode_path(kind: u8, bytes: &[u8]) -> io::Result<Option<PathBuf>> {
    if kind == 0 && bytes.is_empty() {
        return Ok(None);
    }
    if bytes.contains(&0) {
        return Err(invalid("Recovery path contains NUL"));
    }
    #[cfg(unix)]
    if kind == 1 {
        use std::os::unix::ffi::OsStringExt;
        return Ok(Some(PathBuf::from(OsString::from_vec(bytes.to_vec()))));
    }
    if kind == 2 {
        return Ok(Some(PathBuf::from(
            std::str::from_utf8(bytes).map_err(|_| invalid("Recovery path is not UTF-8"))?,
        )));
    }
    Err(invalid(
        "Recovery path encoding is unsupported on this platform; retained",
    ))
}
fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut i = 0;
        while i < 256 {
            let mut value = i as u32;
            let mut bit = 0;
            while bit < 8 {
                value = (value >> 1) ^ if value & 1 != 0 { 0xedb88320 } else { 0 };
                bit += 1;
            }
            table[i] = value;
            i += 1;
        }
        table
    };
    let mut value = u32::MAX;
    for byte in bytes {
        value = (value >> 8) ^ TABLE[((value ^ u32::from(*byte)) & 255) as usize];
    }
    !value
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(source: &str) -> Snapshot {
        Snapshot {
            source: source.into(),
            original_path: None,
            selection: Selection {
                anchor: 0,
                head: source.len(),
            },
            live: true,
            markdown: true,
            label: "Recovered draft.md".into(),
        }
    }
    fn state() -> tempfile::TempDir {
        let mut builder = tempfile::Builder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        builder.tempdir().unwrap()
    }
    fn write_record(path: &Path, bytes: &[u8]) {
        let mut file = create_private(path).unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }

    #[test]
    fn named_and_untitled_round_trip_exact_source_selection_and_metadata() {
        let directory = state();
        let original = directory.path().join("original.md");
        fs::write(&original, b"original").unwrap();
        let root = directory.path().join("state");
        let mut writer = Store::open(&root).unwrap();
        let mut named = snapshot("\u{feff}# Café\r\n- [ ] 界 e\u{301}\n");
        named.original_path = Some(original.clone());
        named.selection = Selection {
            anchor: named.source.len(),
            head: 3,
        };
        writer.checkpoint(41, &named).unwrap();
        writer.checkpoint(42, &snapshot("")).unwrap();
        let reader = Store::open(&root).unwrap();
        assert!(reader.list_abandoned().unwrap().candidates.is_empty());
        drop(writer);
        let listing = reader.list_abandoned().unwrap();
        assert!(listing.diagnostics.is_empty(), "{:?}", listing.diagnostics);
        assert_eq!(listing.candidates.len(), 2);
        assert_eq!(listing.candidates[0].snapshot, named);
        assert!(listing.candidates[0].id.ends_with("0000000000000029"));
        assert_eq!(listing.candidates[1].snapshot.source, "");
        assert_eq!(fs::read(original).unwrap(), b"original");
    }

    #[test]
    fn leases_exclude_live_sessions_and_competing_recovery_dialogs() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        writer.checkpoint(1, &snapshot("draft")).unwrap();
        let first = Store::open(directory.path()).unwrap();
        let second = Store::open(directory.path()).unwrap();
        assert!(first.list_abandoned().unwrap().candidates.is_empty());
        drop(writer);
        let lease = first.list_abandoned().unwrap();
        assert_eq!(lease.candidates.len(), 1);
        assert!(second.list_abandoned().unwrap().candidates.is_empty());
        drop(lease);
        assert_eq!(second.list_abandoned().unwrap().candidates.len(), 1);
    }

    #[test]
    fn consume_requires_new_durable_matching_checkpoint_and_own_candidate() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        let original = snapshot("precious");
        writer.checkpoint(1, &original).unwrap();
        let old = writer.session.clone();
        drop(writer);
        let mut reader = Store::open(directory.path()).unwrap();
        reader.checkpoint(9, &original).unwrap(); // Too early to prove a restoration.
        let listing = reader.list_abandoned().unwrap();
        let candidate = &listing.candidates[0];
        assert!(reader.consume(candidate).is_err());
        assert!(old.exists());
        reader.checkpoint(10, &snapshot("different")).unwrap();
        assert!(reader.consume(candidate).is_err());
        let mut other = Store::open(directory.path()).unwrap();
        other.checkpoint(1, &original).unwrap();
        assert!(other.consume(candidate).is_err());
        reader.checkpoint(11, &original).unwrap();
        reader.consume(candidate).unwrap();
        assert!(!old.exists());
        drop(listing);
        drop(reader);
        assert!(
            other
                .list_abandoned()
                .unwrap()
                .candidates
                .iter()
                .any(|candidate| candidate.snapshot.source == "precious")
        );
    }

    #[test]
    fn failed_atomic_publish_and_invalid_updates_retain_previous_generation() {
        let directory = state();
        let mut store = Store::open(directory.path()).unwrap();
        store.checkpoint(7, &snapshot("good")).unwrap();
        let old = store.owned[&7].path.clone();
        let blocked = store.session.join(record_name(7, 2));
        fs::create_dir(&blocked).unwrap();
        assert!(store.checkpoint(7, &snapshot("new")).is_err());
        assert_eq!(read_record(&old, MAX_RECORD_BYTES).unwrap().source, "good");
        fs::remove_dir(blocked).unwrap();
        let mut invalid = snapshot("e\u{301}");
        invalid.selection.head = 1;
        assert!(store.checkpoint(7, &invalid).is_err());
        assert!(
            store
                .checkpoint(7, &snapshot(&"x".repeat(MAX_SOURCE_BYTES + 1)))
                .is_err()
        );
        assert_eq!(read_record(&old, MAX_RECORD_BYTES).unwrap().source, "good");
        store.checkpoint(7, &snapshot("new")).unwrap();
        assert!(!old.exists());
        let next = store.owned[&7].path.clone();
        drop(store);
        let listing = Store::open(directory.path())
            .unwrap()
            .list_abandoned()
            .unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(listing.candidates[0].snapshot.source, "new");
        assert!(next.exists());
    }

    #[test]
    fn corrupt_new_generation_falls_back_and_all_invalid_data_stays_on_disk() {
        let directory = state();
        let mut store = Store::open(directory.path()).unwrap();
        store.checkpoint(1, &snapshot("good")).unwrap();
        let corrupt = store.session.join(record_name(1, 2));
        let mut bytes = encode(1, 2, &snapshot("new")).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        write_record(&corrupt, &bytes);
        let truncated = store.session.join(record_name(2, 1));
        write_record(&truncated, b"MLREC001");
        let version = store.session.join(record_name(3, 1));
        let mut bytes = encode(3, 1, &snapshot("later")).unwrap();
        bytes[7] = b'9';
        write_record(&version, &bytes);
        let huge = store.session.join(record_name(4, 1));
        let file = create_private(&huge).unwrap();
        file.set_len(MAX_RECORD_BYTES as u64 + 1).unwrap();
        drop(store);
        let mut reader = Store::open(directory.path()).unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(listing.candidates[0].snapshot.source, "good");
        assert_eq!(listing.diagnostics.len(), 4);
        reader
            .checkpoint(1, &listing.candidates[0].snapshot)
            .unwrap();
        reader.consume(&listing.candidates[0]).unwrap();
        for path in [corrupt, truncated, version, huge] {
            assert!(path.exists());
        }
    }

    #[test]
    fn checksum_correct_invalid_utf8_lengths_identity_and_selection_are_rejected() {
        let source = snapshot("hi");
        let original = encode(1, 1, &source).unwrap();
        let repair = |bytes: &mut Vec<u8>| {
            let crc = crc32(&bytes[20..]);
            bytes[16..20].copy_from_slice(&crc.to_le_bytes());
        };
        let mut utf8 = original.clone();
        *utf8.last_mut().unwrap() = 255;
        repair(&mut utf8);
        assert!(decode(&utf8, 1, 1).is_err());
        let mut size = original.clone();
        size[62..70].copy_from_slice(&u64::MAX.to_le_bytes());
        repair(&mut size);
        assert!(decode(&size, 1, 1).is_err());
        let mut selection = original.clone();
        selection[36..44].copy_from_slice(&999u64.to_le_bytes());
        repair(&mut selection);
        assert!(decode(&selection, 1, 1).is_err());
        assert!(decode(&original, 2, 1).is_err());
        for length in [0, 7, 19, 20, original.len() - 1] {
            assert!(decode(&original[..length], 1, 1).is_err());
        }
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
    }

    #[test]
    fn known_removal_clean_exit_and_drop_have_distinct_scopes() {
        let directory = state();
        let mut first = Store::open(directory.path()).unwrap();
        let mut other = Store::open(directory.path()).unwrap();
        first.checkpoint(1, &snapshot("one")).unwrap();
        other.checkpoint(1, &snapshot("other")).unwrap();
        let foreign = other.owned[&1].path.clone();
        first.remove(999).unwrap();
        assert!(first.owned[&1].path.exists());
        first.remove(1).unwrap();
        assert!(first.owned.is_empty());
        assert!(foreign.exists());
        let path = first.session.clone();
        first.finish_clean().unwrap();
        assert!(!path.exists());
        assert!(foreign.exists());
        assert!(first.checkpoint(2, &snapshot("closed")).is_err());
        drop(other);
        let reader = Store::open(directory.path()).unwrap();
        assert_eq!(reader.list_abandoned().unwrap().candidates.len(), 1);
    }

    #[test]
    fn cleanup_refuses_unknown_or_corrupt_entries_without_partial_removal() {
        let directory = state();
        let mut store = Store::open(directory.path()).unwrap();
        store.checkpoint(1, &snapshot("keep")).unwrap();
        let known = store.owned[&1].path.clone();
        let unknown = store.session.join("unexpected");
        write_record(&unknown, b"unknown");
        assert!(store.finish_clean().is_err());
        assert!(known.exists());
        assert!(unknown.exists());
        fs::remove_file(unknown).unwrap();
        let unknown = store.session.join(record_name(99, 1));
        write_record(&unknown, &encode(99, 1, &snapshot("unowned")).unwrap());
        store.remove(99).unwrap();
        assert!(unknown.exists());
        assert!(store.finish_clean().is_err());
        assert!(known.exists());
        fs::remove_file(unknown).unwrap();
        let corrupt = store.session.join(record_name(1, 2));
        write_record(&corrupt, b"bad");
        assert!(store.remove(1).is_err());
        assert!(known.exists());
        assert!(store.finish_clean().is_err());
        assert!(corrupt.exists());
    }

    #[test]
    fn changed_candidate_is_retained_even_after_a_matching_checkpoint() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        writer.checkpoint(1, &snapshot("old")).unwrap();
        drop(writer);
        let mut reader = Store::open(directory.path()).unwrap();
        let listing = reader.list_abandoned().unwrap();
        let candidate = &listing.candidates[0];
        reader.checkpoint(1, &candidate.snapshot).unwrap();
        let path = &candidate.records[0].0;
        fs::write(path, b"changed").unwrap();
        assert!(reader.consume(candidate).is_err());
        assert_eq!(fs::read(path).unwrap(), b"changed");
    }

    #[test]
    fn discovery_counts_are_bounded_and_overflow_is_reported_without_deleting() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        for id in 0..(MAX_RECORDS as u64 + 2) {
            writer.checkpoint(id, &snapshot("small")).unwrap();
        }
        let session = writer.session.clone();
        drop(writer);
        let reader = Store::open(directory.path()).unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), MAX_RECORDS);
        assert!(!listing.diagnostics.is_empty());
        assert_eq!(session_entries(&session).unwrap().len(), MAX_RECORDS + 3);
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions_non_utf8_paths_and_symlinks_are_handled_conservatively() {
        use std::os::unix::{
            ffi::OsStringExt,
            fs::{MetadataExt, PermissionsExt, symlink},
        };
        let directory = state();
        let root = directory.path().join("state");
        let mut store = Store::open(&root).unwrap();
        let mut snap = snapshot("draft");
        snap.original_path = Some(PathBuf::from(OsString::from_vec(
            b"/tmp/non-utf8-\xff.md".to_vec(),
        )));
        store.checkpoint(1, &snap).unwrap();
        for path in [&root, &store.session] {
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
        }
        for path in [&store.owned[&1].path, &store.session.join(LOCK_NAME)] {
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o600);
        }
        let original = directory.path().join("original");
        fs::write(&original, b"untouched").unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o600)).unwrap();
        let linked = store.session.join(record_name(2, 1));
        symlink(&original, &linked).unwrap();
        let hard = store.session.join(record_name(3, 1));
        fs::hard_link(&original, &hard).unwrap();
        let fifo = store.session.join(record_name(4, 1));
        let fifo_c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        drop(store);
        let reader = Store::open(&root).unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(listing.candidates[0].snapshot, snap);
        assert_eq!(listing.diagnostics.len(), 3);
        assert_eq!(fs::read(&original).unwrap(), b"untouched");
        let alias = directory.path().join("alias");
        symlink(&root, &alias).unwrap();
        assert!(Store::open(alias).is_err());
        let public = directory.path().join("public");
        fs::create_dir(&public).unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Store::open(public).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_and_replaced_session_entries_are_retained() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        writer.checkpoint(1, &snapshot("keep")).unwrap();
        let session = writer.session.clone();
        let record = writer.owned[&1].path.clone();
        drop(writer);
        fs::set_permissions(&record, fs::Permissions::from_mode(0o0)).unwrap();
        let reader = Store::open(directory.path()).unwrap();
        let listing = reader.list_abandoned().unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert!(listing.candidates.is_empty());
            assert_eq!(listing.diagnostics.len(), 1);
        }
        assert!(record.exists());
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        drop(listing);
        let saved = directory.path().join("moved");
        fs::rename(&session, &saved).unwrap();
        symlink(&saved, &session).unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert!(listing.candidates.is_empty());
        assert!(!listing.diagnostics.is_empty());
        assert!(saved.exists());
    }

    #[test]
    fn crash_writer_subprocess() {
        let Some(root) = std::env::var_os("MARKLANE_RECOVERY_TEST_ROOT") else {
            return;
        };
        let mut store = Store::open(root).unwrap();
        store
            .checkpoint(8, &snapshot("survives an abrupt exit"))
            .unwrap();
        fs::write(
            std::env::var_os("MARKLANE_RECOVERY_TEST_READY").unwrap(),
            b"ready",
        )
        .unwrap();
        loop {
            std::thread::park();
        }
    }

    #[test]
    fn process_death_releases_lock_and_preserves_the_last_durable_snapshot() {
        use std::{
            process::{Child, Command, Stdio},
            time::{Duration, Instant},
        };
        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = state();
        let root = directory.path().join("state");
        let ready = directory.path().join("ready");
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "recovery::tests::crash_writer_subprocess",
                    "--nocapture",
                ])
                .env("MARKLANE_RECOVERY_TEST_ROOT", &root)
                .env("MARKLANE_RECOVERY_TEST_READY", &ready)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "child failed to checkpoint");
        let reader = Store::open(&root).unwrap();
        assert!(reader.list_abandoned().unwrap().candidates.is_empty());
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(
            listing.candidates[0].snapshot.source,
            "survives an abrupt exit"
        );
    }
    #[test]
    fn maximum_source_round_trips_and_mutating_a_candidate_cannot_authorize_consumption() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        let large = snapshot(&"x".repeat(MAX_SOURCE_BYTES));
        writer.checkpoint(1, &large).unwrap();
        drop(writer);
        let mut reader = Store::open(directory.path()).unwrap();
        let mut listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(listing.candidates[0].snapshot, large);
        let original = listing.candidates[0].records[0].0.clone();
        listing.candidates[0].snapshot.source = "different".into();
        reader.checkpoint(1, &snapshot("different")).unwrap();
        assert!(reader.consume(&listing.candidates[0]).is_err());
        assert!(original.exists());
    }

    #[test]
    fn superseded_corrupt_generation_is_preserved_and_blocks_destructive_cleanup() {
        let directory = state();
        let mut writer = Store::open(directory.path()).unwrap();
        writer.checkpoint(1, &snapshot("first")).unwrap();
        let old = writer.owned[&1].path.clone();
        fs::write(&old, b"corrupted old snapshot").unwrap();
        writer.checkpoint(1, &snapshot("second")).unwrap();
        assert!(old.exists());
        assert!(writer.remove(1).is_err());
        assert!(writer.owned[&1].path.exists());
        drop(writer);
        let reader = Store::open(directory.path()).unwrap();
        let listing = reader.list_abandoned().unwrap();
        assert_eq!(listing.candidates.len(), 1);
        assert_eq!(listing.candidates[0].snapshot.source, "second");
        assert_eq!(listing.diagnostics.len(), 1);
    }
}
