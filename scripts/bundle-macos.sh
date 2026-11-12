#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "This script builds a macOS app bundle." >&2
    exit 1
fi

cd "$(dirname "$0")/.."
cargo build --release --locked --target-dir target

app="dist/Lime.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/Info.plist"
cp macos/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cp target/release/lime "$app/Contents/MacOS/lime"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
plist="$app/Contents/Info.plist"
plutil -lint "$plist" >/dev/null
cmp macos/Info.plist "$plist"
for type in com.eimg.lime.project com.imageedit.project public.png public.jpeg org.webmproject.webp; do
    if ! grep -q "$type" "$plist"; then
        echo "The signed bundle is missing the $type document type." >&2
        exit 1
    fi
done
printf 'Built %s\n' "$app"
