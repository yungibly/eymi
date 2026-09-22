#!/usr/bin/env python3
"""Copy actual notices for locked target dependencies; no SPDX license text substitution."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SUFFIX = r"(?:[-_][A-Za-z0-9._-]+|\.(?:txt|md|rst|html|adoc))?"
GRANT = re.compile(r"^(?:licen[cs]e|copying)" + SUFFIX + r"$", re.IGNORECASE)
SUPPLEMENT = re.compile(r"^(?:copyright|notice|authors)" + SUFFIX + r"$", re.IGNORECASE)
OVERRIDES = Path(__file__).with_name("license-overrides.json")


def dependency_packages(metadata):
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    if root is None:
        raise ValueError("A single root package is required for release notices")
    seen, pending = set(), [root]
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            if any(kind["kind"] in (None, "build") for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    return sorted((package for package in metadata["packages"] if package["id"] in seen - {root}),
                  key=lambda package: (package["name"], package["version"], package["id"]))


def notice_files(package, locked):
    root = Path(package["manifest_path"]).parent.resolve()
    grants = {path for path in root.rglob("*") if GRANT.fullmatch(path.name) and path.is_file()
              and path.suffix.lower() not in (".rs", ".c", ".h", ".py", ".js")}
    candidates = grants | {path for path in root.rglob("*")
                           if SUPPLEMENT.fullmatch(path.name) and path.is_file()}
    if package.get("license_file"):
        path = Path(package["license_file"])
        path = path if path.is_absolute() else root / path
        candidates.add(path)
        grants.add(path)
    config = json.loads(OVERRIDES.read_text())
    override = next((entry for entry in config["packages"]
                     if (entry["name"], entry["version"]) == (package["name"], package["version"])), None)
    result, license_files = {}, []
    if override:
        identity = (package["name"], package["version"], package.get("source"))
        vcs = json.loads((root / ".cargo_vcs_info.json").read_text())["git"]["sha1"]
        if (package.get("source") != override["source"] or locked.get(identity) != override["checksum"]
                or vcs != override["vcs_sha1"] or "MIT" not in (package.get("license") or "").split()):
            raise ValueError(f"Pinned notice override identity changed: {identity}")
        for entry in config["files"]:
            data = (OVERRIDES.parent / entry["file"]).read_bytes()
            if hashlib.sha256(data).hexdigest() != entry["sha256"]:
                raise ValueError("Pinned notice override text changed")
            result[entry["name"]] = data
            if entry["role"] == "standard_license":
                license_files.append(entry["name"])
    if not grants and not license_files:
        raise ValueError(f"No upstream license grant found for {package['name']} {package['version']}; review required")
    candidates.update(path for path in root.iterdir() if path.name.lower().startswith("readme") and path.is_file())
    for path in sorted(candidates):
        if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root):
            raise ValueError(f"Notice is missing, linked, or outside its crate: {path}")
        content = path.read_bytes()
        if not content:
            raise ValueError(f"Upstream notice is empty: {path}")
        relative = path.relative_to(root).as_posix()
        if relative in result:
            raise ValueError(f"Override collides with an upstream notice: {relative}")
        result[relative] = content
        if path in grants:
            license_files.append(relative)
    if override:
        override = override | {"texts": [{k: v for k, v in text.items() if k != "file"}
                                         for text in config["files"]]}
    return result, sorted(license_files), override


def generate(metadata, target, output, lock_sha256, locked=None):
    records, payload = [], {}
    for package in dependency_packages(metadata):
        name, value = package["name"], package["version"]
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[0-9A-Za-z.+-]+", value):
            raise ValueError("Unsafe dependency name or version")
        prefix = f"{name}-{value}"
        if any(record["directory"] == prefix for record in records):
            raise ValueError(f"Ambiguous dependency sources for {prefix}")
        notices, grants, override = notice_files(package, locked or {})
        record = {"name": name, "version": value, "directory": prefix,
                  "license": package.get("license"), "source": package.get("source"),
                  "repository": package.get("repository"), "authors": package.get("authors", []),
                  "license_files": [f"{prefix}/{name}" for name in grants], "files": []}
        if override:
            record["notice_override"] = override
        for relative, data in sorted(notices.items()):
            path = f"{prefix}/{relative}"
            payload[path] = data
            record["files"].append({"path": path, "sha256": hashlib.sha256(data).hexdigest()})
        records.append(record)
    if not records:
        raise ValueError("No resolved production/build dependencies; refusing an empty notice bundle")
    manifest = {"schema_version": 1, "target": target, "cargo_lock_sha256": lock_sha256, "packages": records}
    payload["manifest.json"] = (json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()
    payload["README.md"] = (
        "# Rust dependency notices\n\n"
        "Generated from Cargo.lock and `cargo metadata --locked --filter-platform " + target + "`.\n"
        "Includes resolved normal and build dependencies; development-only dependencies are excluded.\n"
        "Original license/notice files and crate README files are copied without changes.\n"
        "Pinned objc2-family overrides preserve the upstream licensing statement and include standard MIT text.\n"
        "STANDARD-MIT.txt is standard license text with placeholders, not an invented upstream copyright notice.\n"
        "Exact package authors, source, chosen license, and override provenance are recorded in the manifest.\n"
        "SPDX expressions are recorded verbatim; they do not replace the included upstream terms.\n"
        "The manifest records each file's SHA-256. Review new dependencies and missing notices before publishing.\n"
    ).encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()  # Never replace a preexisting bundle.
    try:
        for name, data in sorted(payload.items()):
            destination = output / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
    except BaseException:
        shutil.rmtree(output)
        raise
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--locked", "--filter-platform", args.target, "--format-version", "1",
            "--manifest-path", str(args.source_root / "Cargo.toml"),
        ], cwd=args.source_root))
        lock_bytes = (args.source_root / "Cargo.lock").read_bytes()
        locked = {(p["name"], p["version"], p.get("source")): p.get("checksum")
                  for p in tomllib.loads(lock_bytes.decode())["package"]}
        result = generate(metadata, args.target, args.output, hashlib.sha256(lock_bytes).hexdigest(), locked)
        print(f"Copied actual notices for {len(result['packages'])} locked dependencies ({args.target})")
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Dependency notice generation failed: {error}\n")


if __name__ == "__main__":
    main()
