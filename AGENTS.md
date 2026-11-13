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
unchanged.
Crop has Free, 1:1, 4:3, 16:9, Image, and custom integer aspect ratios. A fixed
ratio constrains creation, handle resize, and numeric dimension edits; choosing
a ratio alone leaves the current box unchanged. Sample or set swatches,
and fill connected regions on the active layer. Rectangle, oval, circle,
freehand or polygon, and wand selections limit fill and Clear to Transparency,
and Crop to Selection uses the selection bounds. Rectangle Select and Oval Select
are separate tools. Oval Select has a Circle option for an exact circle, and
Rectangle Select has a corner radius control with minus and plus buttons, and
the X/Y and Width/Height fields accept typed values for editable geometric
selections. Empty text or Escape restores the numeric edit. Selection and Crop
share ratio geometry from `src/constraints.rs`: dimensions round to pixels,
resizes stay bounded, side handles center the untouched axis, and corner handles
keep their opposite corner. Fixed-ratio handles stop at minimum size rather than
crossing their opposite edge. Circle overrides the oval ratio. Rectangle and
oval tools remember a shared ratio during the session; Crop has a separate one.
Antialias edges defaults off and smooths oval/circle and rounded-rectangle
coverage with a 4×4 subpixel sample, using fast interior/exterior checks. It
updates a current editable curve in one selection-only undo step. Masks retain
their coverage when the preference changes. Move, resize, crop, canvas resize,
image transforms, and history preserve curve quality; clipped curves become
coverage masks.
Rectangle and oval drags may start or extend outside the canvas; coverage stays
bounded to canvas pixels and clipped curves retain their original geometry.
Shift adds to the selection, Control subtracts, and Shift-Control intersects. The wand and
bucket share a tolerance; zero matches a color exactly. The wand can sample the
active layer or the visible composite, independently of Bucket Fill's setting.
Both region tools ignore stored RGB when both compared pixels are fully
transparent; other channel and alpha comparisons keep the shared tolerance.
Feather, Grow, and Shrink edit the mask; Grow/Shrink preserve soft coverage.
Free Select has an independent, default-off antialias preference for new closed
freehand/polygon selections. It uses 4×4 subpixel nonzero-winding coverage;
existing paths retain hard rasterization. Rectangle/Oval/Free Select and Wand
share a default-off Feather new selections preference and a 0–128 image-pixel
radius (initially 8). These settings edit preferences only, not the current mask
or history. Creation feathers only the incoming region before combination;
moving/resizing existing geometry does not feather it again. Work runs in the
selection background job on completion, never on each pointer update. Feathered
results use the existing coverage-mask representation and its editing limits.
Feather uses a fixed square averaging kernel with zero coverage outside the
canvas, including for bounded Full selections; Unrestricted stays unrestricted.
Move repositions the active layer, and its tool options
align it to any canvas edge or center it with icon buttons. View → Toggle
Layer Boundary shows a global subtle outline that follows the active layer.
Move's signed X/Y fields preview a position and commit on Return or leaving the
field; empty/invalid input and Escape restore the position. Arrow keys nudge
one image pixel, or ten with Shift, independent of zoom. Release commits one
undo step per key sequence; Escape, deactivation, or changing tools/tabs/layers
cancels an unfinished sequence. Nudges queued during their own prior canvas
commit resume only on the same tab, layer, and document generation.
Layers have independent alpha and position locks, exposed as visible
icon toggles beside each row's visibility button and through Layer menu actions.
Lock toggles share visibility's background-free resting style and hover treatment;
closed locks use the accent color, including while hovered, while open locks are dimmed. Position lock
uses a plain padlock; alpha lock retains its transparency indicator.
Row toggles target stable tab/layer IDs without changing the active layer; typed
geometry and text commit first, with deferred actions validating their context.
Alpha lock is disabled on editable text rows with an explanatory tooltip.
Lock changes use `Dirty::Unpainted`, commit one image
history step, preserve shared pixel/preview buffers, and require project Save.
Alpha lock is available for raster layers: paint/clone/outline strokes use
source-atop blending; fills and gradients use the same color mix; filters preserve
the original alpha before selection blending. Fully transparent pixels keep
their hidden RGB. Eraser/Cut/Clear cannot remove locked pixels. Adjustment alpha
already stays fixed, and locked transparent samples are skipped. Blur/sharpen
recover weighted colors before rounding faint alpha. Stroke preview/commit reject
a stale lock policy. Duplicates retain both flags; new/imported/shadow layers
start unlocked. Merge Down requires both participating layers unlocked.
Position lock blocks manual layer moves (pointer, typed, and nudges), alignment,
pixel transforms, scale, fit, and layer content crop. Image-wide operations still
transform every layer and retain its locks. Editable text remains editable at its
existing layout anchor, including automatic bitmap resizing; rasterize it before
locking alpha. Opacity, visibility, order, and deletion remain available.
The Layer menu can align, fit it to the canvas, or crop it to its opaque
pixels. Image → Crop to Content
does the same for the canvas. Image and Layer transforms rotate and flip
by quarter turns. Rotate, Flip, and Perspective preview the active layer and
commit on Apply, leaving the selection in canvas coordinates. Rotate also accepts
typed angles in degrees with tenths precision; Shift-drag snaps to absolute
15-degree angles, including when Shift changes without moving the pointer. Perspective
corner grips keep a dark rim so they stay visible on a white image. The Scale
Layer tool draws a frame around the active layer and scales it from a dragged
border or corner; release commits one undo step, and the tool uses Shift+S.
Width/Height edits preview a centered resize and commit on Return or leaving
the field; empty text and Escape restore the original size. Aspect locking starts
on and applies to numeric edits and handles; Shift reverses it during a drag.
Nearest, Bilinear, and Bicubic choose the commit resampling. Escape or a
layer/tab change cancels the pointer drag. Clone Stamp uses Shift+C,
sets its source with Alt-click inside the visible raster layer, or anywhere on
the canvas with Sample All Layers. Merged sampling uses an immutable reference
to the current full-resolution composite at stroke start; default sampling uses
the destination layer's original pixels. The visible composite includes layer
opacity and rendered text, while only the active visible raster layer is painted.
Source coordinates are mapped through the destination layer offset, independently
of destination selection coverage and alpha lock. Sampling changes clear the
source/alignment; source scope includes tab/layer, layer offset/dimensions, canvas
dimensions, and sampling mode. Show Source starts on and draws the sampled pixel
center and brush footprint using the existing cursor mouse/refresh path; it
tracks live/pending/settled aligned offsets and resets between unaligned strokes.
The marker follows zoom/pan, clips to the canvas, and hides if its sample center
is unavailable. Alt retains its source-picking cursor. Aligned starts
on and preserves the source offset across strokes; off restarts each stroke at
the chosen source. Changing alignment resets its offset. Tab/layer changes clear
the source; changed layer/canvas geometry requires a fresh source. The first successful
stroke establishes alignment; canceled/failed strokes leave it unchanged. Pencil,
Brush, Eraser, Blur, and Sharpen paint the active layer inside the selection; a
stroke commits when the pointer is released. Their pointer is a size-scaled
ring, hard for Pencil and soft for the others. Blur and Sharpen change local
pixels under the brush. Blur and Sharpen have no shortcut.
The paint and retouch tools retain independent brush settings during the session;
path and selection outlines share their own settings. Opacity limits each stroke
relative to its starting pixels. A 16-bit coverage buffer per touched tile keeps
overlapping dabs from exceeding that limit and applies soft selection coverage
once. Buildup defaults off; when on, Flow (0–100) accumulates dab coverage toward
the opacity limit. Flow and buildup are remembered per tool during the session.
Blur/Sharpen label opacity Strength and blend the original stroke pixels toward
a fixed local filtered result; a new stroke applies another pass. Outline strokes
use the same coverage cap. Zero opacity, buildup with zero flow, and unchanged targets publish no
pixel changes; sub-byte buildup accumulates without changing hidden RGB until
alpha becomes visible. The bracket keys adjust
the brush size for the paint tools. Gradient fills the active layer using dynamic
foreground/background colors or up to eight custom RGBA stops. Reverse changes
the color direction without moving linear/radial geometry. Custom positions use
0–1000 units, with fixed endpoints and ordered, distinct interior stops. The
editor exposes RGB picking, alpha and position percentages, and Add/Remove Stop;
adding splits the largest gap with its current interpolated color. Colors
interpolate in premultiplied alpha so transparent hidden RGB cannot tint edges.
Six bounded, deduplicated session presets capture literal colors, shape, and
Reverse; loading preserves canvas endpoints and initializes Custom mode. They
are not project data or a persistent gradient library. Settings/preset edits
refresh the coalesced preview without image history. Apply uses the same sampler,
selection coverage, color-mode conversion, and alpha-lock policy. Color picker
targets carry a palette revision and stop index; stale targets cannot recolor
another palette, and RGB changes preserve stop alpha. Percentage editing stores
the exact alpha byte for restoration and accepts decimal input.
Bucket Fill can cover the connected region or the whole selection. Brightness &
Contrast, Hue & Saturation, Desaturate, and Invert preview the active layer's
effect in the canvas composition inside a dialog, and apply inside the selection.
The preview includes offsets, layer opacity/order, and soft selection coverage;
a checker displays transparency. Before/After switches between the original
composition and the chosen effect without changing parameters. Apply still uses
the chosen parameters while Before is displayed. Reset restores built-in defaults
and switches to After; fixed operations such as Invert retain their operation.
The layer's alpha stays in place. Cancel
leaves the pixels unchanged, and identity settings do not create a history
entry. Color Balance, Levels, and Curves use that dialog. Levels and Curves
share a per-channel lookup table, and their histogram skips transparent pixels.
Blur, Sharpen, and Drop Shadow use the same composition preview controls. Blur and
Sharpen follow the selection and mix color and alpha. Drop Shadow adds a layer
beneath the source from its blurred silhouette. A zero radius or amount does
not create a history entry. Each adjustment/filter remembers successful parameters
for the workspace session, shared across tabs; cancellation, failed/stale jobs,
and pixel no-ops leave prior settings intact. Filters → Repeat Last Effect
(Cmd/Ctrl+F) applies the last successful adjustment/filter to the current visible
raster layer and current selection as one undoable edit. Reopen Last Effect
(Cmd/Ctrl+Shift+F) opens its remembered parameters with preview. Typed geometry
and text resolve first; unfinished canvas previews disable these commands.
Shadow repeat preflights the current destination and keeps the source active.
Its dialog exposes the remembered color and Use Foreground; Reset restores
built-in parameters and the current foreground color. Memory is not project data
and is not undone with the document. Path (`P`) edits cubic anchors in canvas
coordinates. Click adds an anchor, a drag pulls its handles, and clicking the
first anchor closes the path. Alt-clicking a visible curve inserts an anchor by exact cubic
subdivision, including closing segments; existing anchor/handle hits take priority.
The inserted point can be dragged within that same one-step gesture. Hit testing
uses an eight-screen-point radius, bounded subdivision, and chord-projection/
Newton refinement, including stationary collinear parameterizations. The selected
anchor has a larger outlined marker. Corner collapses handles; Smooth aligns
them and preserves existing lengths, initializing missing handles from neighbors.
Independent (default), Aligned, and Symmetric are session handle-drag preferences;
they preserve the opposite handle, its length, or mirror its length respectively.
Newly placed handles retain symmetric creation. Linked drags use the baseline
anchor each frame to avoid drift. A click without movement does not couple
handles; repeated identity shaping creates no history. Geometry, not new anchor
mode metadata, persists in the existing project format. Multiple anchor selection
remains planned. Path to Selection fills every closed path with
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
once. Dragging, Shift-click, or Shift+arrows select partial ranges across lines.
Copy/Cut use the range and typing/Paste replace it. Option+Left/Right and
Option+Backspace/Delete on macOS (Ctrl on Windows) navigate/delete by Unicode
word boundaries; Shift extends those selections. Normal moves/deletes use
graphemes, preserving combining characters and emoji clusters. Home/End move
within a line, Windows Ctrl+Home/End and macOS Cmd+Up/Down move through the
document, and macOS Cmd+Left/Right move to line edges. Typing history retains
selection anchors. Pointer selection uses the latest matching caret map;
late layout may resolve a released drag, but edits/navigation invalidate its
old pending hit. Deactivation ends a pointer drag. Modified navigation in the
font filter remains in that filter and cannot affect canvas text. Styles still
apply to the whole text layer. Native Edit menu actions and Undo/Redo step through
typing first, then fall through to image history. There is no Apply or Discard: Escape,
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
the same way. File → Open also imports PDFs through a dialog that selects all
pages or a range, sets a resolution from 1 to 1200 DPI, and renders pages as
layers on the first page's canvas or as separate tabs. PDF imports use the
normal image and stack budgets and become unsaved `.lime` projects; PDF is not a
save or export format. Encrypted PDFs are unsupported, and Open as Layers accepts
one PDF at a time. PDF read/inspection/render/preparation jobs show progress and
accept Cancel or Escape. Cancellation is cooperative: an in-progress parser,
page render, or image-preparation call finishes before cancellation settles;
its results are discarded. New documents and Open as Layers publish no partial
prefix. PDF bytes are retained from inspection through rendering, independent
of later source-file replacement/deletion. Open as Layers preflights the entire
range against remaining layer count, IDs, and stack bytes before rendering and
publishes all selected layers as one undo step. Each tab can undo and redo crops, fills, selections and
layer changes, path edits, strokes, text edits, and rasterizing text. Selection changes preserve the saved revision, and undoing
back to the saved state clears the unsaved marker.
Tabs retain zoom and pan. The canvas
paints 256 px tiles and builds a coarser level when the image is fitted or
zoomed out. Transparent pixels show a checker on the canvas and in each layer
thumbnail. A thumbnail keeps the layer's aspect ratio, and its finer checker
fills only that area rather than the square row slot. It also supports a grid
and user-placed guides with pixel rulers. View → Toggle Rulers and Guides shows
both rulers and guides. Move drags a horizontal guide from the top ruler or a
vertical guide from the left; existing guides can be repositioned, and releasing
outside the canvas workspace (including onto a ruler) removes them. Escape,
deactivation, or a tab/layer/tool change cancels the drag. Guides use canvas pixel
boundaries, follow image transforms/crop/resize, and have undo/redo and project
persistence. The dotted grid is image-anchored, defaults to 16 image pixels, and
uses solid dots with a pale rim for contrast on light/dark pixels. It thins dots
below eight screen points of spacing at low zoom. Move's independent Snap to Grid/Guides toggles
start off; they work even when overlays are hidden, align layer edges/centers
within six screen points, and Control bypasses snapping and guide grabs. Exact
numeric positions and arrow nudges bypass snapping. Grid spacing (1–4096 px) and
overlay/snap preferences are session settings. Save and Save As handle
Lime (`.lime`) projects (legacy `.imageedit` files remain readable),
PNG, JPEG, and WebP. A project stores the layer stack, paths, guides, and text with lossless pixels.
Export writes flattened PNG, JPEG, or WebP copies to a separate destination
and preserves the tab’s saved state. Raster Save is offered only while a
document is a single plain layer without paths, guides, or editable text.

## Code map

- `src/updates.rs` checks the public GitHub latest stable release in a background
  job, starting after 15 seconds and then every 24 hours. A bounded per-user cache
  throttles automatic attempts across restarts, including failed checks; manual
  checks bypass the cooldown and join an existing request. SemVer precedence
  excludes drafts/prereleases and ignores build metadata. Platform/architecture
  matching uses published asset names (Apple silicon DMG, Windows x64 EXE then
  ZIP), and direct URLs must match this repository's HTTPS release paths.
  `src/updates/ui.rs` adds a status-bar badge and a separate update window with
  changelog/download links, current/checking/offline states, and keyboard access.
  Missing platform assets leave the changelog available without guessing a URL.
  The Lime menu (macOS) or Help menu (Windows) exposes Check for Updates. Nothing
  downloads or installs in the app, and update state never enters image history.
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
  Opacity and visibility edits refresh composite tiles while reusing layer
  thumbnails; hidden or fully off-canvas opacity edits need no pixel refresh.
  Reduced transparent previews and thumbnails use premultiplied resampling so
  hidden RGB values cannot tint visible edges.
- `src/paint.rs` spaces pencil, brush, eraser, blur, sharpen, and clone-stamp
  dabs along the pointer path and stores touched 64 px tiles with their 16-bit
  stroke coverage. Non-buildup uses maximum dab coverage; buildup uses Flow and
  combines coverage before applying opacity and selection once against the
  original pixels. Only changed pixel tiles invalidate the live preview.
  It also samples linear and radial gradients
  and strokes a polyline in one pass, skipping a repeated endpoint.
  `src/workspace/paint_ui.rs` composites the live tiles over the existing
  preview and owns the per-tool brush settings. Release commits dirty regions
  in one undo step. Its scoped CloneSource validates tab/layer geometry and
  establishes alignment only after the stroke result publishes successfully.
  `src/gradient.rs` owns bounded color ramps, ordered stop edits, and premultiplied
  interpolation. `src/workspace/gradient_ui.rs` shares one specification between
  the coalesced preview and Apply, owns session palettes/presets, and preserves
  geometry when loading presets. Apply/Cancel remain above the stop editor.
- `src/workspace/scale_ui.rs` owns the Scale Layer frame, handle hit testing,
  the live GPU-scaled preview, and the commit that resamples the layer. It prepares
  the preview in the background and rejects results after release or cancellation.
  Numeric dimensions use the same preview/commit path; aspect locking retains
  opposite corner anchors or centers the unhandled axis. Numeric edits keep the
  layer center. All targets respect image and stack budgets. Hidden layers must
  be shown before scaling.
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
- `src/effect_preview.rs` takes an immutable document/selection snapshot and
  composites canvas-space samples into a preview bounded to 256 px. Adjustments
  use the same prepared mapper/coverage blend as commit. Filter scratch data
  covers only the visible source region plus its full sampling halo, including
  off-canvas pixels; radius and source samples scale for a reduced preview.
  Subpixel blur/sharpen radii leave the original preview unchanged. Shadows
  sample the source region shifted by their offset and composite below the source,
  independently of selection/source-layer opacity. Full-resolution preview tests
  compare these paths to real commits; larger-image filter previews are approximate.
  `EffectImages` keeps a checker-backed Before/After pair. Replaced, canceled,
  and stale pairs release both GPUI images. Preview generation gates publication
  and supplies the dialogs' updating status. Reset uses built-in form defaults,
  independently of remembered parameters; neither comparison nor reset edits pixels
  or history.
- `src/workspace/effects.rs` shares the normal/repeated effect commit path and
  owns bounded, session-only parameter memory for the seven adjustments and three
  filters. Pending entries contain parameter values plus tab/generation/submission
  order, never source pixels or selection snapshots. Only successful publication
  records parameters; late completions from another tab cannot replace newer
  settings. Repeat resolves the current layer and selection after typed geometry/
  text commits, honors model locks, and preflights drop-shadow stack/offset limits.
  No-op/rejected/closed-tab jobs are removed without changing memory. Reopen uses
  the ordinary dialogs, histogram, preview, Apply, Reset, and Cancel paths.
- `src/paths.rs` stores cubic paths in canvas coordinates, flattens them, fills
  closed paths with nonzero winding, and builds path outlines from simple and
  mask selections. `src/workspace/path_ui.rs` edits anchors and handles and
  converts between paths and selections. A drag commits when the pointer is
  released. Cubic splitting preserves the curve and original anchor IDs, enforces
  limits, and uses finite weighted interpolation. Path rows clear stale anchor
  selection on a successful switch and refuse changes while a tab is busy.
  Handle shaping/splitting stays in path metadata, sharing pixel/preview buffers
  through undo/redo; Corner/Smooth actions use the same background commit path.
  `src/workspace/stroke_ui.rs` strokes the active path or the selection contour.
- `src/preview.rs` owns the 256 px tile cache, the one-pixel sampling apron
  and the Fit level choice. The canvas paints those tiles.
- `src/selection.rs` is the canvas-space selection: unrestricted, empty, full,
a rectangle, an ellipse, a rounded rectangle, the inverse of a rectangle, or a
canvas-bounded coverage mask. `src/workspace/selection.rs` drags the rectangle
or oval, resizes it from its handles, moves it from inside, and clears it on a
click outside. Oval Select has a Circle option. Rectangle Select has a corner
radius control with minus and plus buttons. X/Y and Width/Height fields edit
the position and size with live preview. Selection and Crop share constrained
ratio geometry in `src/constraints.rs`; each has its own session preference.
Geometric curve quality lives with the selection, including in history.
Empty numeric text restores the selection. Cropping or shrinking the canvas preserves curved selection coverage;
a partially clipped curve becomes a bounded mask. Scaling a rounded selection
scales its coverage on both axes as a mask. Free Select drags a freehand
segment or click-adds polygon vertices, and previews the next edge. Releasing a
drag keeps the draft open as a checkpoint, and the start node or Return closes
it. Optional polygon antialiasing uses four scanlines per pixel row and counts
horizontal samples into the normal byte-per-pixel mask, without a supersized
intermediate image. Hard polygon/path rasterization keeps its center-sample
scanline path. `combine_created_selection` feathers new shapes/flood results
before combining them, preserving previous coverage and one selection history
entry. The Magic Wand shares the bucket tolerance. Its optional visible-composite
source is built in the background and floods in canvas coordinates; active-layer
sampling maps through the layer offset.
Fill, Clear, and the clipboard honor feathered coverage. Shift adds, Control
subtracts, and Shift-Control intersects. Over the box or ellipse the painted
crosshair gives way to a hand or resize cursor. The outline is a dashed
contour. Stroke polylines follow that contour, including a hole. Mask
transforms avoid per-pixel coordinate lists; feathering uses running sums and
Grow/Shrink use linear-time running max/min filters to preserve soft coverage.
- `src/transform.rs` turns, mirrors, scales, and anchors layer pixels, and
  warps a layer by an angle or a perspective quad. Grayscale keeps alpha and
  is stored in the version 6 project. Smooth scaling filters exact 16-bit
  premultiplied color products to retain faint colors. Image scaling validates
  each layer against the image limit and the aggregate against the stack limit
  before allocation, and rejects a scale that would make path coordinates nonfinite.
- `src/geometry.rs` centers a layer, finds nonzero-alpha bounds, and copies
  the on-canvas part of a layer into a canvas-sized image.
- `src/document/content.rs` owns the canvas dimensions, raster layer stack, and paths.
  It handles crop, layer-local bucket fill, layer offsets, content crops, and outline strokes.
  `RasterLayer::locks` owns the persistent `LayerLocks` flags. Model mutation
  methods enforce them in addition to UI/action gates, and reject alpha-locked
  editable text. Stroke sessions capture the lock policy; fill/gradient previews
  and committed operations share it. Alpha lock does not prevent intentional
  layer geometry changes; position lock handles those. Image transforms bypass
  manual position protection and move the whole document consistently.
  `src/document/composite.rs` blends visible layers for display, sampling, and
  raster output.
- `src/text.rs` rasterizes a layer's text with cosmic-text. One style covers
  the whole layer. The cached bitmap is what the canvas and export paint, and
  the project also stores the source so a machine with different fonts can
  still open the pixels. It also builds a caret map with a stop at every
  grapheme boundary for drawing the caret, hit testing, and Up/Down, with
  per-line selection rectangles. `unicode-segmentation` was already transitive
  and is now a direct dependency for these grapheme and word boundaries.
  `src/workspace/text_ui.rs` edits that draft, commits it when the session
  ends, and draws the font picker and style options.
- `src/project.rs` reads project versions 6, 7, and 8 and applies the same size limits
  in both directions. Any layer lock requires version 8 with an optional per-layer
  `locks` object. Missing flags default to false; unlocked layers omit the object.
  Without locks, projects containing guides write version 7; without guides
  they retain version 6 and omit the guides field for compatibility with older
  builds. Version 6 manifests may not contain nonempty guides. Versions 6/7 may
  not contain active layer locks; alpha-locked text is rejected. Versions before 6
  are not supported. `fixtures/projects/` holds a golden
  project that the reader must keep opening. Regenerate it only when the
  format intentionally changes. The current fixture is version 6
  (`v6-two-layers.imageedit`); keep its legacy extension to exercise compatibility. New projects use `.lime`; the file magic
  stays unchanged; the existing version 6 fixture remains unchanged.
  `v8-layer-locks.lime` additionally covers the lock-bearing format.
  Text bold/italic flags are optional in all supported versions and default to regular for
  projects written before those controls were added.
- `src/pdf.rs` probes PDFs with `hayro`, validates every selected page against
  the image, stack, and page-count budgets, and renders at the requested DPI on
  a white background. `PdfSource` retains the immutable shared `PdfData` bytes
  and inspected dimensions; render jobs parse those same bytes without reopening
  the path. `PdfControl` checks cancellation between bounded read chunks, page
  inspection/rendering, and document preparation; renderer/parser calls themselves
  remain uninterruptible. `src/workspace/pdf_ui.rs` owns probe and render jobs and
  turns the result into new documents or layers above the active layer. Import
  results publish only while their import token and target tab are current and
  cancellation has not been requested. Dropping a job requests cancellation.
  An event stream carries stage and page progress, without idle polling.
  Probe/page-tree and rendering panics are converted into import errors. Page
  preflight uses the renderer's f32/truncated dimensions, rejects nonfinite or
  subpixel dimensions, and retains the tighter document size/memory budgets.
  Preparing new Documents and existing-layer destination patches (composite,
  tiles, preview, thumbnails) stays on the background executor. The PDF modal
  remains active through preparation, including per-page grayscale conversion
  and the final patch build. Cancellation discards the prepared candidate before
  any history publication. Ordinary PDF and raster opens share a publication queue;
  frame callbacks show them in order after modal UI, document edits, and PDF work
  settle, so neither a second PDF nor a raster tab dismisses a pending PDF form.
  Queued PDFs hold paths until their turn; only the active PDF retains a byte
  snapshot. Open as Layers accepts a single PDF and validates its target both
  before rendering and before publication. `DocumentContent::room_for_sizes`
  shares raster import admission rules without allocating destination images;
  complete ranges must fit, including available layer IDs. Peak render/preview
  working memory is not limited solely by the retained-layer pixel budget.
- `src/workspace.rs` owns tabs, view state, and background open, edit, undo
  and redo jobs. A job publishes only if its tab's edit generation is
  unchanged. A coarser preview level publishes only while the composite it
  was built from is still current. The canonical path identifies an already
  open file. PDF paths queue until their turn, then probe before the import form
  opens; raster paths decode directly. Content edits and PDF insertion share
  preview cleanup before preparing their immutable candidate.
- `src/workspace/history.rs` holds each tab's undo and redo snapshots, its
  current and saved revisions, and the entry and memory limits. Snapshots
  share layer pixels, and each shared buffer counts once toward the limits.
  Owned path and text metadata counts separately for each snapshot.
- `src/workspace/clipboard.rs` copies the selected part of the active layer,
  pastes a centered layer, and imports rasters above the active layer. Deferred
  operations validate the document generation and layer; each imported image
  must fit the stack budget before the next one is decoded. New from Clipboard
  decodes the image and prepares its canvas in the background. Open as Layers
  routes a single PDF through the PDF import form.
- `src/workspace/files.rs` handles native file dialogs, Save routing,
  background saving and export, and unsaved-change confirmation. Raster Save
  eligibility is rechecked after the native dialog, since a deferred paste can
  change the document while it is open. An unfinished free selection is a pending
  gesture for Save, Export, Close, and Quit.
- `src/workspace/layers.rs` owns the Layers panel, layer commands, opacity
  drag coalescing, and Move previews for pointer, numeric, and keyboard input.
  Only pointer sessions consume mouse moves/releases; numeric sessions finish
  through the inline editor, and keyboard sessions finish on arrow release.
  Pending keyboard deltas validate the source commit and current context before
  resuming, so repeat events are not dropped during that commit. Move commits
  reuse thumbnails and refresh only the composite at the old/new visible bounds.
  A released opacity value stays visible
  in its tab until the background canvas patch settles, including across tab
  switches; each drag commits one undo step. Workspace mouse events track the
  opacity drag outside the slider and sample its final release position.
  Pending text commits before resolving the layer for a slider opacity change.
  The opacity dialog uses the same commit path. Layer thumbnails keep the layer's
  aspect ratio and draw their checker only inside that area. The preview is
  display-only until the pointer is released. Move's crosshair has a four-arrow badge, and
  its tool options align the layer to canvas edges or center it with icon
  buttons. Move's Snapping disclosure starts collapsed, retains its session
  state, shows the active snap modes while folded, and never changes settings
  when folded. Tool options use their natural height up to half the inspector,
  scrolling independently from Layers/Paths in the remaining space.
  The right inspector is 272 px wide to accommodate visible per-row lock controls.
  The resizable toolbox starts at the same total width, sharing the inspector
  constant. Its outer container owns that width; the resize grip overlays its
  inside edge instead of adding a separate layout column.
  The Layer menu has the fit and content-crop commands.
  Pointer Move computes snapping from the unsnapped grab on every event, including
  final release and Control changes; it never accumulates snapped deltas.
  `src/guides.rs` owns bounded guide metadata, image transforms, grid density, and
  deterministic snapping. `src/workspace/guide_ui.rs` owns rulers, guide hit tests,
  and reversible guide drafts scoped to tab/revision. Guide commits refresh no
  pixels/thumbnails; per-tab pending guides bridge asynchronous publication so
  release does not flash the old position. History accounts for owned guide data.
  `src/workspace/dialogs.rs` renders the canvas dimensions, color, JPEG quality,
  layer, adjustment, and filter dialogs. `src/workspace/color.rs`
  handles the HSV wheel and brightness slider.
- `src/workspace/numeric.rs` edits the selection radius, selection position/size, crop
  fields, shared wand/fill tolerance, brush size/spacing/hardness/opacity/flow, path
  stroke size, selection/crop aspect ratios, Move position, Scale Layer dimensions,
  Rotate angle, and text size/letter/line
  spacing inline with a caret and text selection. Values preview live; selection,
  scale, and Move edits commit one history entry. Rotate angles retain Apply/Cancel.
  Empty text and Escape restore the value from before editing. Letter spacing
  accepts negative values. A shortcut or a switch to another numeric field waits
  for a selection, scale, or Move edit to settle and its action bindings to render; stale
  followups are rejected after a tab, tool, or revision change. Rotation accepts
  signed decimals; other numeric fields retain their existing integer contracts.
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
  app icon: a small lime half-slice angled at 45 degrees inside the original
  corner frame, retaining the blue background and blue/yellow palette.
  `macos/AppIcon.icns` is its packaged form, and `windows/AppIcon.ico` contains
  the same artwork in nine sizes. README uses the shared SVG for its logo.
  `build.rs` embeds `windows/app.rc` in native Windows builds as icon resource 1,
  which GPUI loads for its window class. Keep GPUI's manifest separate and enabled.
  The Windows workflow verifies the icon group and manifest before packaging.
- `src/macos.rs` applies the same icon to the running Dock entry, including
  development launches from Cargo.
- `scripts/bundle-macos.sh` copies the executable, Info.plist, and icon into
  `dist/Lime.app`, ad hoc signs the bundle, checks that signature and the
  project and raster document types, and rejects a Cargo version that differs
  from `CFBundleShortVersionString`. `scripts/package-macos.sh` builds the app
  and creates the drag-to-Applications `dist/Lime-<version>.dmg`. The macOS SDK
  renders its 1x/2x background via `scripts/dmg-background.swift`, and
  `scripts/dmg-layout.applescript` saves a 640-by-400 Finder content layout with
  Lime and Applications icons, an arrow, and installation instructions. Packaging
  requires a logged-in Finder session with Automation access, uses a temporary
  writable HFS+ image, and compresses/verifies it before replacing the final DMG.
  Its cleanup detaches only its own mount. The mounted volume uses the app icon.

## Build and check

From the repository root, use a recent Rust toolchain and Xcode. Run
`cargo fmt --check`, `cargo clippy --locked -- -D warnings`, and
`cargo test --locked`. Keep `Cargo.lock` tracked. GPUI on macOS uses
`font-kit` for text and compiles its Metal shaders at runtime.

Keep the package version in `Cargo.toml` and
`CFBundleShortVersionString` in `macos/Info.plist` in sync; a test checks both.

Run `./scripts/bundle-macos.sh` and open `dist/Lime.app` for native smoke
checks. `./scripts/package-macos.sh` additionally creates and verifies
`dist/Lime-<version>.dmg` with an Applications shortcut. Test a valid image,
a multi-page PDF import with page and DPI choices, and
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
