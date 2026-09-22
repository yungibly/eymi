#!/usr/bin/env python3
"""Rebuild the embedded theme catalog offline; never execute theme settings.

Default input: checked-in color-only JSON. --upstream additionally verifies the
pinned Ghostty source tree and reproduces that JSON and its attribution files.
No network access is performed. --check reports drift without writing files.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "third_party/iterm2-themes"
REVISION = "12d9f63060857aaf673daae635a4c721d12eb586"
SOURCE_SHA256 = "b1ac9e9ca0324314d3793ad990fd90467b90f50158022751a40caa52842624a4"
CATALOG_SHA256 = "0e285505d96db98d7744d6de00ce35cf90b873f9d1b61b1de479135a17d9f4de"
EXCLUDED = (
    "Monokai Classic", "Monokai Pro", "Monokai Pro Light", "Monokai Pro Light Sun",
    "Monokai Pro Machine", "Monokai Pro Octagon", "Monokai Pro Ristretto", "Monokai Pro Spectrum",
)
COUNT = 622
HEX = re.compile(r"#[0-9a-fA-F]{6}\Z")


def normalized(name):
    return re.sub(r"[\s_-]", "", name).lower()


def identifier(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower().replace("+", " plus ")).strip("-")


def parse_palette(raw):
    colors, ansi = {}, {}
    keys = {"background", "foreground", "selection-background", "selection-foreground", "cursor-color", "cursor-text"}
    for line in raw.decode("utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        key, separator, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if not separator:
            raise ValueError("missing assignment")
        if key == "palette":
            index, separator, value = value.partition("=")
            if not separator or not index.isdecimal() or not 0 <= int(index) < 16 or int(index) in ansi:
                raise ValueError("invalid or duplicate ANSI index")
            if not HEX.fullmatch(value):
                raise ValueError("invalid RGB color")
            ansi[int(index)] = value
        elif key in keys and key not in colors and HEX.fullmatch(value):
            colors[key] = value
        else:
            raise ValueError("unknown, duplicate, or non-color setting: " + key)
    if set(colors) != keys or set(ansi) != set(range(16)):
        raise ValueError("incomplete color palette")
    return colors, [ansi[i] for i in range(16)]


def upstream_input(root):
    files = [root / "LICENSE", root / "CREDITS.md", *sorted((root / "ghostty").iterdir())]
    digest = hashlib.sha256()
    raw_files = {}
    for path in sorted(files, key=lambda p: p.relative_to(root).as_posix()):
        if path.is_symlink() or not path.is_file():
            raise ValueError("source must contain only regular palette/notice files")
        raw = path.read_bytes()
        name = path.relative_to(root).as_posix()
        digest.update(name.encode() + b"\0" + len(raw).to_bytes(8, "big") + raw)
        raw_files[name] = raw
    if digest.hexdigest() != SOURCE_SHA256:
        raise ValueError("upstream source hash differs from pinned revision " + REVISION)
    themes = []
    for path in sorted((root / "ghostty").iterdir(), key=lambda p: (p.name.casefold(), p.name)):
        if path.name in EXCLUDED:
            continue
        raw = raw_files["ghostty/" + path.name]
        colors, ansi = parse_palette(raw)
        themes.append({
            "id": identifier(path.name), "name": path.name,
            "background": colors["background"], "foreground": colors["foreground"],
            "selection_background": colors["selection-background"],
            "selection_foreground": colors["selection-foreground"], "ansi": ansi,
            "source_sha256": hashlib.sha256(raw).hexdigest(),
        })
    result = {"schema": 1, "repository": "https://github.com/mbadolato/iTerm2-Color-Schemes",
              "revision": REVISION, "source_sha256": SOURCE_SHA256,
              "excluded": sorted(EXCLUDED), "themes": themes}
    return (json.dumps(result, ensure_ascii=True, indent=2) + "\n").encode(), raw_files


def validate(raw):
    if hashlib.sha256(raw).hexdigest() != CATALOG_SHA256:
        raise ValueError("bundled color-data hash mismatch; review the pin before updating")
    data = json.loads(raw)
    if data["revision"] != REVISION or data["source_sha256"] != SOURCE_SHA256 or len(data["themes"]) != COUNT:
        raise ValueError("catalog metadata does not match pin")
    seen = {"dark": "reserved", "light": "reserved", "sagedark": "reserved", "sagelight": "reserved"}
    for theme in data["themes"]:
        name = theme["name"]
        if not name.isascii() or not name or any(ord(c) < 32 or ord(c) == 127 for c in name):
            raise ValueError("non-displayable theme name")
        if identifier(name) != theme["id"] or name in EXCLUDED:
            raise ValueError("invalid ID or excluded theme")
        for key in {normalized(name), normalized(theme["id"])}:
            if key in seen:
                raise ValueError("ambiguous theme lookup: " + name + " / " + seen[key])
            seen[key] = name
        if len(theme["ansi"]) != 16:
            raise ValueError("palette must have 16 ANSI colors")
        for value in [theme[k] for k in ("background", "foreground", "selection_background", "selection_foreground")] + theme["ansi"]:
            if not HEX.fullmatch(value):
                raise ValueError("invalid RGB value")
    return data


def generate(data):
    lines = ["// @generated by tools/import-themes.py; do not edit.",
             "// Color values only; no upstream configuration or scripts are executed.",
             "// Source: " + data["repository"] + " @ " + REVISION,
             "// Input SHA-256: " + CATALOG_SHA256,
             "// Attribution and license exceptions: third_party/iterm2-themes/README.md",
             "use super::BuiltinTheme;", "", "#[rustfmt::skip]",
             "pub(super) static BUILTINS: &[BuiltinTheme] = &["]
    for theme in data["themes"]:
        lines += ["    BuiltinTheme {", f'        name: {json.dumps(theme["name"])}, id: {json.dumps(theme["id"])},']
        for key in ("background", "foreground", "selection_background", "selection_foreground"):
            lines.append(f'        {key}: 0x{theme[key][1:].lower()},')
        lines.append("        ansi: [" + ", ".join("0x" + c[1:].lower() for c in theme["ansi"]) + "],")
        lines.append("    },")
    return ("\n".join(lines) + "\n];\n").encode()


def self_test():
    sample = b"\n".join([f"palette = {i}=#123456".encode() for i in range(16)] +
                        [f"{key} = #123456".encode() for key in ("background", "foreground", "selection-background", "selection-foreground", "cursor-color", "cursor-text")])
    assert parse_palette(sample)[1] == ["#123456"] * 16
    for malformed in [sample + b"\ncommand = rm -rf /", sample + b"\npalette = 0=#ffffff", sample.replace(b"#123456", b"#zzzzzz", 1), sample.replace(b"palette = 15", b"palette = 16", 1), sample.split(b"\n", 1)[1]]:
        try:
            parse_palette(malformed)
        except ValueError:
            pass
        else:
            raise AssertionError("malformed input accepted")
    assert normalized(" Catppuccin-MOCHA_") == "catppuccinmocha"
    assert normalized("Dracula+") != normalized("Dracula")
    print("Importer parser checks passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, help="local checkout of the pinned collection")
    parser.add_argument("--output", type=Path, default=ROOT / "src/theme_data.rs")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    outputs = {}
    if args.upstream:
        raw, files = upstream_input(args.upstream)
        outputs[DATA / "palettes.json"] = raw
        outputs.update({DATA / key: files[key] for key in ("LICENSE", "CREDITS.md")})
    else:
        raw = (DATA / "palettes.json").read_bytes()
    outputs[args.output] = generate(validate(raw))
    for path, generated in outputs.items():
        if args.check:
            if not path.exists() or path.read_bytes() != generated:
                raise ValueError("generated output differs: " + str(path))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(generated)
    print(f"{'Verified' if args.check else 'Generated'} {COUNT} palettes from {REVISION}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError) as error:
        sys.exit(str(error))
