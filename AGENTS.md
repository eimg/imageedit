# ImageEdit agent guide

ImageEdit is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for Windows. Hide and Quit stay in the macOS
app menu. Windows uses File → Exit. The product
takes cues from GIMP, with native menus, a docked, resizable, and foldable
toolbox, a Layers panel, and a tabbed canvas. File commands belong in the menu.
The workspace favors canvas space and has no top toolbar.

For planned work, start with [FEEDBACK.md](FEEDBACK.md) and take the first open
item only. Leave the rest for another agent. The implementation plan and the
latest pass handoff are in [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md).
The plan is guidance, not a requirement to preserve choices that hurt the
overall editing experience.

## Current product scope

ImageEdit supports layered raster editing. The empty workspace can open a
file or create an image. Crop that image: drag the box, move it from inside,
resize it from its handles, and read X, Y, Width, and Height in the Crop
options. Inline numeric entry is temporarily disabled. Sample or set swatches,
and fill connected regions on the active layer. Rectangle, oval, circle,
freehand or polygon, and wand selections limit fill and Clear to Transparency,
and Crop to Selection uses the selection bounds. Rectangle Select and Oval Select
are separate tools. Oval Select has a Circle option for an exact circle, and
Rectangle Select has a corner radius control with minus and plus buttons.
Width/Height fields show the current size.
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
are read-only for now. Pencil,
Brush, Eraser, Blur, and Sharpen paint the active layer inside the selection; a
stroke commits when the pointer is released. Their pointer is a size-scaled
ring, hard for Pencil and soft for the others. Blur and Sharpen change local
pixels under the brush and have no keyboard shortcut. Gradient fills the active layer between two colors, and
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
nonzero winding. Image transforms, scale, canvas size, and crop move those
paths with the image. Stroke (`S`) paints the active path or the selection
contour with the brush. The contour stays in canvas coordinates and lands on
the active layer. Return commits one undo step. The selection stays put, and
the brush is not clipped to it. Text (`T`) adds an editable layer.
Click sets its origin, or loads the active text layer. Typing, Return, and the
text clipboard stay in that draft until Apply commits one undo step. Native
Edit menu actions and Undo/Redo operate on the draft. Escape discards it.
Modified text must be applied or discarded before changing tools, layers, tabs,
or other content. Save/Export require visible edits to be applied or discarded;
Close/Quit count pending edits as unsaved work. The layer stores the source, one font, a pixel size, and a color,
plus the raster used for display and export. Opening a draft preserves those
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
thumbnail. It also supports a grid and center guides. Save and Save As handle
ImageEdit projects,
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
- `src/paint.rs` spaces pencil, brush, eraser, blur, and sharpen dabs along the
  pointer path and stores touched 64 px tiles. It also samples linear and radial gradients
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
- `src/paths.rs` stores cubic paths in canvas coordinates, flattens them, and
  fills closed paths with nonzero winding. `src/workspace/path_ui.rs` edits
  anchors and handles. A drag commits when the pointer is released.
  `src/workspace/stroke_ui.rs` strokes the active path or the selection contour.
- `src/preview.rs` owns the 256 px tile cache, the one-pixel sampling apron
  and the Fit level choice. The canvas paints those tiles.
- `src/selection.rs` is the canvas-space selection: unrestricted, empty, full,
a rectangle, an ellipse, a rounded rectangle, the inverse of a rectangle, or a
canvas-bounded coverage mask. `src/workspace/selection.rs` drags the rectangle
or oval, resizes it from its handles, moves it from inside, and clears it on a
click outside. Oval Select has a Circle option. Rectangle Select has a corner
radius control with minus and plus buttons, and the Width/Height fields show
the current size. Inline numeric entry is paused. Free Select drags a freehand
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
  is stored in the version 5 project.
- `src/geometry.rs` centers a layer, finds nonzero-alpha bounds, and copies
  the on-canvas part of a layer into a canvas-sized image.
- `src/document/content.rs` owns the canvas dimensions, raster layer stack, and paths.
  It handles crop, layer-local bucket fill, layer offsets, content crops, and outline strokes.
  `src/document/composite.rs` blends visible layers for display, sampling, and
  raster output.
- `src/text.rs` rasterizes a layer's text with cosmic-text. One style covers
  the whole layer. The cached bitmap is what the canvas and export paint, and
  the project also stores the source so a machine with different fonts can
  still open the pixels. `src/workspace/text_ui.rs` edits that draft.
- `src/project.rs` reads and writes the version 5 project format and applies
  the same size limits in both directions. ImageEdit has not shipped, so older
  project versions are not supported. `fixtures/projects/` holds a golden
  project that the reader must keep opening. Regenerate it only when the
  format intentionally changes. The current fixture is version 5.
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
  drag coalescing, and the Move preview. The preview is display-only until
  the pointer is released. Move's crosshair has a four-arrow badge, and
  its tool options align the layer to canvas edges or center it with icon
  buttons. The Layer menu has the fit and content-crop commands.
  `src/workspace/dialogs.rs` renders the canvas dimensions, color, JPEG quality,
  layer, adjustment, and filter dialogs. `src/workspace/color.rs`
  handles the HSV wheel and brightness slider.
- `src/workspace/canvas_tools.rs` maps canvas gestures to full-resolution
  pixels, including creating, moving, and resizing the crop box. The Crop tool
  options show X, Y, Width, and Height values; inline numeric entry is paused.
  `src/workspace/view.rs` lays out the shell, including the resizable
  and foldable toolbox and its full-label or icon-only buttons, and paints the
  canvas using the same geometry. `src/workspace/tooltip.rs` renders shared
  hover tooltips.
- `src/icons.rs` contains tool glyphs. `macos/AppIcon.svg` is the editable
  app icon and `macos/AppIcon.icns` is its packaged form.
- `src/macos.rs` applies the same icon to the running Dock entry, including
  development launches from Cargo.
- `scripts/bundle-macos.sh` copies the executable, Info.plist, and icon into
  `dist/ImageEdit.app`, ad hoc signs the bundle, and checks that signature and
  the project and raster document types.

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
