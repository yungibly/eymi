#!/bin/sh
# Project-local developer tool; official release digests from GitHub's assets API.
set -eu

version=0.1.0-beta.5
case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)
        platform=aarch64-apple-darwin
        expected=4ff9a72e891c9643ed1c126510def9a8f348aa0c20d535d7283ae831e762235a
        ;;
    Darwin-x86_64)
        platform=x86_64-apple-darwin
        expected=8744d5b4a34c8bd58e85352c85b9fe5421cc01da8d67c7d8609cd8c8bd9f40ef
        ;;
    *)
        echo "This bootstrap currently supports macOS arm64 and x86_64." >&2
        exit 1
        ;;
esac

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$script_dir/../.." && pwd)
release_dir="$repo_dir/target/tools/tui-test/$version"
install_dir="$release_dir/$platform"
asset="tui-test-$platform.tar.gz"
archive="$release_dir/$asset"
mkdir -p "$install_dir"
temporary_dir=$(mktemp -d "$release_dir/.install.XXXXXX")
trap 'rm -rf "$temporary_dir"' EXIT
trap 'exit 1' HUP INT TERM

if [ ! -f "$archive" ]; then
    curl --fail --location --silent --show-error --retry 2 \
        --connect-timeout 15 --max-time 120 \
        "https://github.com/microsoft/tui-test/releases/download/$version/$asset" \
        -o "$temporary_dir/$asset"
    archive="$temporary_dir/$asset"
fi
actual=$(shasum -a 256 "$archive" | awk '{print $1}')
if [ "$actual" != "$expected" ]; then
    echo "Checksum mismatch for $archive; nothing was installed." >&2
    exit 1
fi

tar -xzf "$archive" -C "$temporary_dir" tui-test
chmod 755 "$temporary_dir/tui-test"
"$temporary_dir/tui-test" --version >&2
if [ "$archive" != "$release_dir/$asset" ]; then
    mv "$archive" "$release_dir/$asset"
fi
mv "$temporary_dir/tui-test" "$install_dir/tui-test"
printf '%s\n' "$install_dir/tui-test"
