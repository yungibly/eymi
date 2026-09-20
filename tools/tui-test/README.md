# Local terminal test runner

Use the official standalone `tui-test` binary for the first visual/protocol trial. It exercises the compiled application without adding dependencies to Marklane or requiring a Homebrew installation.

Pinned release: [`0.1.0-beta.5`](https://github.com/microsoft/tui-test/releases/tag/0.1.0-beta.5), reviewed source commit `6a991eea96d499689a875b7ee5781aa0c18b88d2`.

From any working directory:

```sh
sh /path/to/md-term-editor/tools/tui-test/bootstrap.sh
```

The script prints the installed binary's absolute path. Downloads and binaries live under the ignored `target/tools/tui-test/` directory. The macOS arm64 and x86_64 release checksums are pinned in the script; every run verifies its cached or freshly downloaded archive before extracting it. It does not modify PATH or install a daemon/service. Session commands can start a runner-owned daemon later; each test must close its own session.

On the current Apple Silicon development host:

```sh
./target/tools/tui-test/0.1.0-beta.5/aarch64-apple-darwin/tui-test --version
```

Bootstrap, SHA-256 verification, `--version`, and `--help` were checked on macOS arm64. Intel and other operating systems have not been exercised; the bootstrap currently accepts only macOS. Deleting Cargo's target directory also removes this installation; rerun the bootstrap to restore it.

Tool installation is complete. Adoption still depends on the [visual/protocol trial](../../docs/visual-testing-plan.md#smallest-useful-spike), particularly backend input bytes, mode-aware paste, pointer-positioned wheel events, and image fidelity. Pin the executable path in test scripts so a global tool upgrade cannot silently change test behavior.
