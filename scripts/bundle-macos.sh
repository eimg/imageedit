#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "This script builds a macOS app bundle." >&2
    exit 1
fi

cd "$(dirname "$0")/.."
cargo build --release --locked --target-dir target

app="dist/ImageEdit.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/Info.plist"
cp macos/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cp target/release/imageedit "$app/Contents/MacOS/imageedit"
codesign --force --sign - "$app"
printf 'Built %s\n' "$app"
