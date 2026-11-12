#!/bin/sh
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "This script builds a macOS disk image." >&2
    exit 1
fi

cd "$(dirname "$0")/.."
./scripts/bundle-macos.sh

version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' macos/Info.plist)
app="dist/Lime.app"
dmg="dist/Lime-$version.dmg"
staging=$(mktemp -d "${TMPDIR:-/tmp}/lime-dmg.XXXXXX")
trap 'rm -rf "$staging"' EXIT INT TERM

cp -R "$app" "$staging/Lime.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "Lime $version" -srcfolder "$staging" -ov -format UDZO "$dmg"
hdiutil verify "$dmg"
printf 'Built %s\n' "$dmg"
