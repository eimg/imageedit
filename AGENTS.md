# ImageEdit agent guide

ImageEdit is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for a future Windows build. The product
takes cues from GIMP, with native menus, a fixed toolbox and Layers panel, and a
tabbed canvas. File commands belong in the menu. The workspace favors canvas
space and has no top toolbar.

## Current product scope

This is an editor shell. File opening, tabs, fitted and fixed zoom, pan,
grid, center guides, and preview color sampling work. The image loader supports
PNG, JPEG, and WebP. The foreground and background swatches can be swapped or
reset. Most editing tools display their role in Tool Options. Menu commands for
image or layer changes, project persistence, clipboard workflows, and export
are placeholders. The Layers panel displays the open image as a source preview.
Use the next user request to decide which editing behavior to implement.

## Code map

- `src/main.rs` starts GPUI, creates or reopens the main window, and routes
  launch arguments and macOS file URLs to the workspace.
- `src/commands.rs` defines actions, shortcuts, native menus, and toolbox
  metadata. Reuse those actions for UI entry points. A `Planned` menu action
  stays disabled until there is a real handler.
- `src/document.rs` decodes source files and applies EXIF orientation. It keeps
  original dimensions and a bounded BGRA preview for GPUI. Preview pixels are
  presentation data and can be downsampled.
- `src/workspace.rs` owns tabs, view state, and the asynchronous image loader.
  The canonical source path identifies an already open file. Tab state follows
  the document when switching tabs.
- `src/workspace/view.rs` lays out the shell and paints the canvas. Panning,
  overlays, and sampling share the preview geometry. The Eyedropper samples
  the preview and sets the foreground swatch.
- `src/icons.rs` contains tool glyphs. `macos/AppIcon.svg` is the editable
  app icon and `macos/AppIcon.icns` is its packaged form.
- `src/macos.rs` applies the same icon to the running Dock entry, including
  development launches from Cargo.
- `scripts/bundle-macos.sh` copies the executable, Info.plist, and icon into
  `dist/ImageEdit.app` and signs the local bundle.

## Build and check

From the repository root, use a recent Rust toolchain and Xcode. Run
`cargo fmt --check`, `cargo clippy --locked -- -D warnings`, and
`cargo test --locked`. Keep `Cargo.lock` tracked. GPUI on macOS uses
`font-kit` for text and compiles its Metal shaders at runtime.

Run `./scripts/bundle-macos.sh` and open `dist/ImageEdit.app` for native smoke
checks. Test a valid image and an unsupported file, switch and close tabs, and
try Finder's **Open With** after rebuilding packaging changes. For canvas work,
test fit, fixed zoom, pan at a zoom that clips the image, and sampling after
panning. Confirm a planned action stays visibly disabled until implemented.

Keep local build output in `target/` and `dist/`. Use temporary directories
for test images. Keep machine-specific settings and credentials out of source.
