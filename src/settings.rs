//! Small, versioned preferences. Loading is read-only; explicit choices save atomically.
use crate::{icons::IconSet, theme::Theme};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: u64 = 16 * 1024;

pub struct Settings {
    path: PathBuf,
    baseline: Option<Vec<u8>>,
    values: BTreeMap<String, String>,
}

pub fn directories() -> io::Result<(PathBuf, PathBuf)> {
    fn base(variable: &str, fallback: &str) -> io::Result<PathBuf> {
        if let Some(value) = env::var_os(variable).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Ok(path);
            }
            return Err(io::Error::other(format!(
                "{variable} must be an absolute path"
            )));
        }
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join(fallback))
            .ok_or_else(|| {
                io::Error::other("No home directory; preferences and recovery are unavailable")
            })
    }
    directories_under(
        &base("XDG_CONFIG_HOME", ".config")?,
        &base("XDG_STATE_HOME", ".local/state")?,
    )
}

fn directories_under(config: &Path, state: &Path) -> io::Result<(PathBuf, PathBuf)> {
    Ok((app_directory(config)?, app_directory(state)?))
}

/// Select existing roots without creating, migrating, or deleting anything.
/// A new-name root wins even when empty. Only a genuinely absent path permits
/// fallback; errors and non-directory entries must not silently split user data.
fn app_directory(base: &Path) -> io::Result<PathBuf> {
    for name in ["eymi", "marklane"] {
        let path = base.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => return Ok(path),
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "Preferences and recovery path must be a real directory: {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(base.join("eymi"))
}

fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::other("Settings must be a regular file"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Settings must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("Settings exceed the 16 KiB limit"));
    }
    Ok(Some(bytes))
}

impl Settings {
    pub fn load(directory: &Path) -> io::Result<Self> {
        let path = directory.join("settings.conf");
        let baseline = read(&path)?;
        let text = std::str::from_utf8(baseline.as_deref().unwrap_or_default())
            .map_err(|_| io::Error::other("Settings are not UTF-8"))?;
        let mut values = BTreeMap::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| io::Error::other("Invalid settings line; expected key=value"))?;
            let (key, value) = (key.trim(), value.trim());
            if key.is_empty()
                || key.chars().chain(value.chars()).any(char::is_control)
                || values.insert(key.to_owned(), value.to_owned()).is_some()
            {
                return Err(io::Error::other("Invalid or duplicate settings key"));
            }
        }
        if values.get("version").is_some_and(|value| value != "1") {
            return Err(io::Error::other(
                "Unsupported settings version; preferences were left untouched",
            ));
        }
        Ok(Self {
            path,
            baseline,
            values,
        })
    }
    pub fn theme(&self) -> Option<Theme> {
        self.values
            .get("theme")
            .and_then(|value| Theme::from_name(value))
    }
    pub fn unknown_theme(&self) -> Option<&str> {
        self.values
            .get("theme")
            .filter(|_| self.theme().is_none())
            .map(String::as_str)
    }
    pub fn sidebar(&self) -> Option<bool> {
        match self.values.get("sidebar").map(String::as_str) {
            Some("show") => Some(true),
            Some("hide") => Some(false),
            _ => None,
        }
    }
    pub fn icons(&self) -> Option<IconSet> {
        self.values
            .get("icons")
            .and_then(|value| IconSet::from_name(value))
    }
    pub fn save_icons(&mut self, icons: IconSet) -> io::Result<()> {
        self.save("icons", icons.name())
    }
    pub fn save_theme(&mut self, theme: Theme) -> io::Result<()> {
        self.save("theme", theme.id())
    }
    pub fn save_sidebar(&mut self, preference: Option<bool>) -> io::Result<()> {
        self.save(
            "sidebar",
            match preference {
                Some(true) => "show",
                Some(false) => "hide",
                None => "auto",
            },
        )
    }
    fn save(&mut self, key: &str, value: &str) -> io::Result<()> {
        let mut values = self.values.clone();
        values.insert("version".into(), "1".into());
        values.insert(key.into(), value.into());
        let text: String = values
            .iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        if text.len() as u64 > LIMIT {
            return Err(io::Error::other(
                "Preferences would exceed the 16 KiB settings limit",
            ));
        }
        let directory = self.path.parent().expect("settings parent");
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory)?;
        // The stable sidecar survives atomic replacement of the preferences inode.
        let _lock = lock(directory)?;
        let permissions = writable_permissions(&self.path)?;
        if read(&self.path)? != self.baseline {
            return Err(io::Error::other(
                "Settings changed in another process; restart before saving preferences",
            ));
        }
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        if let Some(permissions) = permissions {
            file.as_file().set_permissions(permissions)?;
        }
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        if read(&self.path)? != self.baseline {
            return Err(io::Error::other(
                "Settings changed while saving; preferences were left untouched",
            ));
        }
        writable_permissions(&self.path)?;
        if self.baseline.is_none() {
            file.persist_noclobber(&self.path)
                .map_err(|error| error.error)?;
        } else {
            file.persist(&self.path).map_err(|error| error.error)?;
        }
        self.values = values;
        self.baseline = Some(text.into_bytes());
        #[cfg(unix)]
        fs::File::open(directory)?.sync_all()?;
        Ok(())
    }
}

fn lock(directory: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(directory.join("settings.lock"))?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Settings lock must be a regular file"));
    }
    file.try_lock().map_err(|error| {
        io::Error::other(format!(
            "Preferences are being changed by another process: {error}"
        ))
    })?;
    Ok(file)
}
fn writable_permissions(path: &Path) -> io::Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.permissions().readonly() => {
            Ok(Some(metadata.permissions()))
        }
        Ok(_) => Err(io::Error::other(
            "Settings are read-only or are not a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_roots_select_eymi_without_creating_directories() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        let state = directory.path().join("state");
        assert_eq!(
            directories_under(&config, &state).unwrap(),
            (config.join("eymi"), state.join("eymi"))
        );
        assert!(!config.exists());
        assert!(!state.exists());
    }

    #[test]
    fn legacy_roots_are_selected_independently_without_migration() {
        for config_new in [false, true] {
            for state_new in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let config = directory.path().join("config");
                let state = directory.path().join("state");
                fs::create_dir_all(config.join("marklane")).unwrap();
                fs::create_dir_all(state.join("marklane/recovery")).unwrap();
                let preferences = config.join("marklane/settings.conf");
                let snapshot = state.join("marklane/recovery/retained.snapshot");
                fs::write(&preferences, "version=1\ntheme=light\nicons=nerd\n").unwrap();
                fs::write(&snapshot, b"legacy snapshot remains untouched").unwrap();
                if config_new {
                    fs::create_dir(config.join("eymi")).unwrap();
                }
                if state_new {
                    fs::create_dir(state.join("eymi")).unwrap();
                }
                let (chosen_config, chosen_state) = directories_under(&config, &state).unwrap();
                assert_eq!(
                    chosen_config,
                    config.join(if config_new { "eymi" } else { "marklane" })
                );
                assert_eq!(
                    chosen_state,
                    state.join(if state_new { "eymi" } else { "marklane" })
                );
                let settings = Settings::load(&chosen_config).unwrap();
                assert_eq!(
                    settings.theme(),
                    if config_new { None } else { Some(Theme::Light) }
                );
                assert_eq!(
                    settings.icons(),
                    if config_new {
                        None
                    } else {
                        Some(IconSet::Nerd)
                    }
                );
                assert_eq!(
                    fs::read_to_string(&preferences).unwrap(),
                    "version=1\ntheme=light\nicons=nerd\n"
                );
                assert_eq!(
                    fs::read(&snapshot).unwrap(),
                    b"legacy snapshot remains untouched"
                );
                assert_eq!(config.join("eymi").exists(), config_new);
                assert_eq!(state.join("eymi").exists(), state_new);
                assert!(!config.join("eymi/settings.conf").exists());
            }
        }
    }

    #[test]
    fn conflicting_root_files_and_lookup_errors_do_not_trigger_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path();
        fs::create_dir(base.join("marklane")).unwrap();
        fs::write(base.join("eymi"), "keep file").unwrap();
        assert!(app_directory(base).is_err());
        assert_eq!(fs::read_to_string(base.join("eymi")).unwrap(), "keep file");
        assert!(app_directory(&base.join("eymi/child")).is_err());
        fs::remove_file(base.join("eymi")).unwrap();
        fs::remove_dir(base.join("marklane")).unwrap();
        fs::write(base.join("marklane"), "keep legacy file").unwrap();
        assert!(app_directory(base).is_err());
        assert!(!base.join("eymi").exists());
        assert_eq!(
            fs::read_to_string(base.join("marklane")).unwrap(),
            "keep legacy file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_symlinks_are_not_followed_or_treated_as_absent() {
        use std::os::unix::fs::symlink;
        for name in ["eymi", "marklane"] {
            for dangling in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let base = directory.path().join("base");
                let target = directory.path().join("target");
                fs::create_dir(&base).unwrap();
                if !dangling {
                    fs::create_dir(&target).unwrap();
                }
                symlink(&target, base.join(name)).unwrap();
                if name == "eymi" {
                    fs::create_dir(base.join("marklane")).unwrap();
                }
                assert!(app_directory(&base).is_err());
                assert!(
                    fs::symlink_metadata(base.join(name))
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
                assert_eq!(target.exists(), !dangling);
                if !dangling {
                    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
                }
            }
        }
    }

    #[test]
    fn explicit_choices_persist_without_touching_unknown_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.conf");
        fs::write(&path, "version=1\nfuture=keep\n").unwrap();
        let mut settings = Settings::load(dir.path()).unwrap();
        settings.save_theme(Theme::Light).unwrap();
        settings.save_sidebar(Some(false)).unwrap();
        settings.save_icons(IconSet::Nerd).unwrap();
        let loaded = Settings::load(dir.path()).unwrap();
        assert_eq!(loaded.theme(), Some(Theme::Light));
        assert_eq!(loaded.sidebar(), Some(false));
        assert_eq!(loaded.icons(), Some(IconSet::Nerd));
        assert!(fs::read_to_string(path).unwrap().contains("future=keep"));
    }
    #[test]
    fn read_is_lazy_and_concurrent_changes_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested");
        let mut settings = Settings::load(&path).unwrap();
        assert!(!path.exists());
        settings.save_theme(Theme::Dark).unwrap();
        fs::write(path.join("settings.conf"), "theme=light\n").unwrap();
        assert!(settings.save_theme(Theme::Dark).is_err());
        assert_eq!(
            fs::read_to_string(path.join("settings.conf")).unwrap(),
            "theme=light\n"
        );
    }
    #[test]
    fn oversized_updates_and_concurrent_writers_leave_preferences_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.conf");
        let large = format!("future={}\n", "x".repeat(LIMIT as usize - 8));
        fs::write(&path, &large).unwrap();
        let mut settings = Settings::load(dir.path()).unwrap();
        assert!(settings.save_theme(Theme::Dark).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), large);
        fs::write(&path, "theme=dark\n").unwrap();
        let mut settings = Settings::load(dir.path()).unwrap();
        let _locked = lock(dir.path()).unwrap();
        assert!(settings.save_theme(Theme::Light).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "theme=dark\n");
    }
    #[cfg(unix)]
    #[test]
    fn explicitly_readonly_preferences_remain_readonly() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.conf");
        fs::write(&path, "theme=dark\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let mut settings = Settings::load(dir.path()).unwrap();
        assert!(settings.save_theme(Theme::Light).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theme=dark\n");
        assert!(fs::metadata(path).unwrap().permissions().readonly());
    }
    #[test]
    fn invalid_and_future_settings_are_retained() {
        let dir = tempfile::tempdir().unwrap();
        for bytes in [
            b"version=2\n".as_slice(),
            b"version=1\nversion=1\n",
            b"bad",
            b"theme=\xff",
            b"theme=dark\0",
        ] {
            fs::write(dir.path().join("settings.conf"), bytes).unwrap();
            assert!(Settings::load(dir.path()).is_err());
            assert_eq!(fs::read(dir.path().join("settings.conf")).unwrap(), bytes);
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlink_preferences_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("other");
        fs::write(&target, "theme=light\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("settings.conf")).unwrap();
        assert!(Settings::load(dir.path()).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "theme=light\n");
    }
}
