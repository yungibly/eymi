#!/usr/bin/env python3
"""Build and verify Eymi release archives using Python 3.11+ standard libraries."""

import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import re
import struct
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu")
THEMES = "third_party/iterm2-themes"
RUST = "third_party/rust"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Expected a regular file, not a link: {path}")
    return path.read_bytes()


def version(root, tag):
    package = tomllib.loads(regular(root / "Cargo.toml").decode())["package"]
    value = package["version"]
    if package["name"] != "eymi" or not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", value):
        raise ValueError("Expected the Eymi package with a literal SemVer version")
    if tag != "v" + value:
        raise ValueError(f"Tag {tag!r} does not match Cargo version v{value}")
    for required in ["README.md", "LICENSE", f"{THEMES}/LICENSE", f"{THEMES}/CREDITS.md",
                     f"{THEMES}/README.md", f"{THEMES}/additional-licenses.json"]:
        if not regular(root / required):
            raise ValueError(f"Required release attribution is empty: {required}")
    return value


def architecture(data):
    if data[:4] == b"\xcf\xfa\xed\xfe" and len(data) >= 8:
        cpu = struct.unpack_from("<I", data, 4)[0]
        if cpu == 0x0100000C:
            return "aarch64-apple-darwin"
        if cpu == 0x01000007:
            return "x86_64-apple-darwin"
    if data[:6] == b"\x7fELF\x02\x01" and len(data) >= 20:
        if struct.unpack_from("<H", data, 18)[0] == 62:
            return "x86_64-unknown-linux-gnu"
    raise ValueError("Expected a supported 64-bit Mach-O or x86-64 ELF binary")


def tree(path):
    if path.is_symlink() or not path.is_dir():
        raise ValueError(f"Expected a regular directory: {path}")
    files = {}
    for entry in sorted(path.rglob("*")):
        if entry.is_symlink():
            raise ValueError(f"Release tree contains a symlink: {entry}")
        if entry.is_file():
            files[entry.relative_to(path).as_posix()] = regular(entry)
        elif not entry.is_dir():
            raise ValueError(f"Release tree contains a special file: {entry}")
    return files


def verify_notices(files, target, lock_sha256):
    manifest = json.loads(files[f"{RUST}/manifest.json"])
    if manifest["cargo_lock_sha256"] != lock_sha256:
        raise ValueError("Rust notice bundle does not match Cargo.lock")
    if not files[f"{RUST}/README.md"]:
        raise ValueError("Rust notice bundle README is empty")
    if manifest["target"] != target or not manifest["packages"]:
        raise ValueError("Rust notice bundle has no dependencies or targets a different platform")
    seen = set()
    for package in manifest["packages"]:
        identity = (package["name"], package["version"])
        if identity in seen or not package["files"]:
            raise ValueError(f"Duplicate package or missing dependency notices: {identity}")
        seen.add(identity)
        granted = set(package["license_files"])
        if not granted or not granted <= {notice["path"] for notice in package["files"]}:
            raise ValueError(f"Dependency has no included license grant: {identity}")
        for notice in package["files"]:
            name = f"{RUST}/{notice['path']}"
            if not files[name] or digest(files[name]) != notice["sha256"]:
                raise ValueError(f"Rust dependency notice checksum mismatch: {name}")


def archive_name(value, target):
    if target not in TARGETS:
        raise ValueError(f"Unsupported target: {target}")
    return f"eymi-{value}-{target}.tar.gz"


def package(root, binary, notices, output, tag, target, epoch):
    value = version(root, tag)
    if not 0 <= epoch <= 0xFFFFFFFF:
        raise ValueError("Archive epoch must fit the gzip timestamp field")
    data = regular(binary)
    if architecture(data) != target:
        raise ValueError("Binary architecture does not match the requested target")
    files = {"eymi": data, "README.md": regular(root / "README.md"), "LICENSE": regular(root / "LICENSE")}
    files.update({f"{THEMES}/{name}": content for name, content in tree(root / THEMES).items()})
    files.update({f"{RUST}/{name}": content for name, content in tree(notices).items()})
    verify_notices(files, target, digest(regular(root / "Cargo.lock")))
    output.mkdir(parents=True, exist_ok=True)
    destination = output / archive_name(value, target)
    # Exclusive creation prevents a retry from overwriting previously reviewed assets.
    with destination.open("xb") as raw:
        try:
            with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=epoch, compresslevel=9) as compressed:
                with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                    for name, content in sorted(files.items()):
                        info = tarfile.TarInfo(name)
                        info.size, info.mtime = len(content), epoch
                        info.mode = 0o755 if name == "eymi" else 0o644
                        info.uid = info.gid = 0
                        info.uname = info.gname = ""
                        archive.addfile(info, io.BytesIO(content))
        except BaseException:
            destination.unlink()
            raise
    verify_archive(root, destination, target)
    return destination


def verify_archive(root, archive_path, target):
    files = {}
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            if not member.isfile() or path.is_absolute() or ".." in path.parts or str(path) != member.name:
                raise ValueError(f"Unsafe archive member: {member.name}")
            if member.name in files:
                raise ValueError(f"Duplicate archive member: {member.name}")
            if member.mode != (0o755 if member.name == "eymi" else 0o644):
                raise ValueError(f"Unexpected archive file mode: {member.name}")
            files[member.name] = archive.extractfile(member).read()
    if architecture(files["eymi"]) != target:
        raise ValueError("Archive contains the wrong binary architecture")
    expected = {"README.md": regular(root / "README.md"), "LICENSE": regular(root / "LICENSE")}
    expected.update({f"{THEMES}/{name}": data for name, data in tree(root / THEMES).items()})
    for name, data in expected.items():
        if files.get(name) != data:
            raise ValueError(f"Missing or modified release attribution: {name}")
    allowed = set(expected) | {"eymi"}
    if any(name not in allowed and not name.startswith(RUST + "/") for name in files):
        raise ValueError("Archive contains unexpected files")
    verify_notices(files, target, digest(regular(root / "Cargo.lock")))


def checksum_text(root, directory, tag, targets):
    value = version(root, tag)
    if len(set(targets)) != len(targets) or not targets:
        raise ValueError("Expected a nonempty list of distinct release targets")
    expected = {archive_name(value, target): target for target in targets}
    actual = {path.name for path in directory.iterdir() if path.name != "SHA256SUMS"}
    if actual != set(expected):
        raise ValueError(f"Release assets differ: missing {set(expected) - actual}, unexpected {actual - set(expected)}")
    rows = []
    for name, target in sorted(expected.items()):
        data = regular(directory / name)
        verify_archive(root, directory / name, target)
        rows.append(f"{digest(data)}  {name}\n")
    return "".join(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate", help="Validate tag, package name, version, and root/theme notices")
    validate.add_argument("--tag", required=True)
    pack = commands.add_parser("pack", help="Create one flat, reproducible archive without overwriting")
    for flag in ("binary", "notices", "output-dir"):
        pack.add_argument("--" + flag, required=True, type=Path)
    pack.add_argument("--tag", required=True)
    pack.add_argument("--target", required=True, choices=TARGETS)
    pack.add_argument("--epoch", type=int, default=0, help="Typically the tagged commit timestamp")
    for command in ("checksums", "verify"):
        sub = commands.add_parser(command)
        sub.add_argument("--directory", type=Path, required=True)
        sub.add_argument("--tag", required=True)
        sub.add_argument("--targets", nargs="+", choices=TARGETS, default=TARGETS)
    args = parser.parse_args()
    try:
        if args.command == "validate":
            print(f"version={version(args.source_root, args.tag)}\ntag={args.tag}")
        elif args.command == "pack":
            print(package(args.source_root, args.binary, args.notices, args.output_dir,
                          args.tag, args.target, args.epoch))
        else:
            expected = checksum_text(args.source_root, args.directory, args.tag, args.targets)
            manifest = args.directory / "SHA256SUMS"
            if args.command == "checksums":
                with manifest.open("x", encoding="utf-8", newline="\n") as handle:
                    handle.write(expected)
            elif regular(manifest).decode() != expected:
                raise ValueError("SHA256SUMS does not exactly match the verified release assets")
            print(f"Verified {len(args.targets)} release archive(s)")
    except (ValueError, KeyError, OSError, tarfile.TarError) as error:
        parser.exit(1, f"Release validation failed: {error}\n")


if __name__ == "__main__":
    main()
