# Embedded terminal palettes

Marklane embeds **622 imported palettes** from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes), alongside its two original Sage palettes. This is the collection [Ghostty documents as its built-in theme source](https://ghostty.org/docs/features/theme). The import uses the collection's Ghostty color files only.

- Pinned revision: `12d9f63060857aaf673daae635a4c721d12eb586`.
- Source digest: `b1ac9e9ca0324314d3793ad990fd90467b90f50158022751a40caa52842624a4`.
- Color-only `palettes.json` SHA-256: `0e285505d96db98d7744d6de00ce35cf90b873f9d1b61b1de479135a17d9f4de`.
- Generated code: `src/theme_data.rs`; importer: `tools/import-themes.py`.

The source digest covers `LICENSE`, `CREDITS.md`, and all 630 original `ghostty/` files, sorted by repository-relative path. For each entry it hashes UTF-8 path bytes, NUL, the eight-byte big-endian content length, then the exact file bytes. Each included palette also records its original file SHA-256. Excluded palette colors are not bundled.

## Attribution and exceptions

`LICENSE` and `CREDITS.md` are exact copies from the pinned collection. Its MIT license applies to the collection and expressly leaves individual-theme copyrights/licenses with their authors. Retain both files when distributing this data. Original theme names and author links are preserved in the credits; inclusion does not imply author endorsement.

The `licenses/` directory retains additional exact notices checked for Catppuccin, Dracula, Nord, Gruvbox Material, Tokyo Night, Atom One Dark, Solarized, Smyck, Rebecca, and Spacedust. Gruvbox declares MIT/X11 in its README; that original README is also retained. `additional-licenses.json` records source links, original Git blob IDs, and SHA-256 values for these files. Smyck's API license label is `NOASSERTION`, but the retained text is an MIT grant with its author notice.

Tokyo Night uses Apache-2.0. Its exact license is retained; no separate NOTICE file was present in upstream tree `cdc07ac78467a233fd62c493de29a17e0cf2b2b6`. Marklane transforms color data into RGB constants and derives additional editor roles with contrast adjustments; these are Marklane adaptations, not an unchanged Tokyo Night editor theme. No upstream theme code or scripts are imported.

The Modus Emacs package is GPL-3.0, but its author explicitly publishes the **color palettes separately under CC0** on [Colours of the Modus themes](https://protesilaos.com/emacs/modus-themes-colors). These color-only entries are retained on that basis; attribution remains with Protesilaos Stavrou. Its Emacs implementation is not included.

Eight entries credited to the official Monokai Pro downloads are excluded: **Monokai Classic**, **Monokai Pro**, **Monokai Pro Light**, **Monokai Pro Light Sun**, **Monokai Pro Machine**, **Monokai Pro Octagon**, **Monokai Pro Ristretto**, and **Monokai Pro Spectrum**. The [author's licensing terms](https://monokai.pro/license) prohibit redistribution without written consent. Other separately credited Monokai-inspired entries remain. This review preserves the collection's stated rights and attribution plus the specifically checked notices above; it does not claim a separate licensing audit of every original author repository. Review these exceptions when changing the source revision. Reviewed 2026-09-22.

## Reproduce without network access

From the repository root:

```sh
python3 tools/import-themes.py --check --self-test
```

Without `--check`, this regenerates the Rust table from the checked-in color-only JSON. It needs only Python's standard library. Neither building nor running Marklane calls the importer or accesses the network.

To reproduce the JSON, generated table, LICENSE, and CREDITS from a local copy of the pinned upstream revision:

```sh
python3 tools/import-themes.py --upstream /path/to/iTerm2-Color-Schemes --check --self-test
```

Omit `--check` to regenerate those files. The importer verifies the pinned source and bundled-input digests before writing. It accepts only the explicit color keys, exactly sixteen ANSI slots, and six-digit RGB values; unknown settings, scripts, duplicate indices, and incomplete palettes are rejected. Updating the pin is a deliberate source/data/license review, followed by changing the recorded digests and exclusions; it is not an automatic update at build time.

## Adaptation and stable identifiers

Original backgrounds remain unchanged. Text colors remain unchanged when they meet the editor's 4.5:1 contrast floor; otherwise they are minimally blended toward readable neutral ink. Muted text, chrome, code, headings, links, warnings, and both search states derive from that palette and are checked for contrast. Source selections reverse readable foreground/background pairs. Decorative border colors are not text contrast targets. Imported palettes are derived once and cached; per-glyph painting does no color conversion.

Names remain upstream names; IDs are lowercase hyphenated names, with `+` spelled `plus`. Lookup ignores ASCII case, spaces, hyphens, and underscores, while punctuation remains significant so `Dracula` and `Dracula+` are distinct. `dark`/`light` select the original Sage palettes. Familiar `one-dark`, `one-light`, `solarized-dark`, and `solarized-light` aliases resolve to the appropriate imported names. Persist a theme's ID, never its numeric catalog index.
