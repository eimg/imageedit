# ImageEdit agent guide

ImageEdit is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for a future Windows build. The product
takes cues from GIMP, with native menus, a fixed toolbox and Layers panel, and a
tabbed canvas. File commands belong in the menu. The workspace favors canvas
space and has no top toolbar.

For planned work, start with [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)
and the latest pass handoff.

## Current product scope

ImageEdit supports layered raster editing. Open or create an image, crop it,
sample or set swatches, and fill connected regions on the active layer. Rectangle
selection limits fill and Clear to Transparency, and Crop to Selection uses its
bounds. Move repositions the active layer, and the Layer menu can center it,
fit it to the canvas, or crop it to its opaque pixels. Image → Crop to Content
does the same for the canvas. Copy and Cut use the active layer inside the
selection, Paste adds a centered layer, and Open as Layers imports images
the same way. Each tab can undo and redo crops, fills and
layer changes, and undoing back to the saved state clears the unsaved marker.
Tabs retain zoom and pan. The canvas
paints 256 px tiles and builds a coarser level when the image is fitted or
zoomed out. It also supports a grid and center guides. Save and Save As handle
ImageEdit projects,
PNG, JPEG, and WebP. A project stores the layer stack with lossless pixels.
Export writes flattened PNG, JPEG, or WebP copies without changing the tab's
file. Raster Save is offered only while a document is a single plain layer.

## Code map

- `src/main.rs` starts GPUI, creates or reopens the main window, and routes
  launch arguments and macOS file URLs to the workspace.
- `src/commands.rs` defines actions, shortcuts, native menus, and toolbox
  metadata. Reuse those actions for UI entry points. A `Planned` menu action
  stays disabled until there is a real handler.
- `src/document.rs` decodes and saves raster files and prepares the
  full-resolution composite, tile images and bounded BGRA preview for GPUI.
- `src/preview.rs` owns the 256 px tile cache, the one-pixel sampling apron
  and the Fit level choice. The canvas paints those tiles.
- `src/selection.rs` is the canvas-space selection: unrestricted, empty, full,
  a rectangle, or the inverse of a rectangle. `src/workspace/selection.rs`
  drags the rectangle, resizes it from its handles, moves it from inside, and
  clears it on a click outside. Over the box the painted crosshair gives way
  to a hand or resize cursor. The outline is a dashed contour. Shift-add
  waits for the mask in P12.
- `src/geometry.rs` centers a layer, finds nonzero-alpha bounds, and copies
  the on-canvas part of a layer into a canvas-sized image.
- `src/document/content.rs` owns the canvas dimensions and raster layer stack.
  It handles crop, layer-local bucket fill, layer offsets, and content crops.
  `src/document/composite.rs` blends visible layers for display, sampling, and
  raster output.
- `src/project.rs` reads and writes the version 2 project format and applies
 the same size limits in both directions. ImageEdit has not shipped, so older
 project versions are not supported. `fixtures/projects/` holds a golden
 project that the reader must keep opening. Regenerate it only when the
 format intentionally changes.
- `src/workspace.rs` owns tabs, view state, and background open, edit, undo
  and redo jobs. A job publishes only if its tab's edit generation is
  unchanged. A coarser preview level publishes only while the composite it
  was built from is still current. The canonical path identifies an already
  open file.
- `src/workspace/history.rs` holds each tab's undo and redo snapshots, its
  current and saved revisions, and the entry and memory limits. Snapshots
  share layer pixels, and each shared buffer counts once toward the limits.
- `src/workspace/clipboard.rs` copies the selected part of the active layer,
  pastes a centered layer, and imports rasters above the active layer.
- `src/workspace/files.rs` handles native file dialogs, Save routing,
  background saving and export, and unsaved-change confirmation.
- `src/workspace/layers.rs` owns the Layers panel, layer commands, opacity
  drag coalescing, and the Move preview. The preview is display-only until
  the pointer is released. Move's canvas cursor is the four-arrow glyph, and
  its tool options repeat the align, fit, and content-crop commands.
  `src/workspace/dialogs.rs` renders the canvas dimensions, color, JPEG quality
  and layer dialogs. `src/workspace/color.rs`
  handles the HSV wheel and brightness slider.
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
