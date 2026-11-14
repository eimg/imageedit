# Lime agent guide

Work directly on `main` unless the user explicitly requests another branch.

Lime is a Rust and GPUI desktop image editor. Build for macOS first and
keep the shared workspace portable for Windows and Linux. Hide and Quit stay in the macOS
app menu. Windows and Linux use File → Exit. The product
takes cues from GIMP, with native menus, a docked, resizable, and foldable
toolbox, a Layers panel, and a tabbed canvas. File commands belong in the menu.
The workspace favors canvas space and has no top toolbar.
Windows has been manually reviewed by the maintainer and approved for release. A Windows-only `muda` adapter builds a native menu bar from the
shared GPUI menu tree, and `.github/workflows/windows.yml` produces an x64
portable ZIP. Releases distribute the EXE without an installer or file
associations. The macOS DMG is currently Apple silicon only. See README.md
for downloads and signing status.

Linux x86_64 and ARM64 (`aarch64`) ship as portable downloads. The maintainer
accepted device verification for 1.2.0, and the combined candidate passed CI for
all four release targets. ARM64 Wayland was visually checked on Arch Linux ARM/Hyprland
with Mesa 26.2.6 software Vulkan in a VM; X11/Wayland smoke checks also pass.
GPUI's Linux target enables both Wayland and X11. `src/workspace/linux_menu.rs`
renders an in-window menu bar from `commands::application_menus(false)` without
introducing GTK or changing the macOS/Windows menu adapters. Menu navigation
runs before inline editors in the workspace keystroke interceptor; editor focus
stays in place for action dispatch. F10/Alt-letter, arrows, Return, Escape,
scrolling, submenu Back/Left, and current action availability are supported.
Wayland explicitly requests client decorations to avoid GPUI 0.2.2 recording
nonexistent server decorations on GNOME/Weston. Client decorations get a draggable
title bar, close/minimize/maximize controls, and resize edges; server-decorated
windows retain native chrome.
The menu module and integration also compile under `cfg(test)` on other platforms.
`vendor/xattr-compat` bridges GPUI's legacy archive dependency to maintained xattr,
retaining its `set` API without pinning libc or copying obsolete FFI.
`.github/workflows/linux.yml` builds/tests natively on Ubuntu 22.04 x86_64 and ARM64;
`scripts/package-linux.sh` creates raw ELF downloads, portable tarballs, and SHA-256
checksums in `dist/`. Build on the baseline rather than raising the glibc minimum
on a newer host. Linux requires Vulkan, fonts/window libraries, and a working
file chooser portal; do not advertise a fully static universal binary or assume
all ARM GPUs work. The portable archive contains optional desktop integration
files but never installs them. Update discovery recognizes both Linux architectures
and stores its cooldown in XDG cache storage. See `linux/README.txt` for runtime
packages and manual desktop integration.
Empty or relative XDG configuration/cache roots fall back to absolute HOME-based
defaults. Gradient storage and crash recovery share the configuration root.

Use the user's current request to scope work. This guide records the current
architecture and editing contracts; [README.md](README.md) describes using the
app. Update both when behavior changes. Completed plans, feedback queues, and
review handoffs belong in Git history rather than separate living documents.
Keep unrelated work and existing edits intact.
README.md is a compact user-facing overview and download page. Keep detailed
tool instructions, build/CI details, validation status, and release preparation
notes out of it. The UI provides tool help; a dedicated guide website is planned.

## Current product scope

Lime supports layered raster editing. The empty workspace can open a
file or create an image. Crop that image: drag the box, move it from inside,
resize it from its handles, and edit X, Y, Width, and Height in the Crop
options. Typed values apply immediately, and an empty field leaves the crop
unchanged. Crop drags may start outside the canvas; both creation endpoints clamp
to image pixels so free and fixed-ratio crop boxes stay inside the image.
Crop has Free, 1:1, 4:3, 16:9, Image, and custom integer aspect ratios. A fixed
ratio constrains creation, handle resize, and numeric dimension edits; choosing
a ratio alone leaves the current box unchanged. Sample or set swatches and fill
connected regions on the active layer.
One shared recent list holds 12 unique RGB colors across tabs and all color
pickers. Accepting foreground/background, Text, Drop Shadow, and gradient-stop
colors or sampling with Eyedropper remembers the RGB; canceled picker drafts do
not. Foreground/background swatches and recents persist in personal `settings.json`
beside `gradients.json`, outside project data/history. Swap and Reset persist;
Reset leaves recents intact. Gradient alpha stays separate from RGB memory.
Rectangle, oval, circle,
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
oval tools remember a shared ratio across restarts; Crop has a separate one.
Antialias edges defaults off and smooths oval/circle and rounded-rectangle
coverage with a 4×4 subpixel sample, using fast interior/exterior checks. It
updates a current editable curve in one selection-only undo step. Masks retain
their coverage when the preference changes. Move, resize, crop, canvas resize,
image transforms, and history preserve curve quality; clipped curves become
coverage masks.
Rectangle and oval drags may start or extend outside the canvas; coverage stays
bounded to canvas pixels and clipped curves retain their original geometry.
New geometric selections use the persistent radius/antialias defaults rather
than copying style from an existing selection at gesture start. Moving/resizing
an existing shape still preserves its own quality and coverage.
Shift adds to the selection, Control subtracts, and Shift-Control intersects.
Wand and Bucket have independent persistent Tolerance and Gap radius settings,
starting at zero. Zero tolerance matches exactly. Gap radius (0–8 image pixels)
closes small breaks in nonmatching boundaries before four-connected region
traversal; 0 preserves the original fast path. A temporary hard boundary mask
uses square max/min closing with neutral finite-source borders so gaps near
image edges can close. With gap closure enabled, original nonmatching pixels
always remain excluded; large radii may seal narrow passages or entire small regions. Active-layer
fills retain layer-local/off-canvas behavior, and merged sampling uses canvas
coordinates and edits only the active layer. The wand can sample the active
layer or visible composite, independently of Bucket Fill. Both region tools
ignore stored RGB when both compared pixels are fully transparent; other channel
and alpha comparisons use the tool's own tolerance. Wand has an independent,
default-off Antialias edges preference for new selections. It reconstructs the
completed connected region's contour with 4×4 bilinear subpixel samples, leaving
straight boundaries and uniform interiors hard. It does not widen the matching
threshold or propagate a flood through fractional coverage. Sealed gap barriers
and explicit limits remain excluded. Feathering follows contour reconstruction
and precedes combination; changing the preference alone edits no selection.
Bucket's Selection mode hides and ignores matching/sampling controls without resetting them; mode switches
finish inline numeric edits before hiding those fields.
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
Layers have independent alpha and position locks. Each row exposes visibility,
a movement-link toggle, and a plain position padlock. Alpha lock lives in the
Layer menu and layer context menu. Row toggles share visibility's background-free
resting style and hover treatment; closed locks use the accent color, including
while hovered, while open locks are dimmed.
Row toggles target stable tab/layer IDs without changing the active layer; typed
geometry and text commit first, with deferred actions validating their context.
Alpha lock is disabled for editable text with an explanatory context-menu tooltip.
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
Layer movement links form one explicit set per document. Every row always shows
a chain toggle, also available through Layer → Toggle Movement Link. Moving a
linked layer with the pointer, typed X/Y, arrow nudges, or alignment translates
all enabled members by the same delta, including hidden layers, in one history
edit. Moving an unlinked layer does not move that set. A single enabled toggle
stays enabled. Snapping/alignment use the active layer's geometry.
New Drop Shadow layers attach directionally to their source without changing
movement toggles. A source move includes its attached shadows and descendants;
moving a shadow directly adjusts its own position without pulling its source.
The adjusted relative offset is preserved on future source moves. Followers are
included once, even when also explicitly linked. Following an attachment does
not expand the movement set through that follower's manual links. Shadow rows
show their source beneath the name; Layer/context menu → Detach from Source
makes them independent. Attachments do not regenerate shadow pixels after source
edits. Any participating position lock or offset overflow blocks the entire move;
pointer/nudge attempts name the locked layer.
Row metadata toggles target stable tab/layer IDs without changing the active
layer and finish typed geometry/text first. Links and attachments are saved,
undoable project metadata. Duplicates start unlinked and unattached. Deletion/crop
prune missing endpoints; Merge Down detaches both inputs and the result while
preserving unrelated memberships/attachments. Individual scaling, rotation,
pixel edits, opacity, visibility, and stacking stay independent. Image-wide
transforms still affect all layers.
Right-click selects the target row and opens a compact context menu containing
Rename, Duplicate, Delete, Alpha Lock, and relevant Rasterize Text/Detach from
Source actions. Broader transforms, alignment, masks, and effect actions stay in
the application menu/panel. Visibility, position lock, and movement link stay in
the row and application menu. Disabled context actions have reason tooltips.
The popup supports arrows, Home/End, Return, Escape, and scrolling.
It finishes numeric/text edits first, waits for the selected
layer's rendered handlers, and validates stable IDs/generation before dispatch.
Deactivation, outside clicks, and stale target state dismiss it. Delete remains
available below the layer list.
The Layer menu can align a layer, proportionally Fit Layer to Canvas, or crop
it to its opaque pixels. Fit Layer to Canvas also appears in Scale Layer options,
scales the complete raster (including transparent margins), and centers it. It
may enlarge smaller layers and uses the workspace's selected resampling. Hidden,
position-locked, and editable-text layers cannot use it. Layer to Image Size
instead crops/pads at the existing canvas pixel positions. Image → Crop to Content
does the same for the canvas. Image and Layer transforms rotate and flip
by quarter turns. Rotate, Flip, and Perspective preview the active layer and
commit on Apply, leaving the selection in canvas coordinates. Rotate also accepts
typed angles in degrees with tenths precision. Rotate's Pivot offers nine
original-layer anchors, signed canvas X/Y in tenths of an image pixel, and a
screen-sized circular crosshair that can be dragged without snapping to the click.
Custom pivots recalculate the original layer at the current angle; Shift-drag
rotates around that pivot and snaps to absolute 15-degree angles, including when Shift changes without moving the pointer. Perspective
corner grips keep a dark rim so they stay visible on a white image. A 2×2
corner selector and signed canvas X/Y fields edit individual points to tenths of
a pixel; dragging selects and accents that grip. Reset Corner or Reset All
restores original source geometry in the draft, without editing pixels. Invalid
quads remain editable with Apply disabled. The Scale
Layer tool draws a frame around the active layer and previews scaling from a dragged
border or corner; the tool uses Shift+S. Apply or Return on the canvas commits
all preview adjustments in one undo step; Cancel or Escape on the canvas discards
them. Repeated drags and numeric edits always resample the original pixels only at Apply.
Like warp tools, Scale owns an explicit preview session: Apply and Cancel remove
its frame and editing controls. Selecting the tool or changing tabs/layers starts
a session; after dismissal, a canvas click or reselecting Scale starts another.
An untouched session leaves Save and Repeat Last Effect available.
Width/Height edits preview a resize about the chosen nine-point Size anchor
(Center initially). Return or leaving
the field finishes that field without applying; empty text and Escape restore
its prior size while retaining earlier preview adjustments. Aspect locking starts
on and applies to numeric edits and handles; Shift reverses it during a drag.
Nearest, Bilinear, and Bicubic choose the commit resampling. A tool/layer/tab
change cancels the preview; deactivation ends its pointer drag while retaining
the preview. Fit Layer to Canvas previews while Scale Layer is active and
commits immediately from other tools. Transform Apply/Cancel rows share one
renderer. Scale, Rotate, Flip, Perspective, Crop, and geometric selections share
screen-sized white square grips with a dark rim; the selected Perspective grip
adds an accent border. Scale shares the warp frame renderer; crop and selection
keep their specialized outlines. Clone Stamp uses Shift+C,
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
stroke commits when the pointer is released. Pencil, Brush, Eraser, Blur, Sharpen,
and Clone Stamp strokes may start outside the canvas and enter it while held;
painting remains clipped to the canvas, active layer, and selection. Eyedropper,
Wand, Bucket Fill, and clone-source picking still require an inside-canvas pixel.
Their pointer is a size-scaled
ring, hard for Pencil and soft for the others. Blur and Sharpen change local
pixels under the brush. Blur and Sharpen have no shortcut.
Brush, Eraser, Blur, Sharpen, and Clone Stamp have independent persistent smoothing
settings: Off (default), Light, and Strong, shown in a full-width segmented group
below a regular mixed-case Smoothing field label with tight label/control spacing. Pencil and outline strokes stay
unsmoothed. Light/Strong use time-based responses of 16/32 ms and finite settling
windows of 64/128 ms. A short-lived eight-ms timer catches up when the pointer
stops, sleeps when settled, and validates a per-stroke token before touching
live pixels. Off bypasses the filter and timer. Release drains the remaining
bounded tail synchronously to the exact endpoint before the ordinary single
stroke commit; cancellation discards it. Shift-click connectors remain straight.
The Clone source indicator uses the filtered destination during a smoothed stroke.
The paint and retouch tools retain independent brush settings across restarts;
path and selection outlines share their own settings. Opacity limits each stroke
relative to its starting pixels. A 16-bit coverage buffer per touched tile keeps
overlapping dabs from exceeding that limit and applies soft selection coverage
once. Buildup defaults off; when on, Flow (0–100) accumulates dab coverage toward
the opacity limit. Flow and buildup are remembered per tool across restarts.
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
A personal persistent library holds up to 32 named gradients, capturing literal
RGBA stops, shape, and Reverse. Names are trimmed, limited to 64 characters,
control-free, and unique by lowercase comparison. Stable monotonic IDs identify
entries through rename/delete and callbacks. Loading initializes Custom mode
without changing swatches or canvas endpoints. The library is folded by default;
its expanded list scrolls within 156 screen points. Save/Rename use the shared
name dialog. All library mutations publish a candidate only after the background
write succeeds. Failed writes keep the existing in-memory library and report an error;
Reload re-reads disk before further mutations. Library load/save never creates
image history or changes project saved state. Loading or naming a preset finishes
inline stop edits before capturing/replacing palette values. Settings and preset
loads refresh the coalesced preview without image history. Apply uses the same sampler,
selection coverage, color-mode conversion, and alpha-lock policy. Color picker
targets carry a palette revision and stop index; stale targets cannot recolor
another palette, and RGB changes preserve stop alpha. Percentage editing stores
the exact alpha byte for restoration and accepts decimal input.
Bucket Fill can cover the connected region or the whole selection. Brightness &
Contrast, Hue & Saturation, Desaturate, and Invert preview the active layer's
effect in the canvas composition inside a dialog, and apply inside the selection.
Colors adjustment dialogs share Drop Shadow's responsive 720-point layout,
placing the preview beside settings at window widths of at least 760 points and
stacking at narrower widths. Levels pairs input black/white and output black/white
sliders, with Gamma on its own row. Effect titles/actions stay outside the scrolling
body in short windows.
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
across restarts, shared across tabs; cancellation, failed/stale jobs,
and pixel no-ops leave prior settings intact. Filters → Repeat Last Effect
(Cmd/Ctrl+F) applies the last successful adjustment/filter to the current visible
raster layer and current selection as one undoable edit. Reopen Last Effect
(Cmd/Ctrl+Shift+F) opens its remembered parameters with preview. Typed geometry
and text resolve first; unfinished canvas previews disable these commands.
Shadow repeat preflights the current destination and keeps the source active.
Its dialog uses a preview beside the controls on wide windows and a stacked
layout on narrow windows. Shadows default to black; Choose Color opens the shared
color picker and Use Foreground copies the foreground. Canceling the picker
returns to the unchanged shadow draft. Reset restores built-in parameters and black. Memory is not project data
and is not undone with the document. Path (`P`) edits cubic anchors in canvas
coordinates. Click adds an anchor, a drag pulls its handles, and clicking the
first anchor closes the path. Alt-clicking a visible curve inserts an anchor by exact cubic
subdivision, including closing segments; existing anchor/handle hits take priority.
The inserted point can be dragged within that same one-step gesture. Hit testing
uses an eight-screen-point radius, bounded subdivision, and chord-projection/
Newton refinement, including stationary collinear parameterizations. Shift-click
toggles anchor selection within one path without starting a gesture; clicking an
unselected anchor replaces the group, while dragging a selected anchor translates
all selected anchors and their handles from their gesture-start geometry. All
Anchors and Deselect live in Tool Options with the selected count. Every selected
anchor has a larger outlined marker. Corner collapses the group's handles; Smooth
aligns them and preserves existing lengths, initializing missing handles from neighbors.
Delete/Backspace removes all selected anchors in one metadata edit, or the last
anchor when none are selected. Removing down to two anchors opens the path;
removing every anchor removes the path. A group click never closes an open path;
click-first-anchor closure is retained for a single selected anchor. Selection
is session-only and clears on path/tab/tool switches, hiding/deleting its path,
and undo/redo. Stale or hidden IDs cannot target another path.
Independent (default), Aligned, and Symmetric are persistent handle-drag preferences;
they preserve the opposite handle, its length, or mirror its length respectively.
Newly placed handles retain symmetric creation. Linked drags use the baseline
anchor each frame to avoid drift. A click without movement does not couple
handles; repeated identity shaping creates no history. Geometry, not new anchor
mode metadata, persists in the existing project format. Each group gesture or
shaping/deletion action is one undo step and keeps raster buffers shared.
Path to Selection fills every closed path with
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
selection anchors and caret affinity. Wrapped boundaries preserve upstream/downstream
visual placement through pointer hits, Home/End, arrows, and typing undo/redo.
Pointer selection uses the latest matching caret map;
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
pages or a page list, sets a resolution from 1 to 1200 DPI, and renders pages as
layers on the first selected page's canvas, layers on a Fit All Pages canvas, or separate
tabs at each page's own size. White is the default page background; Transparent
preserves unpainted areas and source opacity without removing painted white.
Fit All Pages takes the maximum width and height across the selected pages after
DPI rounding, and validates the combined canvas before rendering. All imported
layers retain their pixels and use the same integer centering rule, including
the first page. The dialog shows canvas dimensions and warns about pages extending
outside the first-page or existing canvas. Open as Layers never resizes its target.
Successful imports remember DPI/background across restarts, and successful
new-document imports also remember mode/canvas choice. Failed/canceled imports
do not change defaults; page selection resets per source. The PDF dialog is
720 px wide above a 760 px window width, with settings beside the page picker;
narrower windows stack its content. Header/footer stay outside the scrollable
body. Page thumbnails toggle inclusion, updating a compact canonical range list.
All selects the source and None clears it; the page field stays visible. An empty
or invalid selection, invalid DPI, budget failure, or stale target disables Import
with its reason shown. Page names, numbered tab titles, and progress retain
source numbers. Lists accept at most 2048 characters and hyphen/en/em dashes.
DPI/page inputs support pointer caret placement using shaped glyph positions,
drag/Shift-click selection, double-click select-all, and horizontal caret scrolling.
Shared Field editing handles partial-range replacement/clipboard and Shift+arrow/
Home/End extension. Tab cycles both inputs without changing selected pages.
Thumbnails load automatically in three-page batches. Previous/Next browse source
batches; Selected brings the first chosen page into view. Selected thumbnails
have check marks and accent borders. Previews respect white/transparent background,
with a checker only within the fitted bitmap, and remain independent of import
DPI. Per-page errors expose details and Retry previews. Dismissing the form
cancels preview work and releases published images.
PDF imports use the
normal image and stack budgets and become unsaved `.lime` projects; PDF is
export-only, never a Save format. Encrypted PDFs are unsupported, and Open as Layers accepts
one PDF at a time. PDF read/inspection/render/preparation jobs show progress and
accept Cancel or Escape. Cancellation is cooperative: an in-progress parser,
page render, or image-preparation call finishes before cancellation settles;
its results are discarded. New documents and Open as Layers publish no partial
prefix. PDF bytes are retained from inspection through rendering, independent
of later source-file replacement/deletion. Open as Layers preflights the entire
page set against remaining layer count, IDs, and stack bytes before rendering and
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
overlay/snap preferences persist across restarts. Save and Save As handle
Lime (`.lime`) projects (legacy `.imageedit` files remain readable),
PNG, JPEG, and WebP. A project stores the layer stack, paths, guides, and text with lossless pixels.
Export writes flattened PNG, JPEG, WebP, or single-page PDF copies to a separate destination
and preserves the tab’s saved state. PDF exports the complete current canvas as a
lossless RGB image over white with embedded sRGB. Its dialog shows physical page
size and accepts 1–1200 DPI (300 initially, remembered across restarts), with a
canvas-dependent minimum to stay within 200 inches per page edge. DPI changes
page size without resampling. Layers and text are flattened; multipage, paper
layouts, searchable text, and vector output are deferred. `src/pdf/export.rs`
uses `pdf-writer` and row-wise Deflate compression; `document::export_pdf` shares
atomic writes and the workspace's existing background export route and pending
edit checks. The PDF menu finishes inline geometry before text and defers through
`NumericFollowup::PdfExport` when its background commit must land first, validating
tab and generation before reopening. PDF filenames stay excluded from `save_format`.
Raster Save is offered only while a
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
  The status bar always shows the compiled package version beside the image count
  in muted 11-point text, matching the update badge's size and padding; the update
  badge remains the rightmost item when available.
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
  `src/paint/smoothing.rs` implements the bounded, constant-space timed stroke
  filter. It rejects nonfinite inputs/backward timestamps, advances in at most
  sixteen short tail segments, and drains the same curve on release. Duplicate
  pointer coordinates do not postpone settling. `src/workspace/paint_ui.rs`
  owns its per-stroke clock and token-checked wakeups, separately from dab spacing.
  `src/workspace/paint_ui.rs` composites the live tiles over the existing
  preview and owns the per-tool brush settings. Release commits dirty regions
  in one undo step. Its scoped CloneSource validates tab/layer geometry and
  establishes alignment only after the stroke result publishes successfully.
  `src/gradient.rs` owns bounded color ramps, ordered stop edits, and premultiplied
  interpolation. `src/workspace/gradient_ui.rs` shares one specification between
  the coalesced preview and Apply, owns the persistent current palette and named
  library UI, and preserves geometry when loading presets. Apply/Cancel remain above the stop editor.
- `src/gradient/library.rs` validates the version 1 JSON gradient library,
  including IDs, unique names, at most 32 entries, 2–8 strictly ordered stops,
  and a 256 KiB file bound. `ColorRamp::from_stops` validates reconstruction.
  Files live under macOS Application Support/com.eimg.lime or Windows
  LOCALAPPDATA/Lime, separate from projects. Initial loading and writes use the
  background executor. Same-directory temporary files are synced then atomically
  persisted; an OS advisory lock serializes Lime writers, and expected-byte
  checks reject stale snapshots. Corrupt or newer files are not overwritten.
  Workspace preset mutations are serialized and acknowledged after persistence;
  store failures block mutations until Reload, while existing gradients remain
  usable. Close/Quit defers while a library write is active, resumes the normal
  close checks after success, and stays open on failure. No idle polling is used.
- `src/workspace/scale_ui.rs` owns the Scale Layer frame, handle hit testing,
  the live GPU-scaled preview, and the commit that resamples the layer. It prepares
  the preview in the background and rejects results after Apply or cancellation.
  Release retains the draft; later pointer/numeric gestures capture the current
  target as their baseline while preserving the original source for one Apply.
  Frame and draft sizes exist only during the transform session; tool preferences persist separately; completion never
  falls back to displaying an idle frame from the committed layer. Scale and warp
  sessions share tool/tab/layer startup routing and restart on a canvas click.
  Numeric dimensions use the same preview/commit path; aspect locking retains
  opposite corner anchors or centers the unhandled axis. Numeric edits keep the
  selected Size anchor, captured when that edit starts. Choosing an anchor alone
  changes no pixels/history; handle behavior stays independent. Anchor placement
  rounds absolute canvas coordinates and rejects i32 overflow before resampling.
  All targets respect image and stack budgets. Hidden layers must
  be shown before scaling. Fit Layer to Canvas routes through the shared action
  and background edit pipeline. `geometry::proportional_fit` uses integer aspect
  comparison/rounding and plans the centered bounds without allocating pixels;
  `DocumentContent` preflights replacement memory before scaling/positioning.
  Identical fitted bounds are a no-op; pure recentering shares the existing pixels.
  The selection stays in canvas coordinates and one fit is one image undo step.
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
  composites canvas-space samples into a display-aware preview, fitted to the
  dialog's image box and screen scale, with a 1024-pixel output edge cap and no
  source upscaling. Streaming box-area reduction averages all contributing source
  pixels in premultiplied color before decimation, avoiding periodic-detail aliasing
  and hidden-RGB fringes. Composition and reduction use source/preview row buffers,
  never a full-canvas scratch bitmap. Source visits run in the coalesced background
  job. Adjustments map source pixels before reduction using the same prepared
  mapper/coverage blend as commit. Filter input also uses area reduction, capped
  to a source-bounded 2× preview grid. Filter scratch data
  covers only the visible source region plus its full sampling halo, including
  off-canvas pixels; radius and source samples scale for a reduced preview.
  Subpixel blur/sharpen radii leave the original preview unchanged. Shadows
  sample the source region shifted by their offset and composite below the source,
  independently of selection/source-layer opacity. Full-resolution preview tests
  compare these paths to real commits; larger-image filter previews are approximate.
  `EffectImages` keeps a checker-backed Before/After pair. Replaced, canceled,
  and stale pairs release both GPUI images. Layout/screen-scale changes replace
  the immutable preview snapshot and invalidate earlier jobs through the existing
  coalesced background queue. Preview generation gates publication and supplies
  the dialogs' updating status. Reset uses built-in form defaults,
  independently of remembered parameters; neither comparison nor reset edits pixels
  or history.
- `src/workspace/effects.rs` shares the normal/repeated effect commit path and
  owns bounded, persistent parameter memory for the seven adjustments and three
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
ratio geometry in `src/constraints.rs`; each has its own persistent preference.
Geometric curve quality lives with the selection, including in history.
Empty numeric text restores the selection. Cropping or shrinking the canvas preserves curved selection coverage;
a partially clipped curve becomes a bounded mask. Scaling a rounded selection
scales its coverage on both axes as a mask. Free Select drags a freehand
segment or click-adds polygon vertices, and previews the next edge. Releasing a
drag keeps the draft open as a checkpoint. Polygon vertices and freehand segment
endpoints have screen-sized draggable nodes; intermediate freehand samples stay
unmarked. Node hit testing chooses the nearest checkpoint within eight screen
points. Drags preserve their grab offset and edit only that point from its original
position. Clicking the start node without dragging or Return closes the outline;
node edits remain in the draft until that single selection history step. Closed
free selections remain masks without node editing. Cancel/deactivation and context
changes clear checkpoint and drag state with the draft. Optional polygon antialiasing uses four scanlines per pixel row and counts
horizontal samples into the normal byte-per-pixel mask, without a supersized
intermediate image. Hard polygon/path rasterization keeps its center-sample
scanline path. `combine_created_selection` feathers new shapes/flood results
before combining them, preserving previous coverage and one selection history
entry. The Magic Wand keeps its own tolerance and gap radius, separate from Bucket Fill. Its optional visible-composite
source is built in the background and floods in canvas coordinates; active-layer
sampling maps through the layer offset.
Fill, Clear, and the clipboard honor feathered coverage. Shift adds, Control
subtracts, and Shift-Control intersects. Over the box or ellipse the painted
crosshair gives way to a hand or resize cursor. The outline is a dashed
contour. Stroke polylines follow that contour, including a hole. Mask
transforms avoid per-pixel coordinate lists; feathering uses running sums and
Grow/Shrink use linear-time running max/min filters to preserve soft coverage.
- `src/selection/antialias.rs` reconstructs wand contours from the completed hard
  flood. A 512-entry table maps 3×3 neighborhoods to sixteen bilinear subpixel
  tests, preserving reflection/rotation symmetry and straight edges. Source
  borders clamp their neighbors, while canvas clipping retains the whole source
  neighborhood. The flood's byte-per-source-pixel visitation state records
  admitted pixels in place; there is no second source-sized mask or supersized
  raster. The output uses the existing canvas-bounded coverage mask and history.
  Gap/limit exclusions are rechecked when writing soft coverage, so contour
  reconstruction never reopens a sealed passage. Bucket paths remain hard.
- `RegionMatch` in `src/selection.rs` closes temporary boundary masks using the
  shared separable running extrema kernel: dilation uses outside 0, erosion uses
  neutral outside 255, while ordinary Grow/Shrink keep outside 0. Original
  color comparisons still gate traversal. Source traversal and the optional
  canvas-space domain remain separate from the clipped output mask.
- `src/transform.rs` turns, mirrors, scales, and anchors layer pixels, and
  warps a layer by an angle about its center or a custom canvas pivot, or by a
  perspective quad. `RotateAround` validates finite inputs and placed bounds;
  integral quarter turns keep exact pixels, fractional placement goes through
  the selected resampler, and full turns return the original geometry even for
  large finite pivots. The centered default retains its earlier placement rules.
  Grayscale keeps alpha and
  is stored in the version 6 project. Smooth scaling filters exact 16-bit
  premultiplied color products to retain faint colors. Image scaling validates
  each layer against the image limit and the aggregate against the stack limit
  before allocation, and rejects a scale that would make path coordinates nonfinite.
- Rotate pivot state belongs to `WarpSession`, starts centered for each target,
  and does not persist in a project. Choosing/moving it at zero angle repaints
  only the frame and creates no image edit. Nonzero changes use the coalesced
  preview and the same `Warp` geometry as Apply, including stack/placement limits.
  Pivot drags retain the pointer grab offset and quantize absolute coordinates
  to tenths, avoiding accumulated drift. Center restores the original lossless
  centered quarter-turn path. Session/layer/tab/tool cancellation clears pivot
  numeric editors; position-locked and editable-text layers keep their guards.
- Perspective uses the same bounded signed-tenths numeric contract as pivots.
  Coordinate fields carry their corner index, so switching the selected corner
  finishes the old edit before showing another point. Pointer corners quantize
  to tenths too, preserving exact empty/Escape restoration. Reset actions keep
  unrelated corner changes; source pixels remain immutable until Apply.
- `src/geometry.rs` centers a layer, finds nonzero-alpha bounds, and copies
  the on-canvas part of a layer into a canvas-sized image.
- `src/document/content.rs` owns the canvas dimensions, raster layer stack, and paths.
  It handles crop, layer-local bucket fill, layer offsets, content crops, and outline strokes.
  `src/document/links.rs` owns the stable-ID movement set, directional shadow
  attachments, validation, atomic checked translations, and pruning. `set_offset` stays the
  single-layer primitive for transforms; user moves and alignment use
  `move_linked_layers`. Composite dirtiness covers all visible members' old/new
  bounds and retains shared pixels and thumbnails.
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
- `src/project.rs` reads project versions 6 through 10 and applies the same size limits
  in both directions. Any layer lock requires version 8 with an optional per-layer
  `locks` object. Missing flags default to false; unlocked layers omit the object.
  Without version 9 metadata or locks, projects containing guides write version 7; without guides
  they retain version 6 and omit the guides field for compatibility with older
  builds. Version 6 manifests may not contain nonempty guides. Versions 6/7 may
  not contain active layer locks; alpha-locked text is rejected. Versions before 6
  are not supported. `fixtures/projects/` holds a golden
  project that the reader must keep opening. Regenerate it only when the
  format intentionally changes. The current fixture is version 6
  (`v6-two-layers.imageedit`); keep its legacy extension to exercise compatibility. New projects use `.lime`; the file magic
  stays unchanged; the existing version 6 fixture remains unchanged.
  `v8-layer-locks.lime` additionally covers the lock-bearing format.
  Version 9 adds optional text layout, blend modes, masks, retained effects, and layer links;
  these require version 9 independently of locks/guides. Mask/source/coverage PNGs
  follow their layer PNG in `RasterLayer::storage` order, and all decoded buffers
  count toward the aggregate limit. Lower feature-free versions still write as
  before. `v9-capabilities.lime` covers the new fields and payload ordering.
  Version 10 stores the explicit movement set and directional shadow attachments.
  Top-level `layer_links` retains its historical array-of-arrays shape but permits
  at most one nonempty set, including a singleton. `shadow_attachments` contains
  child/source ID pairs. Both fields omit empty values. Readers validate existing
  IDs, unique memberships/children, and acyclic attachments before decoding pixels.
  Version 9's disjoint groups migrate into one movement set; shadows in older
  projects remain manual links, with no inferred attachments. Link/attachment-free
  projects retain earlier writer versions when their other metadata permits it.
  Text bold/italic flags are optional in all supported versions and default to regular for
  projects written before those controls were added.
- `src/pdf/pages.rs` parses bounded page-list expressions after checking source
  count and endpoints, canonicalizes duplicates in document order, and validates
  nonempty, unique ascending page numbers against the import page cap. Estimates
  and Apply share its parser for all/custom page choices. `PdfOptions.pages`
  is an explicit source-page list, and preparation checks its count/order before
  constructing a document or inserting layers. Only selected pages contribute
  dimensions, memory, and canvas bounds at import resolution. The
  thumbnail browser can render unselected pages at its fixed small resolution.
- `src/pdf/thumbnails.rs` renders up to three pages with each axis bounded to
  1–144 pixels. It uses the retained `PdfSource` bytes, validates source page
  numbers, handles thin/large pages without full-resolution allocation, and
  catches page render panics independently. Cancellation/parser failures discard
  the batch; individual page failures stay local. A per-source render mutex
  serializes thumbnail and full import parsing/rendering; cancellation is checked
  while waiting for the gate, between pages, and after rendering. Production
  parsing/rendering use the disposable worker described below.
- `src/workspace/pdf_preview.rs` owns the interactive page picker and one coalesced
  background preview worker per workspace. Source/batch/background changes
  invalidate its generation, cancel old work, and retain only the latest request.
  New preview jobs wait until the old worker has returned, even across forms.
  Published/stale GPU images are explicitly dropped when replaced or discarded;
  closing the workspace cancels its worker. At most one displayed batch is kept,
  with no persistent thumbnail cache or idle polling. Preview work never changes
  image history or remembered import preferences. Thumbnail clicks update the
  form page selection through bounded `toggle_page_selection` range formatting. The checker is
  shared with layer thumbnails.
- `src/pdf.rs` probes PDFs with `hayro`, validates every selected page against
  the image, stack, and page-count budgets, and renders at the requested DPI on
  a white or transparent page background. White output flattens renderer rounding
  residue onto white so pages and thumbnails remain fully opaque. Transparent
  output retains source alpha. Its `layered_canvas_size` validates First Page/Fit
  All Pages canvas bounds independently of individual page budgets;
  portrait/landscape maxima can exceed the canvas pixel budget even when each
  page fits. Transparent rendering requests straight RGBA from the renderer.
  `PdfSource` retains the immutable shared `PdfData` bytes
  and inspected dimensions; render jobs parse those same bytes without reopening
  the path. `PdfControl` checks cancellation between bounded read chunks, page
  inspection/rendering, and document preparation; production renderer/parser calls
  run in a child that can be killed and reaped on cancellation. `src/workspace/pdf_ui.rs` owns probe and render jobs and
  turns the result into new documents or layers above the active layer. New
  layered preparation checks the complete rendered page set and stack admission,
  rejects incomplete insertions, and centers every page consistently even for
  odd dimensions. Session `PdfPreferences` update only after successful document
  or existing-layer publication; importing into layers preserves the remembered
  new-document mode/canvas policy. Import results publish only while their import token and target tab are current and
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
  Owned link-group metadata counts separately too; links and linked movements
  preserve saved-revision and one-step undo/redo behavior.
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
  buttons. Move's Snapping disclosure starts collapsed, retains its persistent
  state, shows the active snap modes while folded, and never changes settings
  when folded. Tool options use their natural height up to half the inspector,
  scrolling independently from Layers/Paths in the remaining space.
  The right inspector is 272 px wide to accommodate visible per-row lock controls.
  The resizable toolbox starts at the same total width, sharing the inspector
  constant. Its outer container owns that width; the resize grip overlays its
  inside edge instead of adding a separate layout column.
  The Layer menu has separate proportional Fit Layer to Canvas, crop/pad Layer
  to Image Size, and content-crop commands.
  Pointer Move computes snapping from the unsnapped grab on every event, including
  final release and Control changes; it never accumulates snapped deltas.
  `src/guides.rs` owns bounded guide metadata, image transforms, grid density, and
  deterministic snapping. `src/workspace/guide_ui.rs` owns rulers, guide hit tests,
  and reversible guide drafts scoped to tab/revision. Guide commits refresh no
  pixels/thumbnails; per-tab pending guides bridge asynchronous publication so
  release does not flash the old position. History accounts for owned guide data.
  `src/workspace/dialogs.rs` renders the canvas dimensions, color, JPEG quality,
  layer, adjustment, and filter dialogs. Two-choice dialog groups reuse
  `view::segmented` and `choice_button`, including PDF choices, New Image
  background, and Before/After comparison. Navigation buttons share the form
  action styling and put chevrons before Previous and after Next.
  `src/workspace/color.rs`
  handles the HSV wheel and brightness slider.
  `src/color_memory.rs` owns bounded RGB memory and legacy `colors.json` reads.
  `src/settings.rs` owns the centralized version-1 `settings.json`, a 128 KiB
  read/write limit, validated preference patches, legacy color import, advisory
  locking, and atomic replacement. Invalid/unknown/newer files remain untouched.
  `src/workspace/settings_ui.rs::Preferences` is the sole owner of personal tool
  and workspace preferences; Workspace/Tab retain document data, active gestures,
  render buffers, clone sources, and other transient state. The preferences cover
  all per-tool brushes/smoothing, selection/crop choices, region matching and
  sampling, Clone flags, Scale lock/anchor/resampling, the current gradient and
  library selection/disclosure, text defaults, Path handle/stroke choices,
  successful effect parameters/last effect, PDF/JPEG preferences, overlays,
  snapping, active tool, inspector tab, and toolbox width/folding. Named gradients
  stay in their separate library. Effect job order, pending document contexts,
  selection masks, document coordinates, and transform drafts are never saved.
  Settings load/save uses background jobs and 250 ms coalescing. Early/in-flight
  edits replay over loads; writes merge only changed preference fields (including per-tool brush fields)
  and RGB edits over the current disk state. Numeric previews and live toolbox
  widths save only after their normal commit. Failed writes retain queued values,
  report an error, and retry on the next preference change. Close/Quit flushes
  outstanding settings before normal unsaved-image handling. Preference changes
  never dirty a document or enter its undo history.
  `src/workspace/tool_reset.rs` owns current/all-tool reset scopes and availability.
  Tool Options has a reset arrow; Tools has Reset Current Tool and confirmed
  Reset All Tool Settings. Shared tool options reset together (geometric ratio/AA,
  creation feathering, transform resampling, outline brush); Move includes snap
  flags and grid spacing. Selected tool, layout/disclosures/overlays, colors and
  recents, gradient library, PDF/JPEG choices, and effect memory stay intact.
  Gradient's current palette/selection resets, without writing its library.
  Resets are disabled during dialogs, settings load, document jobs, numeric/text
  editing, gestures, and pending previews. The header consumes mouse-down even
  while disabled so it cannot commit a field through the root handler. Reset
  changes only preferences, uses Clone's existing sampling/alignment invalidation,
  and flushes settings immediately. It never commits, cancels, or applies an image
  edit; existing selections/text/layers and their history remain untouched.
  `src/workspace/dialogs/pdf_input.rs` maps PDF input pointer positions through
  the current shaped font, field padding, and horizontal scroll offset. The modal
  owns the drag through mouse-up so selection can extend beyond the field. UTF-8
  glyph indices map to character boundaries before updating Field's range anchor.
  Field renders the selected span and tracks the appropriate caret child while
  scrolling. Pointer click replaces the prior select-all behavior in PDF inputs.
- `src/workspace/numeric.rs` edits the selection radius, selection position/size, crop
  fields, separate wand/fill tolerance and gap radius, brush size/spacing/hardness/opacity/flow, path
  stroke size, selection/crop aspect ratios, Move position, Scale Layer dimensions,
  Rotate angle/pivot coordinates, Perspective corner coordinates, and text size/letter/line
  spacing inline with a caret and text selection. Values preview live; selection
  and Move edits commit one history entry. Scale and warp fields retain Apply/Cancel.
  Empty text and Escape restore the value from before editing. Letter spacing
  accepts negative values. A shortcut or a switch to another numeric field waits
  for a selection or Move edit to settle and its action bindings to render; stale
  followups are rejected after a tab, tool, or revision change. Transform fields
  accept signed decimals for angles, pivots, and Perspective corners; other
  numeric fields retain their existing contracts. Pivot and corner fields
  restore exact tenths on empty text/Escape. Apply and canvas drags finish a numeric edit first.
  Fit Layer to Canvas waits for any numeric image/selection edit to publish;
  deferred fits validate the tab, revision, tool, and original active layer before
  acting. Its option button stops the generic mouse-down commit so the action
  owns that deferred handoff.
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

`.github/workflows/macos.yml` runs formatting, Clippy including test targets, the
full suite, release rendering/stroke/area-preview benchmarks, the release app
bundle/signature and arm64 checks, and the bundled executable's headless PDF
checks on macos-15. Branch/PR/manual runs and v* tag pushes are supported. The app
artifact is first archived with ditto to preserve executable bits and bundle
metadata; CI never launches GPUI or Finder and does not create the Finder DMG.

`.github/workflows/pdf.yml` runs PDF tests and the actual non-test executable's
headless worker on macOS and Windows. `scripts/check-pdf-worker.py --binary
target/debug/lime` (or `lime.exe` on Windows) checks inspection, page selection,
DPI, transparent/white rendering, bounded thumbnails, and rejected sources and
requests without initializing GPUI. The unit suite retains kill/reap cancellation
and deadline coverage. These checks require no interactive UI testing.

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


## Capability pass 26 contracts

- `src/document/composite.rs` preserves the original high-precision Normal path
  and adds alpha-aware Multiply/Screen/Overlay. Blend colors contribute where
  source and backdrop overlap. Avoid replacing this with RGB blending followed
  by ordinary source-over: partial backdrop alpha would be wrong.
- `src/workspace/composite_preview.rs` retains the immediate normal-stack sprite
  path, and coalesces one background full-stack preview for backdrop-dependent
  stacks. Its key uses the GPUI image ID, tab generation, target, offset, and size.
  Stale/canceled images are dropped. Complex stacks skip unused full-resolution
  below/above split composites before the bounded frame is ready. Canvas previews
  cap at 1024 pixels per edge;
  scale sampling uses pixel centers. Warp previews bake transformed masks before
  compositing; other sprite previews sample their linked mask in local coordinates.
  Linked/attached Move previews use the same coalesced bounded full-stack compositor even
  for Normal stacks, translating an immutable content snapshot so intervening
  unlinked layers, masks, blend modes, and opacity retain their stacking behavior.
- `RasterLayer.mask` stores optional same-size straight RGBA coverage in alpha.
  Coverage multiplies source alpha before layer opacity/blending. Existing
  selection tools create/edit masks through the Layer menu; there is no separate
  paint-mask target. Disabled masks retain storage. All linked geometry transforms
  map masks alongside pixels, including uniform-source warps whose mask alone
  changes. Layer to Image Size uses replacement admission for both pixels and
  masks, subtracting retained data that will be baked. Adjustment/filter preview
  composition applies enabled masks to every stack layer, just like commits.
  Copy/shadow silhouettes honor enabled masks. Text masks are rejected.
- `src/document/layer_effect.rs` retains one adjustment or Blur/Sharpen source
  and optional layer-local coverage bitmap. Pixels cache the applied appearance.
  Re-edit previews use source/coverage and captured alpha-lock behavior; Cancel
  never changes the cached layer. Identity re-edits restore the source. Source,
  coverage, mask, and cached pixels count in project/stack/history budgets. Owned
  retained metadata counts per history entry. Pixel/geometry writes bake the
  retained effect, including warps that change only a mask;
  position/opacity/direct mask edits retain it. New effects bake the
  previous result. Drop Shadow remains a separate raster-layer operation.
- `TextLayout` stores optional fixed Width/Height and Left/Center/Right alignment.
  Fixed boxes wrap by word or glyph, clip raster/caret/highlight coverage, retain
  overflowing source, and expose an overflow notice. Draft history, defaults,
  previews, matches, and version 9 projects carry it. Caret maps retain visual
  wrap boundaries; glyph rasterization only visits visible fixed-box lines.
  Mixed spans remain a recorded follow-up, not a partially persisted rich-text API.
- Feathered simple selections retain source geometry in `Mask`. Move/resize
  previews use the source shape, then rebuild feathering in the change worker.
  A clipped source curve retains its original box. Combination/morphology/image
  transforms or repeated feathering may deliberately drop editable geometry.
  Wand color-edge reconstruction only adds bounded adjacent fractional coverage
  when a premultiplied RGBA color segment supports it; it never expands traversal
  and respects gap/selection exclusions.
- `src/pdf/worker.rs` handles a private CLI mode before GPUI initialization, so it
  never opens a window. Parent/child communicate through a private temporary
  directory, bounded JSON, and validated raw RGBA pages. Source bytes are the
  retained import snapshot. Cancellation/deadlines kill and reap the child. It
  emits pages individually and does not retain the whole output stack. Unix
  workers have CPU/file limits; macOS additionally uses a resident high-water
  watchdog (50 ms, 2 GiB), other Unix uses an address-space limit, and Windows
  uses a private job with process-memory/time limits. This is resource isolation,
  not an OS security sandbox. Worker errors never publish partial imports.
  Preparation checks cancellation inside compositing (32-row batches), between
  thumbnail/tile operations, and before publication. Tiles reuse the flattened
  image rather than recompositing the stack. Bounded preview/thumbnail resizes
  finish their current call before the next cancellation check.
- `src/recovery.rs` and `src/workspace/recovery_ui.rs` own five-second separate
  atomic recovery projects, per-session file leases, clean-exit markers, bounded
  admission, and stale-session recovery. Startup admission uses a conservative
  512 MiB estimate before decoding any PNG payload: decoded storage, full canvas
  and preview, both fine-tile representations with aprons, thumbnails, metadata,
  and encoded input as decoding scratch. Skipped files stay on disk. Snapshot
  tracking is per tab and persistent revision plus text source/style/placement;
  selection/caret changes do not rewrite copies. Batch failures stay local to
  their tab, and stale lease results cannot mark a new lease up to date.
  Original paths and saved revisions stay
  unchanged. Accepted close/discard cleans the session; unreadable/excess old
  copies stay on disk. Text drafts are rendered into recovery candidates without
  committing the live tab. Failed text rendering preserves a JSON source sidecar
  and a recoverable draft. The name/draft are also embedded in the atomic version
  9 project manifest; sidecars are readable copies, never authoritative over that
  snapshot. Other unapplied previews are not snapshotted. Recovery
  errors are surfaced and never trigger a write to an original document.
- `src/color_management.rs` uses the already-present moxcms 0.8.1 as a direct
  dependency. External RGB/grayscale ICC data is bounded and converted to the
  sRGB 8-bit working space, preserving alpha. Unsupported/corrupt profiles fail
  explicitly. Project PNG payloads are already working-space pixels and bypass
  import conversion. Export embeds sRGB. macOS tags its Metal surface sRGB via
  the main-thread AppKit view; Windows retains standard SDR display behavior,
  with per-monitor verification outstanding. Do not advertise arbitrary working
  spaces, soft proofing, CMYK ICC import, or high-precision editing.
- `fixtures/pdf/` contains self-authored CropBox/rotation, Helvetica/embedded Type 3,
  transparency, image, malformed, and encrypted inputs. These tests are in the
  shared macOS/Windows suite. Windows execution/native display QA remains for CI
  and the maintainer. No interactive app launch is part of this pass's validation.
