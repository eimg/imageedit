#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != Linux ]]; then
  echo "Package Linux builds on Linux (a native runner or container)." >&2
  exit 1
fi
case "$(uname -m)" in
  x86_64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) echo "Only x86_64 and ARM64 are supported." >&2; exit 1 ;;
esac
case "${1:-}" in
  "") cargo build --release --locked ;;
  --no-build) ;;
  *) echo "Usage: $0 [--no-build]" >&2; exit 1 ;;
esac

version=$(python3 -c 'import re; print(re.search(r"^version = \"([^\"]+)\"", open("Cargo.toml").read(), re.M)[1])')
name="Lime-$version-linux-$arch"
binary="${CARGO_TARGET_DIR:-target}/release/lime"
[[ -f "$binary" ]] || { echo "Missing release binary: $binary" >&2; exit 1; }
python3 - "$binary" "$arch" <<'PYARCH'
import struct
import sys
with open(sys.argv[1], "rb") as source:
    header = source.read(20)
expected = {"x86_64": 62, "aarch64": 183}[sys.argv[2]]
if header[:6] != b"\x7fELF\x02\x01" or len(header) < 20 or struct.unpack_from("<H", header, 18)[0] != expected:
    raise SystemExit("Release binary is not a Linux ELF64 executable for " + sys.argv[2])
PYARCH
mkdir -p dist
stage=$(mktemp -d "dist/.linux-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name"
install -m 755 "$binary" "$stage/$name/lime"
install -m 644 linux/com.eimg.lime.desktop "$stage/$name/"
install -m 644 macos/AppIcon.svg "$stage/$name/com.eimg.lime.svg"
install -m 644 linux/README.txt "$stage/$name/READ-ME.txt"
# Record the actual linked dependencies. Drivers and dlopen libraries remain
# host requirements; this is a portable download, not a static binary.
ldd "$stage/$name/lime" > "$stage/$name/linked-libraries.txt"
if grep -q 'not found' "$stage/$name/linked-libraries.txt"; then
  cat "$stage/$name/linked-libraries.txt" >&2
  exit 1
fi
install -m 755 "$binary" "dist/$name"
tar -czf "dist/$name.tar.gz" -C "$stage" "$name"
(cd dist && sha256sum "$name" "$name.tar.gz" > "$name.sha256")
printf 'Linux downloads:\n  dist/%s\n  dist/%s.tar.gz\n  dist/%s.sha256\n' "$name" "$name" "$name"
