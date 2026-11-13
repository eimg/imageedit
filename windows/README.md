# Windows icon resources

`AppIcon.ico` uses the lime half-slice artwork from `macos/AppIcon.svg`, with
16, 20, 24, 32, 40, 48, 64, 128, and 256 pixel images for Windows display scaling.
Regenerate the ICO from that SVG when the artwork changes.

`build.rs` embeds `app.rc` in the native Windows executable using the SDK resource
compiler. Icon group ID 1 is deliberate: GPUI 0.2.2 loads that ID for its window
class, so the executable, title bar, taskbar, and task switcher share the icon.
The resource contains no manifest; GPUI's existing DPI/common-controls manifest
stays enabled separately.

The Windows workflow checks for both icon group 1 and manifest 1 in the release
executable before packaging. A macOS-to-Windows `cargo check` does not compile
these native resources.
