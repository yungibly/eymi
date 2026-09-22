mod app;
mod browser;
mod clipboard;
mod file_io;
mod projection;
mod search;
mod search_highlight;
mod simulation;
mod terminal;
mod theme;
mod workspace;

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("marklane: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut path = None;
    let mut snapshot = false;
    let mut source = false;
    let mut positional = false;
    for argument in std::env::args_os().skip(1) {
        if !positional {
            match argument.to_str() {
                Some("--help" | "-h") => {
                    println!(
                        "Marklane — terminal Markdown editor prototype\n\nUsage: marklane [--source] [FILE]\n       marklane --snapshot [--source] [FILE]\n\nWithout FILE, opens an untitled Markdown buffer.\n--snapshot prints the real 80×24 render without accessing the system clipboard.\n\nCtrl+N New · Ctrl+O Open browser · Ctrl+W Close tab · Ctrl+Q Quit all\nF7/F8 or Ctrl+PageUp/PageDown switches tabs; the tab strip is clickable.\nCtrl+S Save · F4 Save As · Ctrl+E/F6 View · F1 Help\nCtrl+F Find · Ctrl+R Replace · F3/Shift+F3 Next/Previous match\nSearch: Tab switches fields; With Enter replaces one; Alt+R/A replaces one/all.\nCtrl+C/X/V uses the system clipboard locally; SSH/errors use internal fallback.\nTerminal paste is supported. Native Linux clipboard needs X11/XWayland.\nBrowser: Tab edits path; F2 shows hidden files; F5 shows all files.\nBrowser opens UTF-8 text up to 8 MiB; larger files are rejected.\nExisting files changed on disk are protected from overwrite."
                    );
                    return Ok(());
                }
                Some("--version" | "-V") => {
                    println!("marklane {}", env!("CARGO_PKG_VERSION"));
                    return Ok(());
                }
                Some("--snapshot") => {
                    snapshot = true;
                    continue;
                }
                Some("--source") => {
                    source = true;
                    continue;
                }
                Some("--") => {
                    positional = true;
                    continue;
                }
                Some(value) if value.starts_with('-') => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("Unknown option {value}; use --help"),
                    ));
                }
                _ => {}
            }
        }
        if path.replace(PathBuf::from(argument)).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Open one file at a time in this prototype",
            ));
        }
    }
    let mut app = workspace::Workspace::open(path)?;
    if source {
        app.editor_mut().live = false;
    }
    if snapshot {
        print!("{}", simulation::snapshot(&mut app, 80, 24)?);
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "Interactive mode needs a terminal; use --snapshot for a headless render",
        ));
    }
    app.enable_system_clipboard();
    terminal::run(&mut app)
}
