#!/usr/bin/env python3
"""Verify published Eymi archives, render the formula, and update one tap file.

Fetching is anonymous. Only the update command reads BREW_TAP_TOKEN, and that
credential is sent solely to the fixed GitHub API endpoint for our tap.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.request

REPOSITORY = "yungibly/eymi"
TAP = "yungibly/homebrew-tap"
TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
)
VERSION = r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"


class ReleaseError(Exception):
    pass


def version_from_tag(tag: str) -> str:
    if not re.fullmatch("v" + VERSION, tag):
        raise ReleaseError("Homebrew requires a stable release tag such as v0.1.0")
    return tag[1:]


def archive_names(tag: str) -> list[str]:
    version = version_from_tag(tag)
    return [f"eymi-{version}-{target}.tar.gz" for target in TARGETS]


def checksums(text: str, tag: str) -> dict[str, str]:
    expected = set(archive_names(tag))
    result = {}
    for line in text.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if not match or match[2] not in expected or match[2] in result:
            raise ReleaseError("SHA256SUMS contains an invalid, duplicate, or unexpected entry")
        result[match[2]] = match[1]
    if set(result) != expected:
        raise ReleaseError("SHA256SUMS does not cover every supported release target")
    return result


def verified_checksums(directory: Path, tag: str) -> dict[str, str]:
    result = checksums((directory / "SHA256SUMS").read_text(encoding="utf-8"), tag)
    for name, expected in result.items():
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise ReleaseError(f"Missing regular release archive: {name}")
        with path.open("rb") as stream:
            actual = hashlib.file_digest(stream, "sha256").hexdigest()
        if actual != expected:
            raise ReleaseError(f"Checksum mismatch: {name}")
    return result


def render_formula(tag: str, hashes: dict[str, str]) -> str:
    version = version_from_tag(tag)
    # Validate even when called independently of the file-verification path.
    hashes = checksums("\n".join(f"{value}  {key}" for key, value in hashes.items()), tag)

    def download(target: str, indent: int) -> str:
        name = f"eymi-{version}-{target}.tar.gz"
        prefix = " " * indent
        url = f"https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"
        return f'{prefix}url "{url}"\n{prefix}sha256 "{hashes[name]}"'

    return f'''class Eymi < Formula
  desc "Terminal Markdown editor with live preview and familiar shortcuts"
  homepage "https://github.com/{REPOSITORY}"
  version "{version}"
  license "MIT"

  on_macos do
    on_arm do
{download("aarch64-apple-darwin", 6)}
    end
    on_intel do
{download("x86_64-apple-darwin", 6)}
    end
  end

  on_linux do
    depends_on arch: :x86_64
{download("x86_64-unknown-linux-gnu", 4)}
  end

  def install
    bin.install "eymi"
    doc.install "README.md", "LICENSE"
    (pkgshare/"third_party").install "third_party/iterm2-themes", "third_party/rust"
  end

  test do
    assert_equal "eymi #{{version}}\\n", shell_output("#{{bin}}/eymi --version")
    source = "# Homebrew\\n\\n- [ ] Keep source.\\n"
    (testpath/"note.md").write(source)
    output = shell_output("#{{bin}}/eymi --no-state --snapshot --theme dark #{{testpath}}/note.md")
    assert_match "Homebrew", output
    assert_equal source, (testpath/"note.md").read
    assert_match "Catppuccin Mocha", shell_output("#{{bin}}/eymi --list-themes")
  end
end
'''


def request_bytes(url: str, *, limit: int, data: bytes | None = None,
                  token: str | None = None) -> bytes:
    headers = {"User-Agent": "eymi-release", "Accept": "application/vnd.github+json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
        headers["X-GitHub-Api-Version"] = "2022-11-28"
    if data is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=data, headers=headers,
                                     method="PUT" if data is not None else "GET")
    with urllib.request.urlopen(request, timeout=60) as response:
        body = response.read(limit + 1)
    if len(body) > limit:
        raise ReleaseError("Release response exceeded its size limit")
    return body


def fetch_published(tag: str, output: Path) -> None:
    names = [*archive_names(tag), "SHA256SUMS"]
    metadata = json.loads(request_bytes(
        f"https://api.github.com/repos/{REPOSITORY}/releases/tags/{tag}", limit=2 * 1024 * 1024
    ))
    if (metadata.get("tag_name") != tag or metadata.get("draft") is not False
            or metadata.get("prerelease") is not False):
        raise ReleaseError("Expected a published, stable release with the exact requested tag")
    assets = [entry.get("name") for entry in metadata.get("assets", [])]
    if any(assets.count(name) != 1 for name in names):
        raise ReleaseError("Published release is missing a unique required asset")
    output.mkdir(parents=True, exist_ok=False)
    for name in names:
        # Use only the fixed public URL, never an asset-provided URL or credential.
        limit = 64 * 1024 * 1024 if name.endswith(".tar.gz") else 4096
        body = request_bytes(
            f"https://github.com/{REPOSITORY}/releases/download/{tag}/{name}", limit=limit
        )
        (output / name).write_bytes(body)
    verified_checksums(output, tag)


def replacement_needed(current: str | None, proposed: str, tag: str) -> bool:
    incoming = tuple(map(int, version_from_tag(tag).split(".")))
    if current is None:
        return True
    match = re.search(r'^  version "(' + VERSION + r')"$', current, re.MULTILINE)
    if not match:
        raise ReleaseError("Existing tap formula has no unambiguous stable version")
    existing = tuple(map(int, match[1].split(".")))
    if existing > incoming or current == proposed:
        return False
    if existing == incoming:
        raise ReleaseError("Refusing to change the formula of an already released version")
    return True


def update_tap(formula: Path, tag: str) -> None:
    version = version_from_tag(tag)
    proposed = formula.read_text(encoding="utf-8")
    # Accept only our generated, fixed-repository formula, including its exact body.
    urls = re.findall(r'url "([^"\n]+)"\n +sha256 "([0-9a-f]{64})"', proposed)
    hashes = {url.rsplit("/", 1)[-1]: digest for url, digest in urls}
    if proposed != render_formula(tag, hashes):
        raise ReleaseError("Tap update requires the exact generated Eymi formula")
    token = os.environ.get("BREW_TAP_TOKEN")
    if not token:
        raise ReleaseError("BREW_TAP_TOKEN is required for a tap update")
    endpoint = f"https://api.github.com/repos/{TAP}/contents/Formula/eymi.rb"
    for attempt in range(3):
        try:
            item = json.loads(request_bytes(endpoint + "?ref=main", limit=256 * 1024, token=token))
            if item.get("type") != "file" or item.get("encoding") != "base64":
                raise ReleaseError("Tap path is not a regular GitHub content file")
            current = base64.b64decode(item["content"]).decode("utf-8")
            sha = item["sha"]
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
            current, sha = None, None
        if not replacement_needed(current, proposed, tag):
            print("Tap already contains this release or a newer version.")
            return
        payload = {
            "message": f"eymi v{version}", "branch": "main",
            "content": base64.b64encode(proposed.encode()).decode("ascii"),
            "committer": {"name": "github-actions[bot]",
                          "email": "41898282+github-actions[bot]@users.noreply.github.com"},
        }
        if sha:
            payload["sha"] = sha
        try:
            request_bytes(endpoint, limit=256 * 1024, token=token,
                          data=json.dumps(payload).encode())
            print(f"Updated {TAP}/Formula/eymi.rb to {version}.")
            return
        except urllib.error.HTTPError as error:
            # Compare-and-swap protects concurrent tap releases without rewriting history.
            if error.code not in (409, 422) or attempt == 2:
                raise
    raise ReleaseError("Tap update did not complete")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    fetch = commands.add_parser("fetch", help="Download and verify a public stable release")
    fetch.add_argument("--tag", required=True)
    fetch.add_argument("--output", type=Path, required=True)
    render = commands.add_parser("render", help="Verify local archives and render Formula/eymi.rb")
    render.add_argument("--tag", required=True)
    render.add_argument("--archives", type=Path, required=True)
    render.add_argument("--output", type=Path, required=True)
    update = commands.add_parser("update", help="Update only the designated tap formula")
    update.add_argument("--tag", required=True)
    update.add_argument("--formula", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "fetch":
            fetch_published(args.tag, args.output)
        elif args.command == "render":
            formula = render_formula(args.tag, verified_checksums(args.archives, args.tag))
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with args.output.open("x", encoding="utf-8") as stream:
                stream.write(formula)
        else:
            update_tap(args.formula, args.tag)
    except urllib.error.HTTPError as error:
        # HTTP bodies and request headers can contain credentials; never print them.
        print(f"Homebrew release request failed (HTTP {error.code}).", file=sys.stderr)
        return 1
    except (ReleaseError, OSError, ValueError, KeyError) as error:
        print(f"Homebrew release: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
