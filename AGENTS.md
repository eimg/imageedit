# ImageEdit agent guide

ImageEdit is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for a future Windows build. The product
takes cues from GIMP, with native menus, a fixed toolbox and Layers panel, and a
tabbed canvas. File commands belong in the menu. The workspace favors canvas
space and has no top toolbar.

For planned work, start with [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)
and the latest pass handoff.

## Current product scope

ImageEdit supports single-layer raster editing. Open or create an image,
crop it, sample or set swatches, and fill connected regions. Tabs retain zoom
and pan. The canvas supports a grid and center guides. Save and Save As handle
ImageEdit projects, PNG, JPEG, and WebP. A project stores the layer stack with
lossless pixels. The UI does not create layers yet. Layer management, clipboard
workflows, undo, and separate export commands are placeholders.

## Code map

- `src/main.rs` starts GPUI, creates or reopens the main window, and routes
  launch arguments and macOS file URLs to the workspace.
- `src/commands.rs` defines actions, shortcuts, native menus, and toolbox
  metadata. Reuse those actions for UI entry points. A `Planned` menu action
  stays disabled until there is a real handler.
- `src/document.rs` decodes and saves raster files and prepares the
  full-resolution composite and bounded BGRA preview for GPUI.
- `src/document/content.rs` owns the canvas dimensions and raster layer stack.
  It handles crop and layer-local bucket fill. `src/document/composite.rs`
  blends visible layers for display, sampling, and raster output.
- `src/project.rs` reads and writes the version 2 project format and applies
 the same size limits in both directions. ImageEdit has not shipped, so older
 project versions are not supported. `fixtures/projects/` holds a golden
 project that the reader must keep opening. Regenerate it only when the
 format intentionally changes.
- `src/workspace.rs` owns tabs, view state, and background open and edit jobs.
  The canonical path identifies an already open file.
- `src/workspace/files.rs` handles native file dialogs, background saving,
  and unsaved-change confirmation.
- `src/workspace/dialogs.rs` renders the canvas dimensions and color dialogs.
  `src/workspace/color.rs` handles the HSV wheel and brightness slider.
- `src/workspace/canvas_tools.rs` maps canvas gestures to full-resolution
  pixels. `src/workspace/view.rs` lays out the shell and paints the canvas
  using the same geometry.
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
panning. Create an image with custom dimensions, fill and crop it, then save
and reopen a project and a raster image. Confirm a planned action stays
visibly disabled until implemented.

Keep local build output in `target/` and `dist/`. Use temporary directories
for test images. Keep machine-specific settings and credentials out of source.
