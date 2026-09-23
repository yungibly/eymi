# Releasing Eymi

The product and executable are **Eymi** / **`eymi`**. The source repository is
[`yungibly/eymi`](https://github.com/yungibly/eymi), and the binary formula lives
in [`yungibly/homebrew-tap`](https://github.com/yungibly/homebrew-tap).
The source uses the MIT license. Theme and dependency licenses accompany every
binary archive and are installed by the formula under Eymi's shared data directory.

`v0.2.0` is [published](https://github.com/yungibly/eymi/releases/tag/v0.2.0).
Its [CI](https://github.com/yungibly/eymi/actions/runs/35808147599),
[native release builds](https://github.com/yungibly/eymi/actions/runs/35808271377),
and [three-platform Homebrew installation checks](https://github.com/yungibly/eymi/actions/runs/35808607212)
all passed before the tap update. The first release,
[`v0.1.0`](https://github.com/yungibly/eymi/releases/tag/v0.1.0), passed the
same checks.

## CI and supported packages

Pull requests and branch pushes run locked Cargo tests on Linux and macOS with
Rust stable and the minimum toolchain, 1.89.0. Stable also runs formatting,
strict all-target Clippy, theme catalog/attribution checks, and Python release
tool regressions. Real-process PTY tests cover terminal cleanup, editing,
saving, and crash recovery without reading the machine's native clipboard.

Release builds use Rust 1.98.0 and three native runners:

| Runner | Archive target | Compatibility |
| --- | --- | --- |
| `macos-15` | `aarch64-apple-darwin` | Apple Silicon; macOS deployment target 11.0 |
| `macos-15-intel` | `x86_64-apple-darwin` | Intel; macOS deployment target 11.0 |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | Linux x86_64; glibc 2.35 build baseline |

The runner tests establish behavior on those operating systems, not every
older release allowed by the binary's deployment target. The build logs retain
`file`, `otool`/`vtool`, or `ldd`/`readelf` output for the packaged executable.
Native Wayland clipboard, OSC 52, IME, and all terminal/font combinations are
outside this compatibility claim. Linux ARM and Windows have no release asset.

## Prepare and review a release

1. Update the package version and lockfile together. Review the changes, then
   require green CI on the release commit.
2. Create and push a matching tag, for example `v0.1.0`. The **Release archives**
   workflow checks the tag against Cargo metadata and builds that exact commit.
   Manual dispatch can rebuild an existing tag for validation; its default
   does not create a draft.
3. Every native build repeats tests and lint, generates dependency notices from
   the locked target graph, creates its archive, and smoke-tests the extracted
   binary's version, help, themes, and source-preserving headless snapshot.
4. Assembly requires all three archives, verifies their contents and notice
   manifests, and produces `SHA256SUMS`. Only the final draft job has repository
   write permission. It creates a draft and refuses to replace an existing release.
5. Download and verify the draft's assets, review its release notes, and publish
   it with a maintainer account. Publishing with the workflow's `GITHUB_TOKEN`
   does not trigger a second release workflow; use the maintainer account or
   dispatch **Homebrew** explicitly afterward.

Each archive is named `eymi-{version}-{target}.tar.gz`, without the tag's `v`.
Its root contains `eymi`, `README.md`, `LICENSE`, the full
`third_party/iterm2-themes/` provenance tree, and `third_party/rust/` with the
dependency notice manifest and texts. Packaging normalizes tar/gzip metadata
and refuses to overwrite existing output. This does not claim byte-identical
Rust builds across different hosts.

Useful local checks, with a native release binary already built:

```sh
python3 -m unittest discover -s tools/release -p 'test_*.py'
python3 tools/release/package.py validate --tag v0.1.0
python3 tools/release/notices.py --target aarch64-apple-darwin --output target/rust-notices
python3 tools/release/package.py pack --tag v0.1.0 --target aarch64-apple-darwin \
  --binary target/release/eymi --notices target/rust-notices \
  --output-dir target/packages --epoch 0
```

Use fresh output directories. Production archives take their timestamp from
the tagged commit. `package.py checksums` and `package.py verify` require all
three final archives; they are the final assembly checks.

## Homebrew publication

The **Homebrew** workflow runs when a stable release is published. Its manual
dispatch accepts an already published stable tag and is suitable for retrying
a failed tap update.

First it downloads the three archives and `SHA256SUMS` anonymously from the
fixed Eymi repository. It validates the exact asset names and archive bytes,
then generates `Formula/eymi.rb`. Separate native Homebrew jobs install the
public archives and run the formula's version/theme/snapshot test. Only after
all three pass does the updater receive `BREW_TAP_TOKEN`.

The updater changes only `Formula/eymi.rb` on the tap's `main` branch. GitHub's
Contents API performs a bounded compare-and-swap retry for concurrent changes;
it does not replace tap history. Repeating the same formula is a no-op. An
older release cannot roll back a newer formula, and changed contents for an
already released version are rejected.

Configure `BREW_TAP_TOKEN` as an Actions secret in the Eymi repository, with
contents-write access to `yungibly/homebrew-tap`. The root `.env` is a local,
ignored setup input; workflows never read or upload it. PR jobs receive no tap
credential. Do not put the token in commands, logs, formulae, or archives.

If publication succeeds but a Homebrew test or tap update fails, leave the
existing tap formula intact, fix the failure, and rerun the Homebrew workflow.
Do not replace a public release archive to make its checksum fit an old formula.

The platform DSL follows the [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook#handling-different-system-configurations).
GitHub documents [release events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#release)
and [workflow-token event behavior](https://docs.github.com/en/actions/how-tos/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#triggering-a-workflow-from-a-workflow).
