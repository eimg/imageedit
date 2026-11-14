# Lime

<img src="macos/AppIcon.svg" width="96" height="96" alt="Lime half-slice logo">

**Lite Image Editor** for macOS and Windows. Layered editing, native menus with
tool and command icons, and a workspace that keeps the canvas in focus.

Current source version: **1.2.0**.

![Lime editing a night skyline, with its toolbox, text controls, and Paths panel](lime-screenshot.jpg)

## Features

- **Layers** — move, align, reorder, merge, and adjust opacity.
  Alpha lock preserves transparency while recoloring; position lock protects
  layers from manual moves and transforms.
  The opacity slider keeps your chosen value while the canvas updates; each drag
  is one undo step. You can keep dragging outside the slider and release at the
  desired position. Pending text commits before the opacity change.
- **Selections and transforms** — rectangle, oval, freehand, and magic wand;
  crop, resize, rotate, flip, and perspective.
  Grow/Shrink retain feathered edges. The wand can sample the visible layers.
  Rectangle and oval selections have exact position/size controls and optional
  smooth curved edges. Selections and Crop support fixed aspect ratios.
  Scale Layer has editable dimensions, aspect locking, and resampling choices;
  Rotate accepts typed angles.
- **Painting and retouching** — pencil, brush, eraser, clone stamp, gradients,
  and fills.
  Paint and retouch tools remember separate brush settings for the session.
  Opacity limits each stroke; optional Buildup and Flow control gradual coverage.
- **Text and paths** — editable text, searchable fonts, bold and italic,
  plus paths and strokes.
- **Color and effects** — brightness, contrast, hue, saturation, levels,
  curves, blur, sharpen, and shadows.
  Dialog previews show the canvas composition, with Before/After and Reset.

## Download

**Lime 1.0.1** · [Release notes](https://github.com/eimg/lime/releases/tag/v1.0.1)

| Platform | Download | Install |
| --- | --- | --- |
| macOS (Apple silicon) | [DMG](https://github.com/eimg/lime/releases/download/v1.0.1/Lime-1.0.1.dmg) | Open the disk image and drag Lime to Applications. |
| Windows x64 | [EXE](https://github.com/eimg/lime/releases/download/v1.0.1/Lime-1.0.1-windows-x64.exe) | Download and run. No installer. |

Windows needs Windows 10 or later, DirectX, and the Microsoft Visual C++
2015–2022 x64 runtime if it is not already installed. Windows builds are unsigned;
macOS builds are ad hoc signed and not notarized.

The macOS disk image opens an installation window with Lime on the left and
Applications on the right. Drag Lime along the arrow into Applications, then
open the copied app from there. Eject the disk image after installation.

Lime quietly checks for a new stable release about once a day. When one is
available, a badge appears at the right of the status bar. Click it for the
changelog and a direct download for your platform. **Check for Updates…** is
also available in the Lime menu on macOS and the Help menu on Windows.
Downloads open in your browser; you choose when to download and install them.
Lime never installs updates automatically. Offline checks stay quiet; a manual
check offers a retry and a link to the releases page if it cannot connect.

## Getting started

Open a PNG, JPEG, WebP, or PDF, drop one into the workspace, or create a new image.
PDF files open a wider import dialog with settings beside page thumbnails (stacked
in narrower windows). Click thumbnails to select or deselect pages. **All** selects
the whole PDF; **None** clears the selection. Two-choice controls use the same
segmented picker as Tool Options. The page field also accepts individual
pages and inclusive ranges, such as `1, 3-5, 8` (en/em dashes work). Overlapping
entries import once, in document order. Thumbnail changes update compact ranges
in that field. An empty selection keeps Import disabled.

Click within either input to place the caret, drag or Shift-click to select text,
or double-click to select the whole value. Shift+arrows/Home/End extend a range;
copy, cut, paste, and typing use that range. Long page lists scroll with the caret.

Thumbnails load automatically in batches of three. Previous/Next have directional
chevrons and browse the source;
Selected brings the first chosen page into view. A check mark and accent border
show included pages. Previews use the chosen page background and a checker behind
transparency; import DPI controls the final images. Failed previews can be retried.
The dialog keeps Cancel/Import visible while its content scrolls. Page choices
reset per source, and layer names/progress retain source page numbers.

**Page background** can be White (the default) or Transparent. White produces fully
opaque pages and thumbnails, including antialiased edges. Transparent keeps unpainted page
areas clear and preserves PDF opacity; white shapes painted in the PDF stay
white. For a new layered image, **Canvas size** offers First Page (the first
selected page) or **Fit All Pages**, which fits the greatest selected width and height. Every page stays at
its rendered size and is centered. The dialog shows the canvas size and warns
when selected pages extend outside it; those layer pixels are retained.
Separate images keep each page's own size, and Open as Layers keeps the existing
canvas size. Successful imports remember DPI and page background for the session;
new-document imports also remember Layers/Images and canvas sizing. Canceling
or failing an import leaves those defaults unchanged. Page choices reset for each
file. A PDF import becomes a new unsaved Lime project; use Export as PDF to write a flattened copy. Encrypted PDFs are
not supported. Open as Layers accepts one PDF at a time. Multiple PDFs opened
together keep their import dialogs queued. Pages are rasterized at the chosen
DPI; PDF text and vector objects do not become editable text or paths.
Reading, inspection, rendering, and image preparation show progress. Use Cancel
or Escape to discard the current import. Inspection and rendering run in a
separate worker process that can be terminated during a parser or page call.
Preparation checks cancellation inside compositing and between tiles; a bounded
preview resize can finish before cancellation settles. No partial result is added.
Canceling one file leaves later queued files available to open.

Each PDF is inspected when its turn opens, and its bytes are retained for that
import. Replacing or deleting the original file afterward does not change the
pages being imported. Open as Layers checks the whole selected page set against
the destination's remaining layer and memory capacity before rendering. If it
does not fit, select fewer pages or reduce the DPI; a successful import adds all selected
pages in one undo step.
Pick a tool on the left and adjust its settings on the right. Hover the **?**
beside a tool name for gestures and shortcuts.

Save as **`.lime`** to keep layers, editable text, and paths. Use **Export** for
a flattened PNG, JPEG, WebP, or single-page PDF copy. Apply or cancel an active preview before saving.
Finish or cancel an unfinished free selection first. If an image gains layers
while the save dialog is open, save it as a `.lime` project or export a copy.
Smooth scaling keeps the colors of nearly transparent pixels intact; show a
hidden layer before dragging its Scale Layer frame.
Reduced previews and layer thumbnails also preserve transparent edge colors.
Save and Undo shortcuts finish an active numeric selection edit first.

**File → Export as PDF…** places the entire current canvas on one PDF page,
with visible layers and text flattened into a lossless image. Transparent areas
become white, and no margins or paper-size fitting are added. Resolution defaults
to **300 DPI** and is remembered for the session; changing it changes the physical
page size shown in the dialog without resizing the canvas pixels. Enter 1–1200 DPI;
large canvases need a higher minimum to keep each page edge within 200 inches.
Export leaves the project's layers, history, and saved state unchanged. PDF is
export-only, so Save/Save As still use the existing project and raster formats.
Typed geometry and text finish before the export dialog opens; apply or cancel
other canvas previews first.
Multipage output, searchable text, and vector PDF export are not supported.

With **Scale Layer** (Shift+S), type Width or Height to preview a resize.
**Size anchor** chooses which of nine points stays fixed during numeric resizing;
it starts at Center. Choosing an anchor alone does not resize the layer. Border
and corner drags keep their opposite edge or corner, independent of this choice.
Dragging and typed dimensions stay in a preview. **Apply** (or Return on the
canvas) commits all adjustments as one undo step; **Cancel** (or Escape on the
canvas) discards them. Apply and Cancel dismiss the scale frame and editing
controls, just like Flip and Perspective. Click the canvas or select Scale Layer
again to start another preview. Return or leaving a size field finishes that field;
Escape or empty text restores the size from before that field's edit without
discarding earlier adjustments. Aspect locking starts on; Shift temporarily reverses the lock
while dragging. Choose Nearest, Bilinear, or Bicubic for the committed result.
**Fit Layer to Canvas** in these options or the Layer menu scales the whole layer
proportionally to fit inside the canvas and centers it. This works even when its
resize handles are off-screen. It uses the chosen resampling, may enlarge smaller
layers, and previews the fit while Scale Layer is active; use Apply to commit.
From other tools the Layer menu command commits one undo step immediately.
The layer must be visible, rasterized, and
position-unlocked. The canvas and selection stay unchanged. **Layer to Image
Size** is a separate command that crops/pads the layer at its existing pixel
positions; it does not proportionally scale the image.

In **Rotate**, enter an angle in degrees (including negative values and tenths),
then use Apply after finishing the numeric edit. **Pivot** offers nine anchors
on the original layer, signed canvas X/Y coordinates in tenths of an image pixel,
and a draggable circular crosshair. Drag elsewhere to rotate around that point;
Shift snaps the angle to 15-degree steps. Changing the pivot recalculates the
original layer at the current angle. Center restores the existing centered
rotation, including lossless pixel placement at quarter turns. Fractional custom
pivots use the selected resampling when their rotated pixel grid is fractional.
Pivot choices alone at zero angle do not edit pixels or add history. Empty
numeric text or Escape restores that field; Escape on the canvas cancels the
whole preview. A new rotation session starts centered. Applying a changed result
creates one undo step while the canvas and selection keep their coordinates.
**Perspective** offers a corner selector and signed canvas X/Y coordinates in
tenths of a pixel. Dragging a grip selects it; its larger accented outline keeps
a dark rim for contrast. Reset Corner restores that point to its original layer
corner, and Reset All restores the original rectangle. Both change the preview
only. Empty text or Escape restores the active field. Crossed, concave, or
oversized results show an error and cannot be applied until corrected; Apply
commits one undo step and leaves the selection in canvas coordinates.
Scale, Rotate, Flip, Perspective, Crop, and geometric selections share white
square grips with a dark rim and a fixed screen size. Scale uses the same
contrasting frame as the other transforms; Perspective accents the selected
corner. Crop and selection retain their own boundary outlines.

**Sample All Layers** on the Magic Wand samples the visible composite and can
select beyond the active layer.

**Rectangle Select** and **Oval Select** accept X/Y and Width/Height for an
editable geometric selection. Return or leaving the field commits one selection
undo step; Escape or empty text restores it. Selection changes keep the image's
saved state. **Antialias edges** optionally smooths oval/circle and rounded
rectangle edges; it starts off for hard pixel selections. Toggling it also
updates the current editable curve. Combined, clipped, feathered, and wand masks
keep their existing coverage when this setting changes.

**Free Select** combines freehand segments and click-added polygon vertices.
While its outline is open, drag any square checkpoint to adjust it. Checkpoints
mark polygon vertices and the start/end of freehand segments; sampled curve
points stay uncluttered. Click the first checkpoint or press Return to close;
Escape cancels the draft. Node adjustments join the selection's single undo step
when it closes. A closed free selection is a mask and has no editable nodes.

**Free Select** has its own **Antialias edges** option for new freehand/polygon
selections. It also starts off; enabling it preserves fractional edge coverage
when the outline closes, without blurring aligned pixel edges.

Rectangle Select, Oval Select, Free Select, and Magic Wand offer **Feather new
selections**, with a shared radius of 0–128 image pixels (initially 8). Feathering
starts off. These settings affect new selections when their gesture finishes;
they leave an existing selection unchanged. A radius of zero skips feathering.
Feathering happens before add/subtract/intersect, so repeated additions do not
soften the old selection again. Use **Select → Feather…** to soften the current
selection. A feathered rectangle, ellipse, or rounded rectangle retains its
source box for moving, resizing, and numeric geometry edits. The drag shows its
source shape; release rebuilds the soft coverage in the background. Combining,
growing, shrinking, transforming, or repeatedly feathering a mask can rasterize
that geometry. Arbitrary masks keep their existing coverage-only behavior.

Feathering treats space outside the canvas as unselected. A selection touching
the border softens there too, including Select All. Coverage remains inside the
canvas. Hard-edge and unfeathered workflows keep their existing behavior.

Selection and **Crop** offer Free, 1:1, 4:3, 16:9, and Image aspect ratios.
Choose a fixed preset and edit Ratio W/H for a custom ratio. It constrains the
next creation drag, handle resize, or Width/Height edit; choosing a ratio alone
leaves the current box in place. Circle takes priority over the oval ratio.
Dimensions round to the nearest image pixel and stay within the available
canvas space when resizing or typing. A side handle keeps the opposite edge
and centers the other axis; a corner keeps its opposite corner. Fixed-ratio
handles stop at the minimum size when dragged past their opposite edge.

**Move** accepts signed X/Y coordinates with a live preview. Return or leaving
the field commits; Escape or empty text restores the position. Arrow keys move
one image pixel, or ten with Shift, regardless of zoom. Releasing an arrow commits
one undo step for that key sequence. Hold Shift while dragging **Rotate** to snap
to 15-degree angles.

Each Layers row has **Lock Alpha** and **Lock Position** buttons beside its
visibility button. Accent-colored closed locks are on; dimmed open locks are off. Like the
visibility control, these icons have no persistent button background. Hover for
their name and effect. These controls affect that row's layer without changing the
active layer. The Layer menu toggles locks for the active layer.
Alpha lock preserves each pixel's transparency through painting,
clone strokes, fills, gradients, adjustments, and filters. Fully transparent
pixels keep their stored colors. Eraser, Cut, and Clear cannot remove pixels
while alpha is locked. Rasterize editable text before enabling alpha lock.

Position lock blocks manual layer moves, numeric positions, arrow nudges,
alignment, scaling, rotation, flipping, perspective, and layer fit/crop commands.
Image-wide crop, scale, rotation, flip, and canvas resizing still transform the
document together. Text remains editable at its original anchor; its bitmap can
resize as text or styles change. Opacity, visibility, layer order, and deletion
remain available. Unlock participating layers before Merge Down. Locks undo/redo
and are retained when a layer is duplicated.

Save locked layers in **`.lime`** projects. Lock-bearing projects use at least format
version 8 and require this build or newer. Export still makes flattened raster
copies; ordinary raster Save cannot discard the lock settings.

**Clone Stamp** starts with Aligned enabled, preserving the source offset across
strokes. Turn it off to start each stroke at the chosen source. **Sample All
Layers** reads the visible canvas, including other layers' opacity and rendered
text; otherwise the active raster layer supplies the samples. With merged
sampling you can Alt-click a source outside the destination layer's bounds.
The destination is still the active visible raster layer, inside its existing
bounds and the current selection. Each stroke samples the pixels as they looked
when it started, so its own paint cannot feed back into its source.

**Show Source** starts on and displays a blue dashed brush ring and crosshair at
the sampled source position. It follows aligned movement, follows the source
during an unaligned stroke, and resets to the chosen point between unaligned
strokes. The indicator tracks zoom/pan and hides outside the sampled image.
Alt still shows the source-picking cursor. Alt-click to set a source again after
changing sampling, tabs/layers, or layer/canvas geometry. Sampling and alignment
cannot change during an active stroke. These settings are independent of the
Wand and Bucket Fill sample settings.

Wand and connected Fill treat fully transparent pixels as the same color even
when their stored RGB differs; other alpha and tolerance comparisons stay in
effect. Each tool remembers its own **Tolerance** and **Gap radius** for the
session. Tolerance 0 matches exactly. Gap radius starts at 0 (off); increase it
up to 8 image pixels to close small breaks in line-art boundaries before finding
the connected region. Larger radii can also seal narrow passages or small
regions, so start low. The boundary mask is temporary; the artwork is unchanged
until you fill, and original nonmatching pixels are never included in the fill.

Magic Wand has an independent **Antialias edges** option, initially off. It
smooths the contour of each new connected selection with fractional edge coverage;
straight boundaries and uniform interiors stay sharp. Color matching and gap
closure find the connected region first, so smoothing cannot spread the flood
into another region. Sealed gap barriers stay excluded. Small corners and very
thin features can receive partial coverage; turn it off for exact pixel edges.
For antialiased artwork, a one-pixel color/alpha reconstruction can recover
partial boundary pixels between an admitted neighbor and the next rejected
color. It uses premultiplied RGBA, so transparent hidden colors cannot bias it;
gap barriers and selection limits stay strict. Changing the option leaves the
current selection and history unchanged.

Wand applies new-selection feathering after contour antialiasing, then combines
the new region with the existing selection. Bucket's Selection mode fills the whole
selection and hides the connected-only tolerance, gap, and sampling controls;
their remembered values return when you choose Connected.

For **Pencil**, **Brush**, **Eraser**, and **Clone Stamp**, **Opacity** limits the
effect of one stroke, including where its dabs overlap. Release and paint again
to build up another stroke. **Buildup** starts off; enabling it reveals **Flow**,
the coverage added by each dab toward the opacity limit. Lower flow builds more
gradually as you paint over an area. Spacing controls the distance between dabs;
pointer event frequency does not change the result along the same path. Soft
selection edges and the paint color's alpha still limit coverage.

**Blur** and **Sharpen** show **Strength** instead of Opacity. One stroke blends
toward a local filtered result from the pixels at its start. Overlapping dabs
respect that strength; Buildup and Flow optionally approach it gradually. A new
stroke applies another filter pass. Each paint and retouch tool remembers its
own flow and buildup setting during the session. Path and selection outline
strokes also limit overlapping dabs to their stroke opacity.

Brush, Eraser, Blur, Sharpen, and Clone Stamp offer **Smoothing: Off / Light /
Strong** in a full-width switch group directly below a regular Smoothing label. Off starts selected and
follows the pointer directly. Light steadies small shakes; Strong gives a steadier trace with more lag. When the pointer
stops, the trace catches up within about 64 ms for Light or 128 ms for Strong.
Release reaches the exact pointer endpoint immediately and commits the whole
stroke as one undo step. Escape discards the stroke and its remaining tail.
Each tool remembers its own smoothing choice for the session. Pencil stays
unsmoothed for precise pixel placement. Shift-click straight connectors and
path/selection outline strokes keep their existing geometry. The Clone source
indicator follows the position actually being painted while smoothing is active.

With **Path** (`P`), click to place anchors or drag to pull their handles. Click
the first anchor to close a path. **Alt-click a visible curve** to insert an
anchor while preserving the curve; you can drag the newly inserted point before
releasing. Existing anchors and handles take priority over curve insertion.
Escape cancels an unfinished gesture and ends the current path session.

**Shift-click anchors** to add or remove them from a selection on one path.
Drag any selected anchor to move the group with its handles; clicking an
unselected anchor starts a new selection. Shift-click selects without dragging
or closing the path. **All Anchors** and **Deselect** are in Tool Options, with
a selected count. Every selected anchor has a larger outlined marker. **Corner**
collapses their handles. **Smooth** aligns them while retaining existing lengths,
or creates handles toward neighboring points. Delete/Backspace removes the
selected anchors in one step; with none selected it removes the last anchor.
Clicking the first anchor closes an open path only when it is selected alone.
**Handle Drag** controls future handle edits:
Independent moves only the chosen handle, Aligned keeps the opposite handle's
length while aligning it, and Symmetric mirrors both direction and length.
Independent starts selected; newly drawn handles still start symmetric. These
drag preferences are session settings; the resulting geometry is saved with
the path. Each group drag or shaping/deletion command is one undo step and leaves
raster pixels unchanged. Anchor selection is a session setting and clears when
switching paths/tabs/tools, hiding/deleting its path, or stepping image history.

**Gradient** supports Linear and Radial fills. Drag its line, adjust the endpoints,
then Apply/Return or Cancel/Escape. **Reverse** reverses the colors without moving
the line or changing a radial gradient's center. FG → BG and FG → Clear follow
the foreground/background swatches.

Choose **Custom** to edit up to eight color stops. Select a stop's colored chip,
use **Color…** for the color picker, and type **Opacity %**. Intermediate stops
also offer **Position %** in tenths of a percent. Positions stay between their
neighbors; the two endpoints stay at 0% and 100% and cannot be removed. **Add
Stop** splits the largest gap using its current interpolated color. Switching
back to a swatch mode keeps the custom palette available for later.
Transparent stops blend without letting hidden RGB tint visible edges.

**Save…** names the current colors, shape, and Reverse setting in a personal
library of up to 32 gradients. Names can contain up to 64 characters and must
be unique ignoring case. **Saved gradients** folds away; expand it to select a
named gradient from a scrollable list. Loading uses literal colors, including
opacity, and leaves the swatches and current canvas endpoints in place. Rename
or Delete manages the selected library entry; deleting keeps the current gradient
available to paint or save again. Library entries survive app restarts.

The library lives in `~/Library/Application Support/com.eimg.lime/gradients.json`
on macOS and `%LOCALAPPDATA%\Lime\gradients.json` on Windows. Reads and writes run
in the background, and library changes appear after a successful save. Close/Quit
waits for an active library write before checking unsaved images. A write failure
keeps the window open and reports the error. Unreadable, unsupported-version, or
externally changed libraries are protected from overwrite; use **Reload Library**
after resolving the error. Library data is separate from projects and image undo.
Stop edits and loading a saved gradient update the preview without image history;
Apply commits the selected area in one undo step and honors alpha lock and the
image's color mode. Cancel leaves pixels unchanged.

Adjustment and filter dialogs preview the whole canvas composition, including
layer order, offsets, opacity, and the selection's soft coverage. A checker
shows transparency. **Before** shows the original composition; **After** shows
the chosen effect. Switching views does not change parameters, and Apply always
uses those parameters. **Reset** restores the built-in defaults and
returns to After. Cancel leaves the document unchanged. Colors adjustment dialogs share a
wider layout with the preview beside the settings when space allows, and stack
on narrow windows. Levels pairs its black/white input and output controls to
reduce height. The dialog title and action buttons stay visible while settings
scroll in shorter windows. Preview resolution follows the image box and screen
scaling, including Retina displays. Source detail is area-averaged before shrinking
so thin patterns do not disappear through sparse sampling. Transparent edges use
premultiplied color to avoid fringes; reduced filter previews remain approximations. Apply and export
continue to use the original image resolution.

Each adjustment and filter remembers its last successfully applied parameters
for this app session, shared across tabs. Canceled, failed, and unchanged edits
leave the previous settings in place. Reset changes the current preview without
overwriting those settings until a changed result is applied.
Drop Shadow starts with a black shadow and remembers its last applied color.
Its wider dialog places the preview beside the controls when space allows.
**Choose Color…** opens the color picker; **Use Foreground** copies the current
foreground color. Reset restores black and the built-in shadow parameters.

**Filters → Repeat Last Effect** applies the last successful adjustment or
filter to the current layer using the current selection, without a dialog
(Cmd/Ctrl+F). **Reopen Last Effect…** opens its remembered settings with a preview
(Cmd/Ctrl+Shift+F). Repeat creates one undo step when it changes pixels or adds a
shadow layer. Both commands require a visible raster layer and wait for typed
geometry to commit; unfinished canvas previews must be applied or canceled first.
Shadow repeats use the whole layer silhouette, as they do in the dialog, and
check the destination's layer budget before applying.

These fitted previews are capped at 1024 pixels on their longest side and never
upscale the source. Filters
keep nearby off-canvas pixels that can influence the visible result, and shadows
preview beneath their source just as they will be inserted. Small filter radii
can disappear at this reduced scale; Apply always processes the full resolution.
“Updating preview…” identifies a pending slider result while the last preview
remains visible.

Use **View → Toggle Grid** for a dotted grid anchored to image pixels, with
contrasting dots and a default spacing of 16 image pixels.
**View → Toggle Rulers and Guides** shows pixel rulers. With **Move**, drag from
the top ruler for a horizontal guide or from the left ruler for a vertical one.
Drag an existing guide to reposition it; drag back onto a ruler or outside the
canvas workspace to remove it. Escape cancels the drag. Guide edits undo/redo,
and guides follow image crop, resize, scale, rotate, and flip.

Expand Move's **Snapping** section for **Snap to Grid**, **Snap to Guides**, and
grid spacing. It starts collapsed and remembers its open/closed state during the
session. The header shows which snapping options are on; folding it keeps those
settings. Compact Move options leave the remaining sidebar space for Layers.
Both sidebars start at the same width, with the toolbox's resize grip inside its
edge; the toolbox remains resizable.
The snapping controls work independently of
overlay visibility. They snap layer edges and centers within a small screen
distance. Hold Control while dragging to bypass snapping and guide grabs.
Typed positions and arrow nudges remain exact. Grid spacing is in image pixels;
dots thin out at low zoom while snapping keeps the chosen spacing.

Guides are stored in **`.lime`** projects, never flattened exports. A project
containing guides uses format version 7 when its layers are unlocked. Existing
version 6/7 projects still open; unlocked projects without guides retain version
6 on save. Lock-only projects use version 8, with or without guides. Blend modes,
masks, retained effects, or text-box layout use version 9 and require this build
or newer. The reader keeps opening versions 6–8.

In **Text**, drag over characters, Shift-click, or use Shift+arrows to select a
range. Copy/Cut use that range; typing and Paste replace it. Selection highlights
follow individual lines. Option+Left/Right and Option+Backspace/Delete on macOS
(Ctrl on Windows) move or delete by word; Shift extends word selections. Normal
arrows and deletion keep combining characters and emoji sequences together.
A caret at a wrapped line end stays on that visual side of the boundary when
clicked or reached with Home/End, arrows, and typing Undo/Redo.
Font/style controls still affect the entire text layer. Undo/Redo restore typing
and its selection before falling through to document history.

**Fixed box** in Text options enables editable pixel Width/Height, word wrapping,
and Left/Center/Right paragraph alignment. Text outside the height remains in
its source; an overflow notice identifies it. Increase the height to reveal it.
Carets, selection highlights, and Home/End follow wrapped lines and clip to the
box. Turning Fixed box off restores automatic sizing. Font/style controls still
cover the entire layer; mixed styling within a text layer is deferred.

The Layers panel offers **Normal**, **Multiply**, **Screen**, and **Overlay**.
Blend modes honor layer opacity, transparency, and stack order in preview and
export. Move, scale, text, gradient, and transform previews use bounded background
compositing when the stack needs its backdrop. A preview may briefly trail a fast
drag while its latest background frame finishes. Merge Down is disabled when a
backdrop-dependent pair above other layers cannot be merged without changing its
appearance; bottom pairs remain available.

**Layer → Mask → From Selection** creates or replaces a raster layer's mask.
No selection produces a white mask. Use the existing selection tools to define
coverage, then choose From Selection; **Mask to Selection** loads it back for
further editing. The Layer mask toggle enables/disables its coverage. The menu
also offers Invert, Apply, and Remove. Apply permanently combines coverage with
pixel alpha; unlock alpha first. Remove keeps the source pixels. Masks follow
layer movement, scaling, crop, rotation, flip, and perspective, and save in a
project. Adjustment and filter previews include enabled masks throughout the stack.
Layer to Image Size checks space for both the resized pixels and their mask.
Rasterize text before adding a raster mask. Painting directly into a
mask is a future extension; painting currently edits the layer beneath its mask.

Applying a color adjustment, Blur, or Sharpen retains **one editable effect per
raster layer**, including its source and original layer-local selection coverage.
Use **Edit effect** or **Layer → Edit Layer Effect…** to replace its parameters
with a preview. Cancel preserves the applied result. Remove restores its source;
Bake keeps the result and frees retained source data. A subsequent effect bakes
the previous one, and pixel/geometry edits bake the retained effect before changing
its result, including transforms that change only a mask. Movement, opacity, and
direct mask edits retain it. Undo restores the prior
state. Memory admission includes retained sources and coverage. Drop Shadow still
creates a separate raster layer; its parameters are not retained on that layer.

Crash recovery writes separate project copies under the app's Application Support
**Recovery** folder (on Windows, Local AppData/Lime/Recovery). Dirty documents are
checked about every five seconds and snapshotted only when their persistent
revision or text draft changes, without changing their saved revision or original
destination. Selection-only edits do not rewrite copies. A failed snapshot does
not prevent other tabs from updating. Recovery includes valid text drafts; failed text rendering
also embeds its source in the same atomic project write, keeps a readable
sidecar, and restores its draft when readable. Other
unapplied canvas previews are not recovery snapshots. After an interrupted
session, available copies reopen as unsaved **Recovered…** documents; use Save As.
Saved/closed documents and an accepted clean exit remove their current recovery
copies. A session lock skips another running instance. Recovery writes are atomic,
with a 1 GiB storage limit; startup checks a conservative 512 MiB memory budget
before decoding pixels, including layer storage, canvas/preview/tile buffers,
thumbnails, and decoding scratch space. Excess or unreadable copies remain on disk
with a message. These are recovery copies,
not a long-term backup history.

Raster imports read embedded RGB/grayscale ICC profiles and convert pixels to
sRGB; invalid or unsupported profiles return an error. Untagged images are
assumed sRGB. PNG, JPEG, WebP, and PDF exports embed an sRGB profile. The macOS Metal
surface is tagged sRGB for Core Animation's display conversion. Windows uses the
standard SDR sRGB display path; per-monitor profile behavior still needs Windows
verification. The working pixels remain **8-bit RGBA**. CMYK profiles, selectable
working spaces, soft proofing, and higher-precision editing are not implemented.

| Action | macOS | Windows |
| --- | --- | --- |
| Open / Save | ⌘O / ⌘S | Ctrl+O / Ctrl+S |
| Undo / Redo | ⌘Z / ⌘⇧Z | Ctrl+Z / Ctrl+Y |
| Copy / Cut / Paste | ⌘C / ⌘X / ⌘V | Ctrl+C / Ctrl+X / Ctrl+V |
| Crop / Move / Text | C / M / T | C / M / T |
| Pan | Hold Space and drag | Hold Space and drag |
| Brush size | [ / ] | [ / ] |
