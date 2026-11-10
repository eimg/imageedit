# ImageEdit

<img src="macos/AppIcon.svg" width="96" alt="ImageEdit app icon">

ImageEdit is a lightweight desktop image editor in development. It uses Rust
and [GPUI](https://gpui.rs/) with a GIMP-inspired workspace and native macOS
menus. The current milestone is the editor shell and image preview workflow.

## Try it on macOS

Install a recent Rust toolchain with Cargo and select an Xcode installation.
Clone the repository, then run these commands from its root.

```sh
./scripts/bundle-macos.sh
open dist/ImageEdit.app
```

The script builds an ad hoc signed local app with the ImageEdit icon and
Finder file associations. You can also run `cargo run --locked` for
development. Use the app bundle to test Finder integration and app identity.

## What's working

- Open PNG, JPEG, and WebP images from **File → Open Image**, by dropping files
  onto the canvas, or through Finder's **Open With** menu.
- Open several images in tabs. Reopening a file focuses its existing tab.
- Fit an image to the window or choose a zoom preset. Use **Pan** to drag a
  zoomed preview and toggle the grid or center guides from **View**.
- Use **Eyedropper** to sample the preview into the foreground swatch. Swap or
  reset the foreground and background swatches in the toolbox.

| Action | Shortcut |
| --- | --- |
| Open Image | ⌘O |
| Close Tab | ⌘W |
| Next Tab | Control-Tab |
| Previous Tab | Control-Shift-Tab |

The menus and toolbox also show planned editing features, including crop, text,
layers, color adjustments, filters, undo, project saving, and PNG/JPEG export.
Planned tools can be selected to inspect their place in the workspace. Commands
that require the editing engine appear disabled. Windows is a future target.

## Development

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
```

See [AGENTS.md](AGENTS.md) for the architecture, current scope, and macOS
smoke checks.
