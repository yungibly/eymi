use std::{fs, process::Command};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_marklane"))
}

#[test]
fn snapshots_use_requested_dimensions_and_preserve_source() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.md");
    let source = "\u{feff}# A heading\r\n\r\nSome **strong** source.\r\n";
    fs::write(&path, source).unwrap();
    for (size, height) in [
        ("1x1", 1),
        ("20x6", 6),
        ("80x24", 24),
        ("120x36", 36),
        ("160x45", 45),
    ] {
        for theme in ["dark", "light"] {
            let output = binary()
                .args(["--snapshot", "--snapshot-size", size, "--theme", theme])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{size} {theme}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert_eq!(text.lines().count(), height + 2);
            assert!(text.contains("Caret source byte:"));
            assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
        }
    }
}

#[test]
fn command_line_open_rejects_binary_and_oversized_files() {
    let directory = tempfile::tempdir().unwrap();
    let binary_path = directory.path().join("binary.md");
    fs::write(&binary_path, b"before\0after").unwrap();
    let output = binary()
        .arg("--snapshot")
        .arg(&binary_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("NUL"));
    assert_eq!(fs::read(binary_path).unwrap(), b"before\0after");

    let large = directory.path().join("large.md");
    fs::File::create(&large)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    let output = binary().arg("--snapshot").arg(&large).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("open limit"));
    assert_eq!(fs::metadata(large).unwrap().len(), 8 * 1024 * 1024 + 1);
}

#[test]
fn invalid_snapshot_options_fail_without_creating_the_target() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing.md");
    let output = binary()
        .args(["--snapshot", "--snapshot-size", "999x999"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("WIDTHxHEIGHT"));
    assert!(!path.exists());
}

#[test]
fn builtin_theme_catalog_and_named_snapshots_are_state_free() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    let state = directory.path().join("state");
    let output = binary()
        .arg("--list-themes")
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_STATE_HOME", &state)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.lines().count() > 600);
    for name in [
        "Catppuccin Mocha",
        "Dracula",
        "Nord",
        "Gruvbox Dark",
        "TokyoNight",
    ] {
        assert!(text.contains(name), "{name}");
    }
    for name in ["Catppuccin Mocha", "nord", "one-dark", "solarized-light"] {
        let output = binary()
            .args(["--snapshot", "--theme", name])
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_STATE_HOME", &state)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!config.exists());
    assert!(!state.exists());
}
