# Lime capability roadmap — local working document

Requested by the maintainer to revisit scope, implementation status, and manual
feedback between passes. The roadmap is now tracked; new pass changes stay
uncommitted until the maintainer requests a commit. GIMP is a
functional reference; its interface is not a design target. Add no toolbox tools.
Existing review fixes and other uncommitted work must remain intact.

Status: `[ ]` planned, `[x]` implemented and automatically checked. Manual testing
is left to the maintainer. A checked item does not imply native interaction QA.

## Pass 1 — selected scope

- [x] Grow/Shrink preserve feathered selection coverage instead of making it binary.
- [x] Scale Layer has editable Width/Height with a live preview and one commit.
- [x] Scale Layer supports an aspect-ratio lock for numeric edits and handle drags.
- [x] Rotate accepts a typed angle while retaining Apply/Cancel and preview behavior.
- [x] Paint/retouch tools remember independent brush settings during the session.
- [x] Magic Wand can sample the visible composite as well as the active layer.
- [x] Update README/AGENTS contracts; pass formatting, Clippy, tests, benchmarks,
      and macOS bundling. Do not run interactive smoke checks.

## Selection quality

- [x] Optional antialiasing for oval/circle and rounded-rectangle edges.
- [x] Optional antialiasing for freehand/polygon selection edges.
- [ ] Optional antialiasing for wand selection edges.
- [x] Optional feathering at selection creation with an explicit radius.
- [x] Preserve soft coverage through Grow/Shrink and combinations.
- [x] Define feathering at canvas boundaries consistently.
- [x] Exact geometric selection position/size controls and fixed-ratio constraints.
- [ ] Retain editable source geometry for feathered geometric selections.

Acceptance: fill, paint, clear, clipboard, adjustments, and filters consume the
same coverage; selection-only changes preserve the saved revision; geometry
clipping retains the original curves; pixel-art hard selections remain available.

## Transform precision

- [x] Editable Scale Layer dimensions, aspect lock, and interpolation options.
- [x] Numeric rotation angles.
- [x] Constrained rotation angle increments while dragging.
- [x] Keyboard layer nudging, with larger modifier steps.
- [x] Typed layer position.
- [ ] Further anchoring/pivot controls for transforms.
- [x] Crop aspect ratios and consistent dimension constraints.

Acceptance: preview and commit agree; Escape cancels drafts; one completed edit
creates one undo entry; tab/layer/tool changes never apply a draft to another
target; dimensions, offsets, and layer-stack budgets remain bounded.

## Grid, rulers, guides, and Move snapping — maintainer feedback

Recorded after Pass 2. The maintainer dislikes the current grid and guide
implementation and requested the replacements below. Implemented in Pass 3;
native interaction and appearance checks remain with the maintainer.

- [x] Replace the current line grid with a subtle dotted grid, using the
      starting/empty canvas dot pattern as the visual reference.
- [x] Replace the fixed guide behavior with canvas rulers from which users can
      drag out and place horizontal or vertical line guides where needed.
- [x] Use the existing Move tool to reposition user-placed guides. Add no new tool.
- [x] Add Snap to Grid for moving layers with the Move tool.
- [x] Add Snap to Guides for moving layers with the Move tool.

Implementation considerations for that pass:

- Keep grid/guide visibility and snapping independently controllable.
- Keep rulers, dots, guide positions, and snap targets consistent through zoom,
  Fit, and pan; avoid dense or visually dominant overlays at reduced zoom.
- Distinguish grabbing a guide from moving the active layer. Provide a clear
  way to remove a guide and retain Escape cancellation for in-progress drags.
- Use a zoom-aware snap distance and deterministic target selection; the
  committed layer position must match its preview and remain one undo step.
- Decide guide persistence, grid spacing, and edge/center snap targets during
  implementation, accounting for project compatibility and existing numeric
  position and keyboard-nudge behavior.

## Painting and retouching

- [x] Independent remembered settings for Brush, Pencil, Eraser, Blur, Sharpen,
      Clone Stamp, and outline strokes.
- [x] Define stroke opacity separately from dab/flow buildup; expose incremental
      behavior deliberately rather than making spacing determine the meaning.
- [ ] Optional stroke smoothing with bounded latency.
- [x] Predictable strength and repeat-pass behavior for Blur/Sharpen.
- [x] Clone aligned/unaligned behavior and scoped source validation.
- [ ] Clone merged sampling and source preview.

Acceptance: Pencil remains hard and pixel-accurate; paint/erase honor soft
selections; pointer sampling density does not change the intended effect;
preview/commit match; no performance regression on sparse dirty-tile strokes.

## Text editing

- [x] Partial range selection using dragging, Shift-click, and Shift+arrows.
- [x] Word navigation and deletion, with Unicode-aware boundaries.
- [ ] Fixed text boxes, wrapping, and paragraph alignment.
- [ ] Consider mixed styles only after ordinary editing is complete.

Acceptance: clipboard, typing history, caret mapping, and project round trips
preserve source and layout; failed commits retain recoverable drafts.

## Region selection and fills

- [x] Wand sampling from active layer or visible composite.
- [x] Deliberate handling of fully transparent pixels and hidden RGB.
- [ ] Improved edge coverage around antialiased artwork.
- [ ] Optional small-gap closure for line-art fills.
- [ ] Revisit whether Wand/Bucket tolerance should be linked or remembered separately.

## Layers and compositing

- [x] Alpha lock for safe recoloring.
- [x] Position lock for protected backgrounds.
- [ ] A small useful blend-mode set: Multiply, Screen, Overlay.
- [ ] Layer masks as a separately scoped architectural milestone.

## PDF import — review of the other agent's feature

Baseline: `91c8a48` adds raster PDF import, contiguous page ranges, DPI selection,
pages as layers or separate tabs, and single-PDF Open as Layers. Imports are new
unsaved projects; the first page sets the layered canvas size. PDF remains import
only, renders onto white, and does not preserve editable PDF text/vector objects.

- [x] Keep multiple PDF import dialogs queued instead of dropping later requests.
- [x] Queue raster publication too, so a mixed Open operation cannot dismiss a
      pending PDF form when its raster tab opens.
- [x] Build imported composites, tiles, previews, and thumbnails off the UI thread.
- [x] Match the renderer's fractional pixel rounding during DPI preflight; reject
      invalid/subpixel page dimensions and inconsistent probe data. Preserve
      existing document/page/stack budgets and convert probe/render panics to errors.
- [x] Add stage/per-page progress and cooperative cancellation during read,
      inspection, rendering, and document/layer preparation; discard canceled results.
- [ ] Interrupt an already-running parser, page render, or image-preparation
      call; evaluate worker isolation for prompt cancellation and resource limits.
- [x] Preflight the destination stack budget before Open as Layers renders pages.
- [ ] Consider incremental rendering/preparation to lower peak working memory,
      while retaining deliberate publication and undo semantics.
- [ ] Preview page thumbnails and allow noncontiguous page selection.
- [ ] Offer white versus transparent page background and remember import options.
- [ ] Offer layered canvas sizing (first page versus largest selected page), with
      an indication when larger page layers extend outside the first-page canvas.
- [x] Detect a source PDF changed between probe and render, or retain a consistent
      source snapshot for the import session.
- [ ] Expand automated fixtures for CropBox/rotation, fonts, transparency, images,
      and representative malformed/encrypted documents on macOS and Windows.
- [ ] Consider storing source/page/DPI metadata for explicit reimport later.

The first reliability fixes are part of Pass 4; source snapshots, progress,
cooperative cancellation, and destination preflight are part of Pass 8. The
unchecked options remain follow-up work. Existing source PDFs are never
overwritten by Save.

## Adjustments and filters

- [x] Preview within the full composition, including selection and layer opacity.
- [x] Before/after comparison and convenient reset.
- [x] Remember prior parameters and repeat the last operation.
- [ ] Re-editable applied effects as a separate project-format/history milestone.

## Paths and gradients

- [ ] Explicit corner/smooth anchors and linked/independent handles.
- [ ] Insert anchors on segments and select/move multiple anchors.
- [ ] Editable intermediate gradient stops, Reverse, and reusable gradients.

## Document foundations

- [ ] Crash recovery/autosave using a separate recoverable destination.
- [ ] Explicit embedded-profile handling and color-managed import/display/export.
- [ ] Plan project compatibility before storing masks, blend modes, or effects.
- [ ] Evaluate higher working precision separately from the core workflow pass.

## Decisions and follow-up notes

- Preserve existing public shortcuts and native menu routing.
- Keep machine-specific settings out of source; no version bump or release publishing.
- Prefer independently testable increments over bundling all architectural changes.
- Record the exact delivered subset and outstanding manual checks at the end of a pass.

## Pass 1 results

Implemented on 2026-11-13, on main. Pass 1 and the preceding review fixes were
committed as `51d4c70` at the start of Pass 2. This roadmap stays untracked.

- Grow/Shrink now use running max/min coverage filters instead of binarizing the
  mask. The square neighborhood and zero coverage beyond canvas edges remain
  consistent with the earlier hard-mask behavior. Feathering is unchanged.
- Scale Width/Height preview without resampling source pixels on each keystroke.
  Enter or leaving the field commits once; Escape, empty input, or invalid sizes
  restore the original target. Both image and stack budgets are enforced.
  Numeric edits keep the center; corner drags keep the opposite corner; side
  drags center the other axis when locked. The lock starts on and Shift reverses
  it during dragging. Nearest/Bilinear/Bicubic control final resampling; the GPU
  frame preview remains an approximation of that final result.
- Rotate accepts signed decimal degrees, rounded to tenths. Enter finishes the
  numeric edit; the existing Apply/Cancel transform workflow remains in place.
- Pencil, Brush, Eraser, Blur, Sharpen, Clone Stamp, and outline strokes each
  retain their own settings in memory. These preferences do not persist across
  app launches. Path and selection outlines use the same outline settings.
- Wand's Sample All Layers setting is independent of Bucket Fill's setting.
  Composite sampling runs in the background, follows visible layers and opacity,
  and works outside the active layer's bounds. Tolerance remains shared.
- No new tools, project-format changes, or external dependencies were introduced.

Automated checks: formatting, Clippy with warnings denied, and 252 tests pass.
Both ignored release benchmarks pass. Eight regression tests were added, and the
existing morphology oracle now checks soft min/max coverage as well as feathering.
macOS bundling and signature verification pass; `dist/Lime.app` was rebuilt.

Manual checks deliberately left to the maintainer:

- Scale: type dimensions, clear/Escape, edit the other field, toggle aspect lock,
  Shift-drag each side/corner, choose resampling, undo/redo, switch tabs/layers.
- Rotate: negative/decimal angles, empty/Escape, Apply/Cancel, undo/redo.
- Paint: change each tool's size/spacing/hardness/opacity and switch among them.
- Wand: compare active and merged sampling on offset, hidden, translucent layers.
- Feather a selection, then Grow/Shrink and fill/clear across its soft edge.

The larger unchecked capabilities above remain later-pass work. No interactive
or visual smoke tests were run for this pass.

## Pass 2 — precision and sampling

- [x] Move has signed X/Y controls using the existing preview and a single commit.
- [x] Move arrow keys nudge one image pixel; Shift nudges ten, independent of zoom.
- [x] Repeated nudges coalesce until arrow release, retaining queued events during
      their prior canvas commit and rejecting stale tab/layer/revision contexts.
- [x] Shift snaps Rotate drags to absolute 15-degree angles, with modifier changes
      reflected without a pointer move.
- [x] Wand and connected Fill ignore hidden RGB when both pixels are fully clear.
- [x] Clone Stamp supports aligned and unaligned strokes with safe source scoping.
- [x] Move metadata edits reuse thumbnails and clip composite refreshes to visible
      old/new bounds. Hidden and fully off-canvas moves need no pixel refresh.
- [x] Finish docs, automatic checks, release benchmarks, and macOS bundling.

Behavior decisions:

- Coordinate editing commits on Return or leaving the field. Empty/invalid text
  and Escape restore the original position. Off-canvas signed positions are allowed.
- A keyboard move is one undo step per arrow-release sequence. Escape or a
  tab/layer/tool change, deactivation, or modal UI cancels an unfinished sequence.
  Already released moves are ordinary document edits and finish in their own tab.
- Clone Aligned starts on. The first successfully published stroke establishes
  its offset; cancellation and failed/stale publication do not. Turning Aligned
  off restarts each stroke at the captured source; changing mode resets alignment.
  A source must be inside the visible raster layer. Changing tabs/layers clears
  it, and a geometry mismatch requires a new Alt-click. Merged sampling and the
  source overlay remain future work.
- Only pairs of fully transparent pixels ignore hidden RGB. Comparisons involving
  nonzero alpha still use the existing channel tolerance. Soft destination
  selection coverage and active-layer/composite source choices remain intact.
- No new tools, project-format changes, or dependencies.

Manual checks for Pass 2 (left to the maintainer):

- Move: type negative positions, clear/Escape, switch X/Y, undo/redo, and save.
  Hold arrows at Fit and a fixed zoom, use Shift, tap rapidly while updates run,
  and cancel a held sequence with Escape or by changing tabs/tools.
- Rotate: press/release Shift mid-drag, cross the angle seam, then Apply/Cancel.
- Clone: compare separate strokes with Aligned on/off; cancel the first stroke;
  change mode/source; switch tabs/layers and move/resize a layer before painting.
- Wand/Fill: use a clear region containing different hidden RGB, bordered by
  faint/opaque pixels; compare active/merged sources and a feathered selection.

Automated results: formatting, Clippy with warnings denied, and 260 tests pass.
Both release benchmarks pass. The macOS bundle and signature verification pass;
`dist/Lime.app` was rebuilt. Eight regression tests were added for this pass,
including composite/cache consistency and thumbnail reuse after moves. Pass 2
was committed as `4e2999b`; this roadmap remains untracked as requested.
No interactive or visual smoke tests were run.

## Pass 3 — dotted grid, ruler guides, and snapping

Implemented; subsequently committed as `a38b0ce` and `7c8ffc5` before the PDF review:

- Replaced grid lines with dots anchored to image coordinates. After maintainer
  feedback that the initial grid was too faint and loose, dots now have a solid
  1.5-point center and a pale 2.5-point rim for contrast. Default spacing is 16
  image pixels, with an eight-point minimum visible spacing at low zoom; spacing
  remains editable from Move options (1–4096 px), and snapping keeps that exact
  image-space spacing.
- View → Toggle Rulers and Guides shows pixel rulers and user-placed guide lines.
  With Move, drag from the top ruler for a horizontal guide or the left ruler
  for a vertical one. Drag existing guides to reposition them. Release on a ruler
  or outside the canvas workspace to remove; Escape/deactivation/tab/layer/tool
  changes cancel the draft. Rulers consume clicks instead of editing pixels.
- Guide drafts are scoped to their tab and revision. Release keeps the chosen
  position visible until publication. Each edit is one undo step, marks the
  project modified, and reuses all rendered pixels and thumbnails.
- Snap to Grid and Snap to Guides are separate controls in Move and View, both
  initially off. They work independently of visibility, snap layer edges/centers,
  and use a six-point screen tolerance. Nearest correction wins; equal distances
  favor guides and then edges over centers. Control bypasses snapping and guide
  grabs. Numeric coordinates and keyboard nudges retain exact values.
- Up to 1024 bounded guides are saved with the document and counted in history
  memory. They follow image scale, crop, canvas resize, quarter-turns and flips;
  crop/resize remove guides outside the resulting canvas. Layer transforms leave
  guides in canvas coordinates. Guides never appear in raster exports.
- Project version 7 stores guides. This build still reads version 6, keeps the
  golden v6 fixture unchanged, and writes version 6 (omitting guide metadata)
  whenever no guides exist. Files containing guides need this build or newer.
  A document with guides routes Save to project format so guides are not lost.
- No new toolbox tool or dependency was added. Ruler/grid/snap display preferences
  and grid spacing are session settings; guide coordinates are document data.

Automated: 268 tests, formatting, Clippy, and both release benchmarks pass.
Eight regression tests were added and the history-budget check now includes guide
ownership. macOS bundling and signature verification pass; `dist/Lime.app` was
rebuilt. These implementation changes and the roadmap were subsequently committed
by the other agent; the changes in the current pass are left uncommitted.

Manual checks left to the maintainer:

- Toggle grid/rulers at Fit, 100%, and high zoom; pan a clipped image and check
  ruler labels, dots, guide hit areas, cursor changes, and guide positions.
- Drag out horizontal/vertical guides; move, Escape-cancel, and remove them by
  dropping onto rulers or outside the workspace. Try guides at canvas edges.
- Move layers near guide/grid targets with each snap toggle independently; hide
  overlays, change grid spacing, and press/release Control during a drag.
  Check odd layer sizes, small images, off-canvas layers, and final release.
- Confirm typed Move positions and arrow nudges stay exact with snapping on.
- Undo/redo guide edits; save/reopen a guided project, transform/crop/resize it,
  and export a raster to verify overlays are excluded.

No interactive or visual smoke testing was performed.

## Pass 4 — PDF reliability and ordinary text editing

Committed as `16afc87`:

- Reviewed PDF import and fixed the four checked reliability items in its section
  above. Large imported documents no longer prepare their rendered canvases on
  the UI thread; mixed PDF/raster Open requests publish in order around dialogs.
- Text now has an anchored partial selection instead of an all-or-nothing flag.
  Dragging, Shift-click, Shift+arrows, and Shift+word movement extend it. Copy/Cut
  use the selected range; typing/Paste replace it. Highlights follow individual
  selected line segments and include selected newlines.
- Arrow motion and ordinary deletion respect Unicode graphemes (including
  combining marks and joined emoji). Word navigation/deletion uses Option on
  macOS and Ctrl on Windows; line/document edge navigation follows the platform
  gestures described in README. `unicode-segmentation` was already in Cargo.lock
  transitively; it is now declared directly without introducing a new package.
- Typing undo/redo keeps the selection anchor and caret. Delayed layout can
  resolve a released drag; source edits and caret navigation clear its pending
  pointer hit so it cannot move a later edit's caret. Window deactivation ends
  pointer dragging. Modified navigation cannot leak out of the font query and
  edit canvas text. Font/style controls still apply to the entire text layer.
- No new toolbox tool or project-format change. Fixed text boxes, paragraph
  alignment, rich text, IME/AltGr refinement, and deeper bidirectional selection
  validation remain later work.

Automatic validation: formatting, Clippy with warnings denied, all 281 tests,
and both release benchmarks pass. Seven regression tests were added for PDF
dimension/preparation contracts, partial text replacement/history, grapheme and
word deletion, delayed pointer selection, selection rectangles, and font-query
navigation isolation. macOS bundling and signature verification pass;
`dist/Lime.app` was rebuilt. No interactive or visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Open multiple PDFs, then a mixed PDF/raster batch; cancel/import each PDF and
  confirm later requests still appear. Try a fractional page size at low DPI.
- In Text, drag across lines, Shift-click, extend/reverse selections with arrows
  and word keys, copy/cut/paste, replace a range, and undo/redo. Include accented
  text, combining accents, emoji, CJK, and right-to-left text.
- Click/drag immediately after opening text or changing font/size, release before
  the preview catches up, then type or navigate. Confirm the caret remains stable.
- Exercise the font query's clipboard/history and modified navigation, and ensure
  it never changes canvas text. Confirm styles still affect the whole layer.

## Pass 5 — selection and crop precision

Committed as `b35a519`:

- Rectangle and oval selections have editable X/Y as well as Width/Height.
  Numeric edits preview live and commit one selection-only undo step; empty text
  and Escape restore the original selection. These controls cover editable
  geometric selections, rather than arbitrary coverage masks.
- Selections and Crop have independent session aspect-ratio preferences: Free,
  1:1, 4:3, 16:9, Image, or custom positive integer parts. Circle takes priority
  over the oval ratio. Choosing a ratio does not reshape the existing box; the
  next creation drag, handle resize, or numeric dimension edit uses it.
- One shared geometry implementation bounds ratio resizes and typed dimensions,
  preserves opposite corners/edges, and centers the unhandled axis for a side
  resize. Dimensions round to image pixels; ratios smaller than a pixel remain
  approximate. Fixed-ratio handles stop at minimum size when crossing their
  opposite edge. Free handles keep their previous crossing behavior.
- Optional Antialias edges smooths oval/circle and rounded-rectangle coverage.
  It starts off. Geometric curves keep their quality through move/resize,
  image transforms, canvas changes, clipping, and undo. Existing masks retain
  coverage when the tool preference changes; antialiasing does not try to
  reconstruct geometry from a combined or clipped mask.
- Coverage uses a 4×4 subpixel sample only near the curve; whole-pixel bounds
  provide fast interior/exterior results. Fill, clear, paint, clipboard,
  adjustments, and filters consume the existing shared coverage interface.
  Outline spans include pixels with nonzero soft coverage.
- Numeric position/size edits preserve the selection's current corner radius
  and quality, including after undo, rather than reusing stale tool settings.
  Radius readouts and steppers follow the current geometric selection.
- No new toolbox tool, dependency, or project-format change. Polygon/wand
  antialiasing, feather-on-creation, and arbitrary mask position controls remain
  later work.

Automatic validation: formatting, Clippy with warnings denied, all 287 tests,
and both release benchmarks pass. Six new regression tests cover constrained
geometry and extreme deltas, subpixel coverage and outline spans (including
clipped/thin curves), fill/clear coverage, transform invariance, and selection
history/saved-state behavior. Existing numeric limit tests also cover ratio
inputs. The macOS bundle was rebuilt and its signature verified. No interactive
or visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Draw rectangle, rounded rectangle, oval, and circle selections with hard and
  antialiased edges. Fill/Clear at high zoom, combine and clip selections, then
  undo/redo. Change radius, undo, then edit X/Y or size without changing corners.
- Try each ratio and a custom portrait ratio in Select and Crop; drag every
  handle to canvas edges and across its opposite edge. Edit Width/Height near
  an edge and confirm the paired dimension and preview agree.
- Move selections numerically, clear a field, press Escape, and undo/redo.
  Confirm selection-only changes do not mark a saved image unsaved. Apply a crop
  and confirm it remains one image undo step. Check controls in a narrow sidebar.

## Pass 6 — paint opacity, flow, and retouch strength

Committed as `4ce2937`:

- Opacity is now a per-stroke limit for Pencil, Brush, Eraser, and Clone Stamp.
  Ordinary overlapping dabs use maximum coverage against the starting pixels,
  so a 40% stroke no longer approaches full strength just because spacing is
  tight. Releasing and painting again deliberately compounds a new stroke.
- Optional Buildup reveals a separate Flow setting. Flow accumulates each dab's
  coverage toward the opacity limit; selection coverage and color alpha apply
  once after that accumulation. Defaults are Buildup off and Flow 100%. Each
  paint/retouch tool remembers these settings independently during the session.
  Buildup is distance sampled, with no stationary-pointer airbrush timer.
- Blur and Sharpen label the limit Strength. Each stroke blends its original
  pixels toward the existing local filter result; repeated dabs respect the
  chosen strength. Buildup approaches it gradually, and each new stroke applies
  another pass. Brush size still controls the affected area, not kernel radius.
- Path/selection outline strokes use the same per-stroke coverage cap, including
  overlapping contours. Their default remains non-buildup. Existing closed-join
  and repeated-endpoint tests now also exercise the explicit buildup mode.
- Touched 64 px tiles keep a 16-bit coverage buffer, adding scratch memory equal
  to half their RGBA storage. It is transient stroke data, never project/history
  data. Only real pixel changes invalidate previews or make the stroke dirty.
  Zero opacity, zero flow in buildup mode, and unchanged targets remain no-ops.
  Sub-byte buildup is retained without publishing invisible hidden-RGB edits.
- Existing spacing and pointer resampling remain in place: event density along
  the same path does not affect the result. Flow buildup intentionally depends
  on the number of spatial dabs, so spacing still affects its rate. Pencil keeps
  its hard edge and pixel snapping. No tool, dependency, or project-format change.
- Smoothing and merged Clone sampling/source previews remain later work.

Automatic validation: formatting, Clippy with warnings denied, all 291 tests,
and both release benchmarks pass. Four new regression tests cover opacity and
buildup caps across all six tools, repeat passes, source alpha/selection math,
low-flow accumulation, pointer-event density, and no-op scratch/publication.
Existing numeric limits and independent-setting tests include Flow/Buildup.
The macOS bundle was rebuilt and its signature verified. No interactive or
visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Paint, erase, and clone at 40% with tight spacing, crossing the same region
  within one stroke, then release and repaint. Include soft and feathered edges.
- Enable Buildup at low Flow; compare its gradual result to ordinary strokes,
  try Flow/Opacity zero, and switch tools to check independent settings.
- Blur/Sharpen at low Strength, revisit an area during the same stroke, then
  apply another stroke. Undo/redo and Escape should retain their usual behavior.
- Check live preview/commit agreement on offset or partially off-canvas layers
  and at non-100% layer opacity; exercise Shift-click and release endpoints.

## Pass 7 — composition previews and comparison

Committed as `93d1aba`:

- Adjustment and filter dialogs now preview the full canvas composition. Other
  visible layers, their order, offsets, opacity, and transparency are retained;
  the effect applies to the active layer with the current selection's coverage.
  The preview remains in the existing dialog rather than replacing the main
  canvas. A checker displays transparent pixels.
- Before/After switches between the opening composition and the chosen effect
  without changing parameters or launching another job. Apply still uses the
  current parameters while Before is displayed. Reset restores opening defaults,
  clears form errors and curve dragging, and switches to After. Parameterless
  operations retain their operation; filter defaults retain their opening color.
- The immutable preview snapshot shares layer pixels and caches its original
  reduced composite. Slider changes continue to coalesce in background jobs;
  stale results cannot replace a newer request or a dismissed dialog. Both
  comparison images are released when replaced, canceled, or rejected as stale.
  An updating indicator identifies a pending result without moving the controls.
- Output stays bounded to 256 px on its longest side. Filter scratch pixels cover
  the source region that can affect the canvas plus its sampling halo, scaled to
  that preview. This preserves detail when a small canvas views a large layer
  and includes off-canvas blur/shadow contributions without processing that
  entire layer for every slider position.
- Drop Shadow previews a separate layer beneath the source, with its actual
  offset and clipping to the canvas. Its silhouette is independent of selection
  and source-layer opacity, matching the committed operation.
- Point adjustments use the commit mapper and coverage blend. At unreduced
  canvas size, filter/shadow previews match committed composition in regression
  fixtures. Reduced filter kernels remain approximate; a blur/sharpen radius
  that rounds below one preview pixel leaves the original preview unchanged.
  Apply always processes full-resolution pixels.
- Centralized opening/reset defaults and start each adjustment with its intended
  operation, avoiding the previous throwaway neutral preview for Invert and
  Desaturate. Removed the superseded isolated adjustment-preview path.
- No toolbox tool, dependency, or project-format change. Remembered/repeated
  parameters and re-editable effects remain later work.

Automatic validation: formatting, Clippy with warnings denied, all 295 tests,
and both release benchmarks pass. Five new regression tests replace the old
isolated adjustment-preview test and verify real composition against committed
adjustments, filters, shadows, soft selection coverage, offsets, opacity, hidden
layers, and grayscale. They also cover large-layer/small-canvas sampling halos,
extreme offsets, bounded scratch sizes, immutable comparison snapshots, and the
transparency checker. The preview-generation test now checks pending/reset
status and stale completion. The macOS bundle was rebuilt and its signature
verified. No interactive or visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Adjust an offset, partially transparent layer above and beneath other visible
  layers, with a feathered selection. Compare the dialog to the applied result;
  use Undo to return to the original image.
- Switch Before/After while dragging sliders and while an update is pending.
  Reset each dialog, including Levels/Curves channel and curve edits; Cancel
  should leave pixels/history unchanged. Apply from Before should use the chosen
  effect, not the displayed original.
- Blur/Sharpen at small and large radii, and Drop Shadow with positive/negative
  offsets, a low-opacity source, and a source partly outside the canvas. Check
  transparency, grayscale, and a large layer viewed through a smaller canvas.
- Close and reopen effect dialogs rapidly, switch documents after dismissing,
  and confirm stale previews never appear in the later dialog. Check the panel
  in a small window and leave appearance feedback for the next pass.

## Pass 8 — PDF import cancellation, snapshots, and destination preflight

Committed as `9df5b6f`:

- PDF inspection retains immutable, shared source bytes. Rendering parses that
  same snapshot, so replacing or removing the original after inspection cannot
  change the imported pages or invalidate the inspected dimensions. Queued PDFs
  hold only paths until their turn; they no longer retain many source snapshots.
- A PDF progress modal reports reading, inspection, rendering each selected page,
  and preparing new documents or destination layers. Cancel and Escape request
  cancellation. Stage/page events drive updates without polling while idle.
  Cancellation is checked around parsing, between 64 KiB read chunks, between
  pages, during per-layer insertion/conversion, and around final image preparation.
- Cancellation is cooperative. A running parser, page-render, or composite/tile
  preparation call finishes before cancellation settles. The modal explicitly
  shows Canceling while it waits. Interrupting those calls and bounding their
  working resources through isolation remain unchecked follow-up work.
- Canceled jobs discard every prepared document/layer candidate, including a
  completed prefix. A final UI publication gate checks cancellation and job
  identity again, covering cancellation after the worker finishes. Dropping the
  workspace job requests cancellation. Later queued files remain available.
- Open as Layers validates the entire selected range against destination layer
  count, available IDs, and pixel bytes before rendering. A range that does not
  fit stays in the options dialog with an error; it no longer renders pages only
  to import a prefix. Budget admission shares raster-import checks and needs no
  allocated page image to preflight a size.
- Existing-layer preparation now builds the candidate content, composite, tiles,
  and thumbnails within the cancellable background job. Publication validates
  the original tab/revision/anchor and adds all selected pages in one undo step,
  preserving selection and choosing the last imported layer. Ordinary content
  edits and PDF insertion share preview cleanup before candidate preparation.
- The existing document, page, DPI, and source-size limits remain. Selected page
  rasters are still accumulated before preparation; incremental processing to
  lower peak working memory is not implemented. No tool, dependency, project
  format, or version change.

Automatic validation: formatting, Clippy with warnings denied, all 302 tests,
and both release benchmarks pass. Seven new regression tests cover immutable
source snapshots, cancellation before/after rendering and preparation (including
completed prefixes), job teardown/late-publication rejection, whole-batch size/
layer/ID admission without allocating large images, and successful prepared-layer
publication as one undo step. The macOS bundle was rebuilt and its signature
verified. No interactive or visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Open a mixed PDF/raster batch and multiple PDFs. Cancel during inspection,
  page rendering, and preparation; confirm later requests still appear and no
  partial PDF tabs/layers are added. Try Escape and the Cancel button.
- Change or remove the source PDF while its options dialog is open; importing
  should still use the inspected snapshot. Reopen the path to inspect new bytes.
- Open as Layers into a nearly full stack. Choose a range that exceeds available
  count or memory, reduce range/DPI after the error, import, and undo/redo once.
- Import into grayscale and partially occupied documents, and check centered
  placement, page order/names, active layer, and retained selection. Cancel late
  during preparation and confirm pixels, layer stack, history, and saved state
  stay unchanged. A complex current step can delay completion of cancellation.

## Pass 9 — alpha and position locks

Implemented; committed as `993ab0a` at the start of Pass 10:

- Layers have independent Lock Alpha and Lock Position icon toggles beside each
  row's visibility button, with closed/open lock states and explanatory
  tooltips, plus matching Layer menu actions. Row controls preserve the active
  layer and validate the clicked tab/layer after numeric or text commits.
  Editable text rows disable alpha lock until rasterized.
  Active lock icons use the accent color, retained on hover; inactive open locks
  are dimmed. They retain visibility's hover treatment without persistent
  backgrounds or size changes. Position lock uses a plain padlock, without move arrows.
  Toggling a lock is one undoable image
  change with no pixel, composite, or thumbnail rebuild. Duplicate retains locks;
  newly created, imported, and shadow layers start unlocked.
- Move's Snapping section folds independently of its settings, starts collapsed,
  remembers the session's fold state, and displays enabled modes in its header.
  Tool options now take only their natural height (capped at half the inspector),
  scrolling independently from Layers/Paths in the remaining space.
  The inspector is widened from 232 to 272 px
  for the visible per-row controls; the old active-layer lock checkboxes are
  removed. The resizable toolbox also starts at 272 px for matching sidebar widths;
  its explicit outer width contains the resize grip as an overlay along the
  inside edge, so the grip cannot add layout width.
- Alpha lock preserves each original pixel's alpha during paint, clone, outline
  strokes, connected/selection fills, gradients, adjustments, and filters. Paint
  and fills mix color with source-atop semantics, including opacity, flow, source
  color alpha, and soft selection coverage. Transparent pixels retain hidden RGB.
  Eraser, Cut, and Clear cannot remove locked pixels. Obvious transparent fill
  no-ops skip unnecessary work. Stroke previews and commits reject stale policies.
- Blur and Sharpen retain weighted color before rounding a faint blurred alpha,
  then blend against the original mask. This avoids darkening a 1/255-alpha edge
  just because the intermediate filtered alpha rounds to zero. Filter and
  gradient previews use the same alpha policy as the committed result.
- Position lock blocks pointer moves, numeric moves, nudges, alignment, scale,
  quarter/free rotation, flips, perspective, and layer fit/content crop. UI entry
  points avoid creating transform previews, and model mutations enforce the same
  rule. Move's guide operations remain independent of the active layer lock.
- Image-wide crop, scale, rotate/flip, and canvas resize still carry the whole
  document with them and retain lock metadata. Alpha lock permits intentional
  layer geometry changes; position lock controls those. Opacity, visibility,
  stacking, and deletion remain available. Merge Down requires both layers
  unlocked so it cannot silently replace a protected silhouette or geometry.
- Editable text accepts position lock and remains editable at its current layout
  anchor, including automatic bitmap resizing for content/style changes. Alpha
  lock requires rasterization. Both the model and project codec reject alpha-
  locked editable text; the panel/menu keep that toggle unavailable until then.
- Projects with any lock use format version 8. Optional per-layer lock metadata
  defaults to unlocked, and unsupported legacy-version lock combinations are
  rejected. The reader continues to support versions 6/7; unlocked projects still
  write version 6, or version 7 when guides are present. Ordinary raster Save
  cannot discard lock metadata; flattened Export remains available.
- Added `fixtures/projects/v8-layer-locks.lime`, preserving the existing v6
  golden fixture and its legacy extension. No toolbox tool, dependency, package
  version, or release publishing change. Blend modes and layer masks remain later.

Automatic validation: formatting, Clippy with warnings denied, all 310 tests,
and both release benchmarks pass. Eight new regression tests cover locked paint/
clone/retouch, fill/gradient/filter/outline coverage, faint alpha, manual versus
image-wide geometry, text anchors, lock-bearing/legacy project round trips and
malformed metadata, preview/commit agreement, and undo with shared pixel buffers.
Existing native-menu and raster tests were updated for the added actions/policy.
The macOS bundle was rebuilt and its signature verified. No interactive or
visual smoke tests were run.

Manual interaction testing stays with the maintainer. Useful checks:

- Fold/unfold Move snapping, switch tools, and confirm the header/settings and
  Layers space remain correct. Toggle either lock on an inactive row and confirm
  the active layer stays selected. Hover the buttons, click without triggering
  rename/reordering, and toggle a lock immediately after typing a Move position.
- Lock alpha on a layer with soft transparency; paint, clone, fill, gradient,
  blur/sharpen, and stroke outlines. Compare previews with Apply/release, and try
  Eraser, Cut, Clear, fully transparent areas, and low opacity/flow.
- Lock position and try drags, X/Y, arrow nudges, alignment, Scale dimensions and
  handles, Rotate/Flip/Perspective, layer fit/crop, and the native menu entries.
  Confirm Move can still place/reposition guides. Unlock and retry normally.
- Edit position-locked text and change its styles; its anchor should stay put.
  Rasterize it, then enable alpha lock. Duplicate locked layers and check locks;
  confirm Merge Down becomes available after unlocking both participants.
- Toggle locks, undo/redo, save/reopen `.lime`, and export a raster copy. Undo to
  the saved state should clear the dirty marker. Try Save on an imported PNG
  after locking it; the app should preserve the settings through project Save.

## Pass 10 — remembered and repeated effects

Implemented; committed as `8c9318c` at the start of Pass 11:

- All seven adjustments and three filters reopen with their own last successfully
  applied parameters, shared across tabs for the app session. Color Balance keeps
  all three ranges and its luminosity option; Levels and Curves retain every
  channel, not just the currently displayed one. Memory contains parameters only.
- Filters → Repeat Last Effect (Cmd/Ctrl+F) applies the most recent successful
  adjustment or filter directly. Reopen Last Effect… (Cmd/Ctrl+Shift+F) opens its
  remembered dialog for preview and further editing. Native menus on both
  platforms share these actions, shortcut labels, and availability.
- Repeat resolves the current visible raster layer and current selection after
  typed geometry/text commits. Hidden/editable-text layers, busy/modal states,
  and unfinished canvas previews disable it. It follows alpha/position locks,
  preserves the active layer, and uses the ordinary background edit/history path.
  A changed result is one undo step; an empty/unchanged result creates none.
- Shadow repeats use the whole current source silhouette, insert below it, and
  preflight the current destination's layer budget and representable offsets.
  The dialog now shows its remembered color and offers Use Foreground. Reset
  restores built-in values, including the current foreground for shadows, and
  returns to After rather than restoring the remembered opening parameters.
- Memory updates only after an edit successfully publishes. Canceled, identity,
  pixel-no-op, rejected/stale, and closed-tab edits cannot replace prior values.
  Submission order prevents slower jobs in another tab from restoring older
  settings. Undo/Redo change the document without rewriting session preferences.
- No toolbox tool, dependency, project-format, package-version, or publishing
  changes. Re-editable effects remain a separate architectural milestone.

Automatic validation: formatting, Clippy with warnings denied, and all 317 tests
pass. Seven new regression tests cover per-effect memory, out-of-order cross-tab
completion, failed/no-op/stale publication, current target/selection and lock
handling, shadow preflight/insertion, and adjustment/filter control round trips.
The shared native-menu checklist and icon validation include both new actions.
The macOS bundle is rebuilt and signed for the maintainer's manual testing.
No interactive or visual smoke tests are run. Rendering and stroke algorithms are
unchanged; release benchmarks are not repeated for this workflow/UI pass.

Manual interaction testing stays with the maintainer. Useful checks:

- Before any applied effect, confirm both new commands are disabled. Apply several
  adjustment/filter kinds and reopen each ordinary dialog to check independent
  remembered values. Exercise every Levels/Curves channel and Color Balance range.
- Cancel a changed preview, Reset/Apply an identity adjustment, and apply with an
  empty selection; verify they do not replace the last repeatable operation.
- Repeat on another layer or tab and inside a new feathered selection. Check
  alpha/position locks, hidden and text layers, typed Move/Scale values, one-step
  undo/redo, saved revision markers, and both shortcuts on macOS/Windows.
- Reopen a remembered shadow after changing the foreground. Inspect the swatch,
  Use Foreground, Before/After, Reset, and Cancel; compare the eventual layer.
  Repeat with a constrained destination stack and confirm it is rejected cleanly.
- Leave a gradient/transform/crop/free-selection draft or an active pointer
  gesture and confirm Repeat/Reopen cannot silently discard that work.

## Pass 11 — polygon edge quality and selection creation feathering

Implemented; committed at the start of Pass 12:

- Free Select has an independent Antialias edges preference for new closed
  freehand/polygon selections, default off. Four vertical scans and four
  horizontal subpixel samples produce fractional coverage using nonzero winding.
  Clipped shapes, opposite winding, holes, crossings, thin shapes, and aligned
  edges retain their geometry. Paths and antialias-off polygons keep hard
  center sampling. The normal byte mask holds counts; no 4x-size image is built.
- Rectangle Select, Oval Select, Free Select, and Magic Wand expose Feather new
  selections, default off, with a shared explicit radius (0–128 image pixels,
  initially 8). The setting is remembered for the workspace session. Empty text
  and Escape restore its previous value; settings changes do not edit history or
  dirty the image. A radius of zero bypasses feathering.
- Only the incoming selection is feathered, then combined with the existing
  mask. Shift/add, Control/subtract, and Shift-Control/intersect therefore retain
  old coverage rather than repeatedly softening it. Existing geometry moves,
  resizes, and typed dimensions do not apply creation feathering retroactively.
  Free Select still closes on its start node/Return and rejects degenerate shapes.
- Polygon coverage and feathering run inside the existing background selection
  job when the new gesture finishes. Pointer gestures retain their ordinary
  geometric outline; the final selection publishes with soft coverage in one
  selection-only undo entry. Wand feathering honors layer offsets and clipping.
  Pixel tools consume the same mask, retaining existing selection/history rules.
- Canvas-boundary feathering is explicitly defined and checked: its square
  averaging kernel treats outside-canvas coverage as zero and keeps its full
  denominator. Bounded Full selections soften at the image boundary as well.
  Unrestricted is no mask and remains unrestricted. Coverage cannot escape the
  canvas; grow/shrink behavior is unchanged.
- Feathered selections use the existing mask representation, so analytic
  geometry fields/handles are unavailable afterward, as with Select → Feather.
  Retaining editable source geometry is recorded separately. Wand antialiasing
  remains planned; feathering does not claim color-aware edge recovery.
- No toolbox tool, dependency, project-format, version, or publishing changes.

Automatic validation: formatting, Clippy with warnings denied, all 324 tests,
and both release benchmarks pass. Seven new regression tests compare subpixel
winding and feather coverage to independent references, check thin/clipped/
invalid shapes, exercise actual fill/clear alpha, preserve incoming-only
combination semantics and offset wand coverage, and validate radius input.
Existing selection history, saved revision, clipboard, paint, and effect tests
remain passing. The macOS app is rebuilt and its signature verified.
No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Close diagonal/thin/concave Free Select outlines with antialias on/off. Zoom in,
  fill or clear, and compare edges. Confirm aligned rectangles stay crisp with
  antialias on, and Escape still cancels an unfinished outline.
- Enable creation feathering on each of the four selection tools. Change radius,
  clear/Escape its field, then create a new selection. Verify the existing mask
  stays unchanged until another selection is completed and the option survives
  tool changes. Check that no creation setting alone changes the saved marker.
- Add/subtract/intersect feathered regions repeatedly, including regions touching
  the canvas edge and active layers with offsets. Confirm old edges do not fade
  again. Undo/Redo selections and verify pixels/saved revisions remain unchanged.
- Use feathered selections with paint, fill, Clear, clipboard, adjustments, and
  filters. Compare alpha/coverage, including around the canvas border and after
  Select All → Feather. Check that radius zero and disabled feather preserve
  existing hard selection behavior and numeric geometry controls.
