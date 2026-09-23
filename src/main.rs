mod app;
mod browser;
mod cli;
mod clipboard;
mod file_io;
mod icons;
mod projection;
mod recovery;
mod search;
mod search_highlight;
mod settings;
mod simulation;
#[allow(dead_code)]
mod syntax;
mod terminal;
mod theme;
mod workspace;

use std::io::{self, IsTerminal};

fn main() {
    if let Err(error) = run() {
        eprintln!("eymi: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let options = match cli::parse(std::env::args_os().skip(1))? {
        cli::Action::Help => {
            print!("{}", cli::HELP);
            return Ok(());
        }
        cli::Action::Version => {
            println!("eymi {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        cli::Action::ListThemes => {
            for theme in theme::Theme::all() {
                println!("{}\t{}", theme.id(), theme.name());
            }
            return Ok(());
        }
        cli::Action::Edit(options) => options,
    };
    theme::set_theme(options.theme.unwrap_or_default());
    icons::set(options.icons.unwrap_or_default());
    let mut app = workspace::Workspace::open(options.path)?;
    if options.source {
        app.editor_mut().live = false;
    }
    if options.snapshot {
        print!(
            "{}",
            simulation::snapshot(&mut app, options.size.0, options.size.1)?
        );
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "Interactive mode needs a terminal; use --snapshot for a headless render",
        ));
    }
    app.enable_system_clipboard();
    if !options.no_state {
        match settings::directories() {
            Ok((config, state)) => {
                app.enable_state(&config, &state, options.theme.is_some());
                if let Some(icons) = options.icons {
                    icons::set(icons);
                }
            }
            Err(error) => app.editor_mut().set_message(error.to_string()),
        }
    }
    terminal::run(&mut app)
}
