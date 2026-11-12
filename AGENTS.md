# Lime agent guide

Work directly on `main` unless the user explicitly requests another branch.

Lime is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for Windows. Hide and Quit stay in the macOS
app menu. Windows uses File → Exit. The product
takes cues from GIMP, with native menus, a docked, resizable, and foldable
toolbox, a Layers panel, and a tabbed canvas. File commands belong in the menu.
The workspace favors canvas space and has no top toolbar.
Windows has been manually reviewed by the maintainer and approved for release. A Windows-only `muda` adapter builds a native menu bar from the
shared GPUI menu tree, and `.github/workflows/windows.yml` produces an x64
portable ZIP. Releases distribute the EXE without an installer or file
associations. The macOS DMG is currently Apple silicon only. See README.md
for downloads and signing status.

Use the user's current request to scope work. This guide records the current
architecture and editing contracts; [README.md](README.md) describes using the
app. Update both when behavior changes. Completed plans, feedback queues, and
review handoffs belong in Git history rather than separate living documents.
Keep unrelated work and existing edits intact.

## Current product scope

Lime supports layered raster editing. The empty workspace can open a
file or create an image. Crop that image: drag the box, move it from inside,
resize it from its handles, and edit X, Y, Width, and Height in the Crop
options. Typed values apply immediately, and an empty field leaves the crop
unchanged. Sample or set swatches,
and fill connected regions on the active layer. Rectangle, oval, circle,
freehand or polygon, and wand selections limit fill and Clear to Transparency,
and Crop to Selection uses the selection bounds. Rectangle Select and Oval Select
are separate tools. Oval Select has a Circle option for an exact circle, and
Rectangle Select has a corner radius control with minus and plus buttons, and
the Width/Height fields accept typed values. Empty numeric text does not change
the selection.
Rectangle and oval drags may start or extend outside the canvas; coverage stays
bounded to canvas pixels and clipped curves retain their original geometry.
Shift adds to the selection, Control subtracts, and Shift-Control intersects. The wand and
bucket share a tolerance; zero matches a color exactly. Feather, Grow, and
Shrink edit the mask. Move repositions the active layer, and its tool options
align it to any canvas edge or center it with icon buttons. View → Toggle
Layer Boundary shows a global subtle outline that follows the active layer.
The Layer menu can align, fit it to the canvas, or crop it to its opaque
pixels. Image → Crop to Content
does the same for the canvas. Image and Layer transforms rotate and flip
by quarter turns. Rotate, Flip, and Perspective preview the active layer and
commit on Apply, leaving the selection in canvas coordinates. Perspective
corner grips keep a dark rim so they stay visible on a white image. The Scale
Layer tool draws a frame around the active layer and scales it from a dragged
border or corner; release commits one undo step, and its Width/Height options
are read-only for now, and the tool uses Shift+S. Escape or a layer/tab change
cancels the scale drag. Clone Stamp uses Shift+C,
sets its source with Alt-click, and copies from that offset while painting. Pencil,
Brush, Eraser, Blur, and Sharpen paint the active layer inside the selection; a
stroke commits when the pointer is released. Their pointer is a size-scaled
ring, hard for Pencil and soft for the others. Blur and Sharpen change local
pixels under the brush. Blur and Sharpen have no shortcut.
The bracket keys adjust the brush size for the paint tools. Gradient fills the active layer between two colors, and
Bucket Fill can cover the connected region or the whole selection. Brightness &
Contrast, Hue & Saturation, Desaturate, and Invert preview the active layer in a
dialog and apply inside the selection. The layer's alpha stays in place. Cancel
leaves the pixels unchanged, and identity settings do not create a history
entry. Color Balance, Levels, and Curves use that dialog. Levels and Curves
share a per-channel lookup table, and their histogram skips transparent pixels.
Blur, Sharpen, and Drop Shadow preview the active layer in a dialog. Blur and
Sharpen follow the selection and mix color and alpha. Drop Shadow adds a layer
beneath the source from its blurred silhouette. A zero radius or amount does
not create a history entry. Path (`P`) edits cubic anchors in canvas
coordinates. Click adds an anchor, a drag pulls its handles, and clicking the
first anchor closes the path. Path to Selection fills every closed path with
nonzero winding. Selection to Path creates closed paths from the current
selection. Selection ants paint above path outlines so an overlapping path
cannot hide the selection; path handles remain above both. Image transforms,
scale, canvas size, and crop move those paths with the image. A Paths tab beside Layers lists every path, selects one, toggles its
visibility, or deletes it. Escape or switching tools stops the
current path, and the next Path click starts a new one. Stroke in the Paths tab
paints the selected visible path onto the active layer with the foreground
color, using a Pencil or Brush stroke and an adjustable size. Stroke Selection
remains on the Edit menu. The contour stays in canvas coordinates, and the
brush is not clipped to it. Text (`T`) edits text in place with a caret.
A click on empty canvas starts new text, a click on a visible text layer opens
it, and a click inside the current text moves the caret. A click within
a few screen points of a text box still edits that text. A dashed frame marks
the text being edited, and the caret blinks. Typing, Return, the
arrow keys, and the text clipboard edit the draft and show it on the canvas at
once. Native Edit menu actions and Undo/Redo step through the typing first,
then fall through to image history. There is no Apply or Discard: Escape,
Cmd/Ctrl-Enter, a click outside the text, or changing tools, layers, tabs, or
any image command commits a modified draft as one undo step, and a command
that depends on the result resolves its active layer after that commit lands.
If rendering or the layer budget rejects a commit, its draft stays available
on its original tab and Save/Export does not continue. An empty new draft
just closes. Save/Export commit typed text first and still require other
visible previews to be applied or discarded; Close/Quit count pending edits as
unsaved work. The text options pick the font from the installed families, Bold
and Italic independently, the size, letter spacing in pixels, line spacing as a
percent of the size, and the text color; new text inherits the last style and
the foreground color. The
font list scrolls independently of Tool Options and shows an English Aa sample
in each font. Opening the font box focuses a case-insensitive name filter;
typing narrows the list, arrows browse it, Return chooses, and Escape closes
it. Query typing, clipboard actions, and undo/redo stay in the filter and do not
edit the canvas text. The layer stores the source, one font with bold/italic flags, a
pixel size, letter and line spacing, and a color, plus the raster used for
display and export. Opening a draft preserves those
pixels until its source or style changes. Preview jobs coalesce changes and
preserve layer opacity and stack order. A missing named font shows a substitution
notice. Rasterize Text Layer drops the source
and keeps the pixels. Pixel tools stay disabled until then. The Image menu
can scale the canvas, resize it from a nine-point anchor,
or switch between RGB and grayscale. Copy and Cut use the active layer inside the
selection, Paste adds a centered layer, and Open as Layers imports images
the same way. Each tab can undo and redo crops, fills, selections and
layer changes, path edits, strokes, text edits, and rasterizing text. Selection changes preserve the saved revision, and undoing
back to the saved state clears the unsaved marker.
Tabs retain zoom and pan. The canvas
paints 256 px tiles and builds a coarser level when the image is fitted or
zoomed out. Transparent pixels show a checker on the canvas and in each layer
thumbnail. A thumbnail keeps the layer's aspect ratio, and its finer checker
fills only that area rather than the square row slot. It also supports a grid
and center guides. Save and Save As handle
Lime (`.lime`) projects (legacy `.imageedit` files remain readable),
PNG, JPEG, and WebP. A project stores the layer stack, paths, and text with lossless pixels.
Export writes flattened PNG, JPEG, or WebP copies to a separate destination
and preserves the tab’s saved state. Raster Save is offered only while a
document is a single plain layer without paths or editable text.

## Code map

- `src/main.rs` starts GPUI, creates or reopens the main window, and routes
  launch arguments and macOS file URLs to the workspace.
- `src/commands.rs` defines actions, shortcuts, native menus, and toolbox
  metadata. Toolbox buttons show the full label, trim it when the sidebar is
  narrow, and add the shortcut and a tooltip; menus keep the full label. Folded
  buttons keep the icon and tooltip but drop the label. Pencil and Brush have
  no key. Hold Space to pan without leaving the current tool. Reuse those
  actions for UI entry points. A `Planned` menu action stays disabled until
  there is a real handler.
- `src/document.rs` decodes and saves raster files and prepares the
  full-resolution composite, tile images and bounded BGRA preview for GPUI.
- `src/paint.rs` spaces pencil, brush, eraser, blur, sharpen, and clone-stamp
  dabs along the pointer path and stores touched 64 px tiles. It also samples linear and radial gradients
  and strokes a polyline in one pass, skipping a repeated endpoint.
  `src/workspace/paint_ui.rs` composites the live tiles over the existing
  preview. Release commits their dirty regions in one undo step.
  `src/workspace/gradient_ui.rs` previews a gradient until Apply.
- `src/workspace/scale_ui.rs` owns the Scale Layer frame, handle hit testing,
  the live GPU-scaled preview, and the commit that resamples the layer.
- `src/adjustments.rs` changes brightness, contrast, hue, saturation, and
  lightness, and it desaturates or inverts. It also balances color and maps
  levels and curves through a 256-entry lookup table per channel.
  `src/workspace/adjust_ui.rs` shows the reduced preview for the latest slider
  position. Apply writes the active layer in the background.
- `src/filters.rs` blurs and sharpens the active layer and builds a drop shadow
  from its alpha. Blur uses a bounded row buffer, and soft selection edges blend
  premultiplied color. `src/workspace/filter_ui.rs` shows the reduced preview for
  the latest slider position. Apply writes the layer, or inserts the shadow
  beneath it, in the background.
- `src/paths.rs` stores cubic paths in canvas coordinates, flattens them, fills
  closed paths with nonzero winding, and builds path outlines from simple and
  mask selections. `src/workspace/path_ui.rs` edits anchors and handles and
  converts between paths and selections. A drag commits when the pointer is
  released.
  `src/workspace/stroke_ui.rs` strokes the active path or the selection contour.
- `src/preview.rs` owns the 256 px tile cache, the one-pixel sampling apron
  and the Fit level choice. The canvas paints those tiles.
- `src/selection.rs` is the canvas-space selection: unrestricted, empty, full,
a rectangle, an ellipse, a rounded rectangle, the inverse of a rectangle, or a
canvas-bounded coverage mask. `src/workspace/selection.rs` drags the rectangle
or oval, resizes it from its handles, moves it from inside, and clears it on a
click outside. Oval Select has a Circle option. Rectangle Select has a corner
radius control with minus and plus buttons, and the Width/Height fields edit
the current size with live preview. Empty numeric text keeps the selection
unchanged. Cropping or shrinking the canvas preserves curved selection coverage;
a partially clipped curve becomes a bounded mask. Scaling a rounded selection
scales its coverage on both axes as a mask. Free Select drags a freehand
segment or click-adds polygon vertices, and previews the next edge. Releasing a
drag keeps the draft open as a checkpoint, and the start node or Return closes
it. The Magic Wand shares the bucket tolerance.
Fill, Clear, and the clipboard honor feathered coverage. Shift adds, Control
subtracts, and Shift-Control intersects. Over the box or ellipse the painted
crosshair gives way to a hand or resize cursor. The outline is a dashed
contour. Stroke polylines follow that contour, including a hole. Mask
transforms avoid per-pixel coordinate lists; morphology uses running sums.
- `src/transform.rs` turns, mirrors, scales, and anchors layer pixels, and
  warps a layer by an angle or a perspective quad. Grayscale keeps alpha and
  is stored in the version 6 project.
- `src/geometry.rs` centers a layer, finds nonzero-alpha bounds, and copies
  the on-canvas part of a layer into a canvas-sized image.
- `src/document/content.rs` owns the canvas dimensions, raster layer stack, and paths.
  It handles crop, layer-local bucket fill, layer offsets, content crops, and outline strokes.
  `src/document/composite.rs` blends visible layers for display, sampling, and
  raster output.
- `src/text.rs` rasterizes a layer's text with cosmic-text. One style covers
  the whole layer. The cached bitmap is what the canvas and export paint, and
  the project also stores the source so a machine with different fonts can
  still open the pixels. It also builds a caret map with a stop at every
  character boundary for drawing the caret, hit testing, and Up/Down.
  `src/workspace/text_ui.rs` edits that draft, commits it when the session
  ends, and draws the font picker and style options.
- `src/project.rs` reads and writes the version 6 project format and applies
  the same size limits in both directions. Lime has not shipped, so older
  project versions are not supported. `fixtures/projects/` holds a golden
  project that the reader must keep opening. Regenerate it only when the
  format intentionally changes. The current fixture is version 6
  (`v6-two-layers.imageedit`); keep its legacy extension to exercise compatibility. New projects use `.lime`; the file magic
  and version stay unchanged.
  Text bold/italic flags are optional in version 6 and default to regular for
  projects written before those controls were added.
- `src/workspace.rs` owns tabs, view state, and background open, edit, undo
  and redo jobs. A job publishes only if its tab's edit generation is
  unchanged. A coarser preview level publishes only while the composite it
  was built from is still current. The canonical path identifies an already
  open file.
- `src/workspace/history.rs` holds each tab's undo and redo snapshots, its
  current and saved revisions, and the entry and memory limits. Snapshots
  share layer pixels, and each shared buffer counts once toward the limits.
  Owned path and text metadata counts separately for each snapshot.
- `src/workspace/clipboard.rs` copies the selected part of the active layer,
  pastes a centered layer, and imports rasters above the active layer. Deferred
  operations validate the document generation and layer; each imported image
  must fit the stack budget before the next one is decoded.
- `src/workspace/files.rs` handles native file dialogs, Save routing,
  background saving and export, and unsaved-change confirmation.
- `src/workspace/layers.rs` owns the Layers panel, layer commands, opacity
  drag coalescing, and the Move preview. Layer thumbnails keep the layer's
  aspect ratio and draw their checker only inside that area. The preview is
  display-only until the pointer is released. Move's crosshair has a four-arrow badge, and
  its tool options align the layer to canvas edges or center it with icon
  buttons. The Layer menu has the fit and content-crop commands.
  `src/workspace/dialogs.rs` renders the canvas dimensions, color, JPEG quality,
  layer, adjustment, and filter dialogs. `src/workspace/color.rs`
  handles the HSV wheel and brightness slider.
- `src/workspace/numeric.rs` edits the selection radius, selection size, crop
  fields, shared wand/fill tolerance, brush size/spacing/hardness/opacity, path
  stroke size, and text size/letter/line spacing inline with a caret and text
  selection. Values preview live; selection edits commit one history entry.
  Empty text and Escape restore the value from before editing. Letter spacing
  accepts negative values. Scale Layer's size readouts remain read-only.
- `src/workspace/canvas_tools.rs` maps canvas gestures to full-resolution
  pixels, including creating, moving, and resizing the crop box. The Crop tool
  options edit X, Y, Width, and Height live.
  `src/workspace/view.rs` lays out the shell, including the resizable
  and foldable toolbox and its full-label or icon-only buttons, and paints the
  canvas using the same geometry. Tool options group related controls under
  section labels: segmented choices for either/or settings, check toggles for
  booleans, aligned steppers and readouts for numbers, and full-width actions.
  Explanatory paragraphs stay out of Tool Options. Each tool has a help icon
  with essential gestures and shortcuts from `Tool::help`; the Paths panel has
  the same help. Warnings, progress, and editing readouts remain visible.
  `src/workspace/tooltip.rs` renders shared hover tooltips and help icons.
  Modifier-change events are handled on the workspace focus path so Clone
  Stamp's source cursor repaints on Option/Alt press and release without a
  pointer move or a polling timer.
- `src/windows_menu.rs` converts the shared GPUI menu tree into a `muda`
  native Windows menu bar, attaches it to the Win32 HWND, keeps native owners
  and item handles on the UI thread, syncs action availability after workspace
  renders and menu opening (without idle frame polling), and forwards activations
  to GPUI actions after Win32 callbacks return. A native subclass routes Alt
  mnemonics and F10 before GPUI's text handling. Windows uses GPUI's HWND DirectX
  swap-chain path so native menu painting has an opaque backing surface.
  `src/windows_menu/appearance.rs` requests dark native menus. Its dynamically
  loaded theme APIs are version-checked; unavailable support and high-contrast
  mode retain system rendering. `src/windows_menu/drawing.rs` uses Win32 owner
  drawing to match `src/theme.rs` for the menu bar, popup backgrounds, highlights,
  separators, text, and disabled icons. Windows still owns navigation, submenu
  placement, and activation. `src/windows_menu/drawing/popup.rs` finishes native
  popup borders and submenu chevrons after Windows paints them. Its per-popup
  subclasses are limited to our menu drawing DC and owner thread, coalesce
  selection redraws without polling, and detach for system rendering/teardown.
  Drawing data exposes MSAAMENUINFO names to assistive
  technology. Preserve original native styles/data/bitmaps for fallback and
  teardown, and release adapter/drawing borrows before synchronous native redraws.
  `src/menu_icons.rs` selects icons by action and rasterizes the toolbox SVGs for
  native menus. Icon columns align plain entries with illustrated entries.
  macOS uses 2x template images so AppKit owns their appearance and tint.
  `.github/workflows/windows.yml` builds and tests the x64 MSVC release ZIP,
  setting `GPUI_FXC_PATH` from the installed Windows SDK.
- `src/icons.rs` contains tool glyphs. `macos/AppIcon.svg` is the editable
  app icon and `macos/AppIcon.icns` is its packaged form.
  `windows/AppIcon.ico` contains the same original blue artwork in nine sizes.
  `build.rs` embeds `windows/app.rc` in native Windows builds as icon resource 1,
  which GPUI loads for its window class. Keep GPUI's manifest separate and enabled.
  The Windows workflow verifies the icon group and manifest before packaging.
- `src/macos.rs` applies the same icon to the running Dock entry, including
  development launches from Cargo.
- `scripts/bundle-macos.sh` copies the executable, Info.plist, and icon into
  `dist/Lime.app`, ad hoc signs the bundle, checks that signature and the
  project and raster document types, and rejects a Cargo version that differs
  from `CFBundleShortVersionString`. `scripts/package-macos.sh` builds the app
  and creates the drag-to-Applications `dist/Lime-<version>.dmg`.

## Build and check

From the repository root, use a recent Rust toolchain and Xcode. Run
`cargo fmt --check`, `cargo clippy --locked -- -D warnings`, and
`cargo test --locked`. Keep `Cargo.lock` tracked. GPUI on macOS uses
`font-kit` for text and compiles its Metal shaders at runtime.

Keep the package version in `Cargo.toml` and
`CFBundleShortVersionString` in `macos/Info.plist` in sync; a test checks both.

Run `./scripts/bundle-macos.sh` and open `dist/Lime.app` for native smoke
checks. `./scripts/package-macos.sh` additionally creates and verifies
`dist/Lime-<version>.dmg` with an Applications shortcut. Test a valid image and
an unsupported file, switch and close tabs, and
try Finder's **Open With** after rebuilding packaging changes. For canvas work,
test fit, fixed zoom, pan at a zoom that clips the image, and sampling after
panning. Create an image with custom dimensions, fill and crop it, then save
and reopen a project and a raster image. Confirm menu availability matches the
active document, layer, and edit state.

The preview and stroke benchmarks are ignored by default. Run them explicitly
with `cargo test --release --locked benchmark -- --ignored --nocapture` when
changing rendering or stroke performance. Benchmark fixtures are generated in
memory; they are not disposable project fixtures.

Keep local build output in `target/` and `dist/`. Use temporary directories
for test images. Keep machine-specific settings and credentials out of source.
`cargo clean` removes the rebuildable Rust output without deleting the current
`dist/Lime.app` or DMG. Keep the golden project fixture and editable/packaged
app icons; they are source assets, not generated leftovers.
