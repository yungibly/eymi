use crate::{icons::IconSet, theme::Theme};
use std::{ffi::OsString, io, path::PathBuf};

pub const HELP: &str = "Marklane — a source-preserving Markdown editor

Usage: marklane [--theme NAME] [--source] [FILE]
       marklane --snapshot [--snapshot-size WIDTHxHEIGHT] [--theme NAME] [FILE]

Without FILE, opens an untitled Markdown document. Use -- before a filename
that starts with a dash. Files must be UTF-8 text, at most 8 MiB.

  --theme NAME            Choose a built-in theme (name or ID)
  --icons plain|nerd      Optional Nerd Font icons (default: plain)
  --list-themes           List built-in theme IDs and names
  --no-state              Disable preferences and crash recovery
  --source                Open with Markdown source visible
  --snapshot              Print the real renderer without opening a terminal
  --snapshot-size WxH     Snapshot dimensions (default: 80x24; max: 400x160)
  -h, --help              Show this help
  -V, --version           Show the version

F2 / Ctrl+P Commands · F9 Outline · F1 Help
Ctrl+N New · Ctrl+O Open · Ctrl+W Close tab · Ctrl+Q Quit
Ctrl+S Save · F4 Save As · F5 Reload · Ctrl+E / F6 Live/source
F7/F8 or Ctrl+PageUp/PageDown switches tabs.
Ctrl+F Find · Ctrl+R Replace · F3/Shift+F3 Next/previous match
Ctrl+Z Undo · Ctrl+Y Redo · Ctrl+C/X/V Copy/cut/paste

Terminal paste is literal and undoable. Native clipboard uses a reported
internal fallback on SSH or errors. Existing files changed on disk are
protected from overwrite; Save As requires a new filename.
";

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub path: Option<PathBuf>,
    pub snapshot: bool,
    pub source: bool,
    pub theme: Option<Theme>,
    pub icons: Option<IconSet>,
    pub no_state: bool,
    pub size: (u16, u16),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Edit(Options),
    Help,
    Version,
    ListThemes,
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> io::Result<Action> {
    let mut options = Options {
        path: None,
        snapshot: false,
        source: false,
        theme: None,
        icons: None,
        no_state: false,
        size: (80, 24),
    };
    let mut positional = false;
    let mut explicit_size = false;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if !positional {
            match argument.to_str() {
                Some("--help" | "-h") => return Ok(Action::Help),
                Some("--version" | "-V") => return Ok(Action::Version),
                Some("--list-themes") => return Ok(Action::ListThemes),
                Some("--no-state") => {
                    options.no_state = true;
                    continue;
                }
                Some("--snapshot") => {
                    options.snapshot = true;
                    continue;
                }
                Some("--source") => {
                    options.source = true;
                    continue;
                }
                Some("--") => {
                    positional = true;
                    continue;
                }
                Some(value)
                    if value == "--theme"
                        || value == "--icons"
                        || value.starts_with("--icons=")
                        || value == "--snapshot-size"
                        || value.starts_with("--theme=")
                        || value.starts_with("--snapshot-size=") =>
                {
                    let (flag, value) = match value.split_once('=') {
                        Some((flag, value)) => (flag, OsString::from(value)),
                        None => (
                            value,
                            arguments
                                .next()
                                .ok_or_else(|| invalid(format!("{value} needs a value")))?,
                        ),
                    };
                    let value = value
                        .to_str()
                        .ok_or_else(|| invalid("Invalid option value"))?;
                    if flag == "--theme" {
                        options.theme = Some(Theme::from_name(value).ok_or_else(|| {
                            invalid(format!("Unknown theme {value:?}; use --list-themes"))
                        })?);
                    } else if flag == "--icons" {
                        options.icons = Some(
                            IconSet::from_name(value)
                                .ok_or_else(|| invalid("--icons must be plain or nerd"))?,
                        );
                    } else {
                        options.size = parse_size(value)?;
                        explicit_size = true;
                    }
                    continue;
                }
                Some(value) if value.starts_with('-') => {
                    return Err(invalid(format!("Unknown option {value}; use --help")));
                }
                _ => {}
            }
        }
        if options.path.replace(PathBuf::from(argument)).is_some() {
            return Err(invalid(
                "Open one file on the command line; Ctrl+O opens more tabs",
            ));
        }
    }
    if explicit_size && !options.snapshot {
        return Err(invalid("--snapshot-size requires --snapshot"));
    }
    Ok(Action::Edit(options))
}

fn parse_size(value: &str) -> io::Result<(u16, u16)> {
    let size = value.split_once(['x', 'X']).and_then(|(width, height)| {
        Some((width.parse::<u16>().ok()?, height.parse::<u16>().ok()?))
    });
    match size {
        Some((width @ 1..=400, height @ 1..=160)) => Ok((width, height)),
        _ => Err(invalid(
            "--snapshot-size must be WIDTHxHEIGHT, from 1x1 to 400x160",
        )),
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> io::Result<Action> {
        parse(values.iter().map(OsString::from))
    }

    #[test]
    fn defaults_and_explicit_snapshot_settings() {
        assert_eq!(
            args(&[]).unwrap(),
            Action::Edit(Options {
                path: None,
                snapshot: false,
                source: false,
                theme: None,
                icons: None,
                no_state: false,
                size: (80, 24),
            })
        );
        assert_eq!(
            args(&[
                "--snapshot",
                "--source",
                "--theme=light",
                "--icons=nerd",
                "--snapshot-size",
                "160x45",
                "notes.md"
            ])
            .unwrap(),
            Action::Edit(Options {
                path: Some(PathBuf::from("notes.md")),
                snapshot: true,
                source: true,
                theme: Some(Theme::Light),
                icons: Some(IconSet::Nerd),
                no_state: false,
                size: (160, 45),
            })
        );
    }

    #[test]
    fn invalid_flags_dimensions_and_values_fail_before_opening_a_file() {
        for values in [
            vec!["--theme"],
            vec!["--theme", "no-such-theme-987654321"],
            vec!["--theme="],
            vec!["--icons"],
            vec!["--icons", "unknown"],
            vec!["--icons="],
            vec!["--snapshot-size", "120x36"],
            vec!["--unknown"],
            vec!["a.md", "b.md"],
        ] {
            assert_eq!(
                args(&values).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        for size in [
            "0x24",
            "80x0",
            "401x24",
            "80x161",
            "999999x24",
            "80",
            "80x24x2",
            "-2x24",
        ] {
            assert!(
                args(&["--snapshot", "--snapshot-size", size]).is_err(),
                "{size}"
            );
        }
        assert!(args(&["--snapshot-size=400X160", "--snapshot"]).is_ok());
    }

    #[test]
    fn separator_preserves_option_like_filename() {
        let Action::Edit(options) = args(&["--", "--theme=light"]).unwrap() else {
            panic!()
        };
        assert_eq!(options.path, Some(PathBuf::from("--theme=light")));
        assert_eq!(options.theme, None);
    }

    #[cfg(unix)]
    #[test]
    fn filenames_do_not_need_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"notes-\xff.md".to_vec());
        let Action::Edit(options) = parse([path.clone()]).unwrap() else {
            panic!()
        };
        assert_eq!(options.path, Some(PathBuf::from(path)));
    }
}
