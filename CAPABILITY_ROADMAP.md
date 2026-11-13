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
- [x] Optional antialiasing for wand selection edges.
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
- [x] Proportional Fit Layer to Canvas from the Layer menu and Scale Layer options
  for oversized layers whose handles cannot be reached (GitHub issue #1).
- [x] Rotation pivot anchors, typed canvas coordinates, and dragging.
- [x] Nine-point anchors for typed Scale Layer dimensions and precise Perspective
  corner coordinates with individual/all-corner reset.
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
- [x] Optional stroke smoothing with bounded latency.
- [x] Predictable strength and repeat-pass behavior for Blur/Sharpen.
- [x] Clone aligned/unaligned behavior and scoped source validation.
- [x] Clone merged sampling and source-position/footprint preview.

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
- [x] Optional small-gap closure for line-art fills and wand selections.
- [x] Remember Wand/Bucket tolerance and gap radius separately during the session.

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
- [x] Allow noncontiguous page selection with individual pages and ranges.
- [x] Preview/select pages with an interactive, bounded thumbnail browser.
- [x] Offer white versus transparent page background and remember import options
      for the session.
- [x] Offer layered canvas sizing (first page versus fitting all selected pages), with
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

- [x] Corner/Smooth anchor commands and independent/aligned/symmetric handle drags.
- [x] Insert anchors on segments without changing the curve.
- [x] Select and move multiple anchors together.
- [x] Editable intermediate gradient stops and Reverse.
- [x] Reusable gradient presets.
- [x] Persist a named gradient library across app launches.

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

Implemented; committed as `71451b5` at the start of Pass 12:

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

## Pass 12 — Clone Stamp merged sampling and source indicator

Implemented; committed as `88d2478` at the start of Pass 13:

- Clone Stamp exposes Sample All Layers, independently of Wand/Bucket settings.
  Default active-layer sampling is unchanged. Merged sampling reads the current
  visible full-resolution composite, including other layer opacity and cached
  text, and can choose source pixels outside the destination layer's bounds.
  Painting still changes only the active visible raster layer within its existing
  bounds, selection, and alpha-lock policy.
- A stroke retains its immutable sampling image, separate from the destination's
  starting pixels. Layer-local destinations map into canvas-space samples through
  the layer offset and clone delta. Source pixels cannot pick up live changes from
  that same stroke, including with overlap/flow. Unavailable/transparent samples
  remain no-ops. The composite is shared rather than recomputed/copied per dab.
- Source validity includes tab/layer identity, destination layer geometry, canvas
  dimensions, and sampling mode. Changing sampling clears the source and prompts
  for Alt-click again; sampling/alignment changes are refused during a stroke.
  Existing successful-publication alignment semantics remain in place, including
  cancellation and failed commits. Unaligned strokes restart at the chosen source.
- Show Source, default on, displays a blue dashed source brush footprint and
  crosshair while hovering/painting. It marks actual sampled pixel centers,
  follows live/pending/committed aligned offsets, and follows then resets for
  unaligned strokes. The indicator respects image coordinates, zoom/pan, canvas
  clipping, sample bounds, and mouse exit. Alt retains its source-picking cursor.
  This is a position/footprint preview. Its drawing shares the current cursor
  refresh path, without polling or per-pointer sample-image generation.
- Help, README, and AGENTS document sampling, scoping, and preview behavior.
  No toolbox tool, dependency, project-format, version, or publishing changes.

Automatic validation: formatting, Clippy with warnings denied, all 330 tests,
and both release benchmarks pass. Six new regression tests cover merged clone
coordinate mapping with negative layer offsets, soft selection/alpha lock,
immutable snapshots without feedback, unavailable/transparent no-ops, source
context validation, source-marker coordinates, and real composite stroke-preview/
commit agreement with visible/hidden/partially opaque layers. Existing aligned/
unaligned tests use the stricter source context. The macOS app is rebuilt and its
signature verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- On a small/offset raster layer, enable Sample All Layers and Alt-click a visible
  source outside that layer. Include partially opaque layers, hidden layers, and
  editable text elsewhere. Paint and compare sampling with the merged canvas;
  ensure only the destination pixels and selected coverage change.
- Clone an overlapping region in a long stroke, with Buildup/Flow and alpha lock.
  Compare live tiles and release, then Undo/Redo. Try a transparent source and a
  source that leaves the canvas; neither should create unexpected paint/history.
- Check Show Source while hovering, painting, holding Alt, changing brush size,
  zooming, fitting, panning, and entering/leaving the canvas. Compare aligned
  continuation with unaligned reset. Hide the indicator and confirm painting stays
  unchanged. Check markers on light/dark backgrounds and near image edges.
- Change sampling, tabs/layers, or layer/canvas geometry and confirm a fresh source
  is required. Cancel a first stroke, retry it, and verify alignment is established
  only by a successfully committed change.

## Pass 13 — gradient stops, direction, and session presets

Implemented; committed as `9fc37f1` at the start of Pass 14:

- Gradient retains dynamic FG → BG / FG → Clear and adds Custom palettes with
  up to eight RGBA stops. Select a colored stop chip, use the existing RGB picker,
  edit opacity, and type interior positions to tenths of a percent. Endpoints
  stay at 0%/100%; interior positions stay ordered between distinct neighbors.
  Add Stop splits the largest gap using the current color there; Remove protects
  endpoints. Dynamic modes and the remembered Custom palette remain independent.
- Reverse changes the sampled color direction for Linear/Radial gradients,
  including degenerate lines, without changing canvas endpoints or radial center.
  All stops use premultiplied-alpha interpolation; hidden colors at transparent
  stops cannot tint visible edges, and faint alpha retains its color.
- Six deduplicated session preset slots capture literal colors/stops, shape, and
  Reverse. Loading selects Custom mode without moving the current canvas line.
  Presets survive tool/tab changes and are independent copies; deleting a saved
  slot leaves the current palette in place. Persistent named libraries remain a
  separate follow-up. No preset or stop settings are stored in image projects.
- Stop/preset/shape/direction edits refresh the existing coalesced background
  preview. Preview and Apply use one gradient specification and sampler, including
  every stop's grayscale conversion, destination alpha lock, layer offsets, and
  soft selection coverage. Only Apply changes pixels/history; Cancel leaves them
  unchanged. Apply/Cancel sit above the longer palette/preset editor.
- The shared color dialog now uses typed targets. Gradient stop targets carry a
  palette revision/index and preserve alpha while editing RGB, without changing
  foreground/background swatches. New stop colors join Recent Colors. Stale
  targets cannot recolor another palette. Opacity numeric editing retains the
  exact alpha byte so restoring a rounded percentage cannot shift transparency.
  Decimal numeric input is shared with the existing angle editor.
- Updated help, README, and AGENTS. No toolbox tool, dependency, project-format,
  package-version, or publishing change. Concurrent app-icon edits are retained.

Automatic validation: formatting, Clippy with warnings denied, all 337 tests,
and both release benchmarks pass. Seven new regression tests cover bounded stop
editing, independent integer interpolation, transparent/faint colors, immutable
palette color mapping, reversed geometry, decimal position/alpha restoration,
and real preview/Apply agreement across grayscale, masks, offsets, and alpha lock.
Existing gradient history, selection, and project tests remain passing. The macOS
app is rebuilt and its signature verified. No interactive or visual smoke tests
are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Try dynamic and Custom gradients, edit every stop color/opacity, add up to eight,
  move interior stops toward either neighbor, and remove them. Confirm endpoints
  cannot move/remove and Custom survives a round trip through the dynamic modes.
- Type decimal positions and opacity, clear fields or press Escape, and check
  restored values, cursor/clipboard editing, and the preview. Cancel a color pick;
  confirm foreground/background colors remain unchanged and stop alpha survives.
- Reverse Linear/Radial gradients and zero-length lines; compare preview/Apply
  with transparency, soft selections, layer offsets, grayscale, and alpha lock.
  Undo/Redo the result as one edit and verify Cancel keeps pixels/saved revision.
- Save six presets, load them after changing tools/tabs/swatches, and check literal
  colors, shape, and direction. Confirm loading preserves endpoint positions,
  duplicate saves do not fill another slot, and deletion leaves the preview intact.
- Check Apply/Cancel access and independent Layers scrolling with eight stops in
  the options panel. Presets deliberately reset with a new workspace session.

## Pass 14 — path anchor insertion and handle precision

Implemented; committed as `5150b9b` at the start of Pass 15:

- Alt-click a visible path segment to insert an anchor by de Casteljau subdivision
  without changing the represented cubic. Existing anchor/handle hits take
  priority. The new point can be dragged immediately within the same gesture;
  release commits one path edit and Escape discards the draft. Closed paths can
  split their closing segment without changing the first anchor or closed state.
- Segment hit testing uses the existing eight-screen-point path slop, preserving
  behavior through Fit, fixed zoom, and pan. Bounded subdivision prunes control
  bounds; chord-projection inversion and Newton refinement locate the curve even
  with stationary, nonuniform straight-cubic parameterizations. Anchor limits,
  finite coordinates, fresh IDs, and non-endpoint split parameters are enforced.
- Path options now expose selected-anchor Corner/Smooth actions. Corner collapses
  both handles. Smooth aligns them while preserving existing handle lengths,
  initializing missing handles from neighboring anchor distances. Degenerate and
  identity actions do not create history. The selected anchor has a larger
  outlined marker, including during a live insertion/drag preview.
- Handle Drag offers Independent (the existing default), Aligned (rotate the
  opposite handle while retaining its length), and Symmetric (mirror direction
  and length). These are session editing preferences; selecting one alone does
  not reshape an anchor. Newly created handles still begin symmetric. Each drag
  uses its baseline anchor, avoiding repeated-update drift. Clicking a handle
  without moving it never couples or reshapes the opposite handle.
- Path-row switches clear stale anchor selection and refuse changes while a tab
  is busy. Deleting an anchor clears its selection, and busy tabs reject deletion.
  Geometry commands use the normal metadata/history path without rebuilding or
  changing raster pixels. Saved geometry opens in the existing project format;
  there is no new per-anchor mode metadata or format-version change.
- Multi-anchor selection/movement remains planned as a separate increment.
  Updated help, README, and AGENTS. No toolbox tool, dependency, package-version,
  installer, icon, or publishing changes belong to this capability pass.

Automatic validation: formatting, Clippy with warnings denied, all 344 capability
snapshot tests, and both release benchmarks pass. Seven new regression tests
cover exact open/closing cubic subdivision, hit-test geometry and stationary
parameters, invalid/over-budget/large-finite edits, handle coupling, corner/smooth
identity and endpoints, existing-format project round trips, and undo/redo with
shared pixel buffers and saved-revision restoration. The shared workspace also
passes formatting, Clippy, and all 349 tests. Release validation uses a temporary
snapshot of the last complete build plus this pass, excluding concurrent
housekeeping changes. Its separate app bundle is rebuilt and signed
without overwriting the other agent's installer artifacts. No interactive or
visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Alt-click straight, curved, crossing, and closing path segments at Fit and high
  zoom, including after pan. Verify a click inserts on the curve without a jump,
  a drag moves the inserted point, and one Undo restores the exact old path.
  Try near existing anchors/handles and away from curves; normal drawing and
  first-anchor closure should retain their existing gestures.
- Select different anchors and exercise Corner/Smooth, including open endpoints,
  unequal handles, collapsed handles, and repeated identity commands. Verify the
  selected marker and that no raster pixels change until an explicit path stroke.
- Drag incoming/outgoing handles in every Handle Drag mode, away and back to the
  original point. Check opposite length/alignment, identity clicks, cancellation,
  and one-step undo/redo. Confirm choosing a drag preference creates no history.
- Switch paths/tabs/tools, hide/delete a path, and check selected-anchor controls
  cannot edit a stale or hidden target. Save/reopen `.lime`, convert paths to
  selections, and stroke them to confirm the edited geometry is retained.


## Pass 15 — grouped path anchor editing

Implemented; committed as `ea5da09` at the start of Pass 16:

- Shift-click anchors to add/remove a selection within one path. This changes
  session selection only, without starting a drag, creating an anchor, closing
  the path, or adding image history. Clicking a selected anchor retains its
  group for dragging; clicking another anchor or path replaces the selection.
  Clicking an existing path now selects it without an otherwise empty edit.
- Drag a selected anchor to translate the entire group and its handles. Each
  preview derives from the original anchors and pointer-down position, preserving
  the grab offset and restoring exact geometry when returning to the start.
  Invalid/nonfinite/overflowing, duplicate, or stale group targets reject the
  entire translation. Unselected anchors and open/closed state stay intact.
- All Anchors and Deselect join the existing Path options, with a selected count;
  every selected marker uses the larger outlined style. Corner and Smooth shape
  the entire group; Delete/Backspace removes it. Each command or drag commits one
  metadata undo step, shares raster buffers, and preserves ordinary saved-state
  tracking. Deleting to two anchors opens the path; deleting all removes it.
- First-anchor click closure applies only to a single selected anchor. Handle
  dragging still edits one handle in its chosen coupling mode and preserves an
  existing group containing that anchor. Alt-insertion selects its new anchor.
  Escape reliably repaints even when canceling the first uncommitted anchor;
  other gesture cancellation removes nonexistent draft IDs before they can be reused.
- Path/tab/tool switches, path hiding/deletion, and undo/redo clear session
  selection. Controls validate live IDs and path visibility, and busy tabs
  reject edits. No new tool, project format, dependency, installer, icon, package
  version, or publishing changes. Updated help, README, and AGENTS.

Automatic validation: formatting, Clippy with warnings denied, and all 356 tests
pass (two existing performance benchmarks remain ignored). Seven new regression
tests cover selection toggling and target validation, group/inserted-point drag
behavior and first-anchor closure, baseline translation without drift, atomic
invalid updates, batch deletion, and exact undo/redo with shared raster buffers
and saved-state restoration. A separate release app bundle is rebuilt and its
signature verified from the current committed workspace plus this pass; shared
installer artifacts are untouched. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Shift-click several nodes, toggle one off/on, and drag a selected node at Fit
  and high zoom after panning. Check all selected handles move together, others
  stay fixed, returning to the starting point restores the geometry, and one
  Undo restores the entire move. Try an offset click within a node's hit area.
- Use All Anchors/Deselect and Corner/Smooth. Delete several anchors, undo/redo,
  delete down to two, then delete all. Check counts, markers, path closure, and
  the saved marker. Selection alone should not create an undo step.
- Shift-click the first node of an open path, include it in a group, then click
  that selected node; neither action should close the path. With that node selected alone, confirm normal click
  closure. Alt-insert and drag a node, and drag handles while a group is selected.
- Cancel a live move and the very first newly drawn anchor. Switch paths, hide or
  delete one, change tabs/tools, and undo/redo. Confirm no stale group is editable.
  Save/reopen the geometry and check raster pixels are unchanged until Stroke.


## Pass 16 — optional bounded stroke smoothing

Implemented; committed as `8f87605` at the start of Pass 17:

- Brush, Eraser, Blur, Sharpen, and Clone Stamp offer Off/Light/Strong smoothing
  in the existing brush options, in a full-width group below a regular mixed-case
  Smoothing field label with tight spacing, so longer choices remain readable.
  Each tool remembers its own session setting;
  Off starts selected. Pencil and path/selection outline strokes remain raw.
  Smoothing choices cannot change an active gesture's settings.
- A constant-space, time-based low pass uses a 16/32 ms response for Light/Strong,
  subdividing catch-up into eight-ms steps and reaching a stationary pointer
  exactly after 64/128 ms. Duplicate coordinates never extend that deadline.
  An enabled stroke wakes only while its tail is pending; there is no idle
  frame polling, input queue, added dependency, or default-mode filter work.
- Release feeds its final pointer coordinate, drains at most sixteen remaining
  tail samples synchronously, and then finishes/commits through the existing
  dirty-tile stroke path. There is no delayed edit after release. Escape,
  context changes, focus cancellation, or a replaced stroke discard pending
  work; timer callbacks validate a per-stroke token before updating pixels.
- Off keeps the direct sampling path and existing raster output. Initial dabs,
  Shift-click straight connectors, dab spacing, stroke opacity/flow, alpha locks,
  selection coverage, and layer-local coordinates keep their existing contracts.
  The Clone source marker follows the actual filtered destination while painting.
- Invalid/nonfinite input and backward timestamps are rejected. Catch-up work
  stays bounded even after a long UI pause or extreme finite coordinates.
  Updated help, README, and AGENTS. No new toolbox tool, project format, package
  version, release publishing, or housekeeping changes belong to this pass.

Automatic validation: formatting, Clippy with warnings denied, all 364 tests,
and both release benchmarks pass. Eight new regression tests cover timed response
against an independent exponential reference, timer-density invariance, jitter
reduction, exact bounded settling/release, duplicate/invalid/extreme inputs,
independent per-tool modes, unchanged raw strokes, cancellation without source
writes, and live-tail/release/preview/commit agreement for all five paint kinds
with offset layers, soft selections, and alpha locks. The existing Clone marker
test also checks the filtered destination. `dist/Lime.app` is rebuilt and its
signature verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Compare Off, Light, and Strong for Brush, Eraser, Blur, Sharpen, and Clone Stamp
  at Fit and high zoom after panning. Draw fast curves and small shakes, stop
  without releasing, then release. Check finite catch-up, exact endpoints, and
  independent remembered choices. Pencil and outline strokes should stay raw.
- Try very short strokes, rapid release, repeated identical pointer positions,
  and Shift-click straight connectors. Undo/redo each completed stroke in one
  step, and check subsequent Shift-click starts at the exact last endpoint.
- Paint on offset layers with soft selections, alpha lock, Buildup/Flow, and
  different spacing. Check preview/commit agreement and Clone's source marker.
- Cancel a live tail, immediately start another stroke, change tabs/layers/tools,
  and lose/regain app focus. Confirm canceled work cannot resume or change the
  new target. Changing smoothing alone should not dirty a document.


## Pass 17 — PDF backgrounds, canvas sizing, and session defaults

Implemented; committed as `9a8295f` at the start of Pass 18:

- PDF import offers White (default) and Transparent page backgrounds. Rendering
  starts with that background and retains straight RGBA opacity; transparent
  import does not remove actual white content in a PDF. The choice applies to
  new images and Open as Layers without adding an artificial background layer.
- New layered imports offer First Page or Fit All Pages canvas sizing. Fit All
  takes the maximum selected width and height after the renderer's DPI rounding,
  accommodating mixed portrait/landscape pages without resampling their pixels.
  Every page is centered with the same integer rounding, including the first.
  Separate images keep their own dimensions; existing-layer imports keep their
  destination canvas unchanged.
- The dialog shows resulting canvas dimensions and warns when selected pages
  extend outside the first-page or existing canvas. Full page pixels stay in
  their layers. Page-range/DPI and composite-canvas validation run for the live
  readout as well as Import; combined width/height maxima must fit the canvas
  pixel budget even when every individual page is otherwise valid.
- Successful imports remember DPI/background for the workspace session.
  Successful new-document imports also remember Layers/Images and canvas policy;
  Open as Layers cannot overwrite those new-document choices. Cancel, failure,
  stale targets, or refused publication do not update defaults. Ranges start
  fresh per file. Preferences are session-only and are not project metadata.
- New layered preparation verifies the complete page count, page sizes, layer
  capacity, and pixel admission, and rejects incomplete insertion rather than
  publishing a prefix. Existing queued imports, immutable source snapshots,
  cooperative cancellation, target-generation checks, and one-step insertion
  history retain their contracts.
- Updated README and AGENTS. No new toolbox tool, dependency, project format,
  package version, installer, icon, or release publishing change. The broader
  PDF fixture matrix, noncontiguous pages/thumbnails, interruptible workers,
  incremental preparation, and source reimport metadata remain planned.

Automatic validation: formatting, Clippy with warnings denied, all 369 tests,
and both release benchmarks pass. Five new regression tests cover actual PDF
transparent/white rendering with translucent art and painted white, combined
canvas preflight, consistent odd-dimension centering without resampling, separate
image sizing, complete-range/stack rejection, and remembered-default target
semantics. Existing cancellation, immutable-source, import-budget, and one-step
history tests remain passing; the insertion test also verifies existing canvas
and full page dimensions. `dist/Lime.app` is rebuilt and its signature verified.
No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Import a PDF with unpainted margins, translucent art, and painted white shapes
  on White and Transparent. Check canvas/thumbnail checkers, source opacity,
  save/reopen, and flattened PNG export. Actual white artwork should remain white.
- Import mixed portrait/landscape and odd-sized pages using First Page and Fit
  All Pages, change DPI and range, and check the canvas readout and clipping
  warning. Confirm all page layers keep full pixels and align consistently.
- Compare As Images and Open as Layers. Separate tabs should use each page's
  size; existing images must keep their canvas. Undo one existing-layer import
  should remove its entire selected range.
- Queue several PDFs and confirm successful choices carry forward. Cancel an
  import, use invalid/oversized choices, or let a target become stale, then check
  defaults stay at the last successful settings. Open as Layers must preserve
  the remembered new-document mode/canvas policy; restart resets session defaults.


## Pass 18 — independent region settings and optional line-art gap closure

Implemented; committed as `350e510` at the start of Pass 19:

- Wand and Bucket remember independent Tolerance and Gap radius values during
  the workspace session. Both start at zero. Typed edits and steppers share
  0–255 tolerance and 0–8 image-pixel radius bounds. Numeric field identities are
  pinned to their tool, including empty/Escape restoration after context changes;
  preferences alone do not modify pixels, selections, history, or saved revision.
- Nonzero radius closes small breaks in a temporary nonmatching-boundary mask
  before four-connected traversal. Square max/min closing reuses a linear-time
  separable extrema kernel with two byte buffers. Finite-source erosion ignores
  nonexistent neighbors, including near image edges and on thin sources. Source
  colors and original nonmatching boundaries are never modified by closure.
  Large radii can seal narrow passages or small regions; 0 retains the previous
  flood/fill path without allocating a boundary mask.
- Wand closes in source-pixel space, preserves off-canvas connectivity while
  clipping the output to the canvas, and performs creation feathering only after
  region detection, before add/subtract/intersect. Existing selection-only undo
  and saved-state behavior remain in the normal selection job.
- Active-layer Bucket keeps layer-local and unrestricted off-canvas coverage.
  Sample All Layers closes boundaries in the visible composite, respects the
  selection's traversal domain, and writes only the active layer. Soft selection
  coverage, grayscale, alpha locks, and transparent hidden-RGB rules remain in
  effect. Selection mode hides/ignores connected-only match and sampling controls
  while preserving their values; switching modes finishes any inline numeric edit.
- Ordinary selection Grow/Shrink retain zero outside coverage despite sharing
  the running extrema kernel. Added help/tooltips and updated README/AGENTS.
  No new toolbox tool, dependency, project-format, package-version, publishing,
  installer, or icon changes. Wand antialiasing and improved coverage around
  antialiased artwork remain planned independently of this hard-boundary feature.

Automatic validation: formatting, Clippy with warnings denied, all 375 tests,
and both release benchmarks pass. Six new regression tests cover useful versus
oversized openings, 360 deterministic comparisons with an independent binary
closing/connectivity oracle (including thin sources, hidden RGB, tolerances,
and clipped offsets), selection-domain limits and invalid seeds, active/merged
fill agreement, original-ink preservation, off-canvas/soft-mask/grayscale/alpha
lock behavior, and whole-selection independence from connected-only settings.
The existing numeric-bounds test now includes both tools' tolerance and radius
fields, and ordinary Grow/Shrink's reference tests remain passing.
`dist/Lime.app` is rebuilt and its signature verified. No interactive or visual
smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Set different Wand/Bucket tolerances and radii, switch tools repeatedly, type
  values past the limits, clear a field, and press Escape. Verify each tool keeps
  its own settings and preference changes do not dirty the document.
- Select/fill line art with small and large openings at radii 0, 1, 2, and 8.
  Try breaks near image edges and narrow passages. Confirm off retains the old
  result, useful gaps stop leaking, and original boundary pixels are preserved.
- Compare active-layer and Sample All Layers on offset layers with transparency,
  soft selections, grayscale, and alpha lock. Check Wand feather/combinations,
  one-step selection/fill undo, and the saved marker after undoing back to saved.
- Change zoom/pan and confirm radius remains in image pixels. Switch Bucket to
  Selection while typing a match value; matching/sampling fields should hide,
  typing should stop, and whole-selection fill should ignore those settings.


## Pass 19 — precise rotation pivot editing

Implemented; committed as `4e145ff` at the start of Pass 20:

- Rotate now exposes a compact nine-anchor Pivot selector, signed canvas X/Y
  fields with tenths precision, and a contrast-rimmed circular crosshair. Presets
  refer to the original layer's corners, edge centers, and center. Dragging the
  pivot preserves its grab offset; dragging elsewhere rotates about it, with
  the existing absolute 15-degree Shift constraint.
- Pivot edits recalculate the original layer at the current angle. Choosing a
  pivot at zero angle changes only the frame, does not launch another raster
  preview, and does not create history or pending pixel edits. Center restores
  the existing centered rotation and its mixed-parity quarter-turn placement.
- Custom rotation shares `Warp::RotateAround` geometry across frame, bounds,
  reduced preview, and Apply. Integral quarter turns preserve exact RGBA pixels;
  fractional-grid placement uses the chosen resampler rather than an invalid
  lossless shortcut. Identity rotations retain exact geometry even for large
  finite pivots. Nonfinite or unplaceable results are rejected before allocation.
- Coordinates are quantized from absolute values, bounded, and restored exactly
  on empty numeric text/Escape. Apply and canvas gestures finish a numeric edit
  first. Cancellation clears pivot fields along with the angle editor; draft
  pivot state resets with each new target/session and is not project metadata.
- Apply stays one undo step and leaves the canvas selection in place. Existing
  source snapshots, opacity, layer-position/text guards, stack budgets, and
  preview coalescing remain in effect. Other transform anchors remain a separate
  follow-up. Updated help, README, and AGENTS; no new toolbox tool, dependency,
  project format, package version, publishing, installer, or icon changes.

Automatic validation: formatting, Clippy with warnings denied, all 381 tests,
and both release benchmarks pass. Six new regression tests cover exact custom
quarter turns, fractional placement under all resamplers, independent rotation
angle/distance geometry, shared coarse/full bounds, invalid/oversized placement,
large-pivot identity previews, nine-anchor/default-center behavior, bounded signed
decimal editing/restoration, source sharing, and one-step Apply/undo/redo with
stationary selections and saved-revision restoration. Existing centered rotation,
perspective, layer-transform, and preview tests remain passing. `dist/Lime.app`
is rebuilt and its signature verified. No interactive or visual smoke tests run.

Manual interaction testing stays with the maintainer. Useful checks:

- Rotate offset and odd-sized layers using each anchor, custom positive/negative
  coordinates, and a dragged pivot at Fit/high zoom after pan. Check the crosshair,
  grab offset, tenths readouts, and Shift-constrained angles around the new point.
- Change the pivot at zero and nonzero angles, return to Center, and compare
  quarter turns with fractional custom pivots under all resampling choices.
  Choosing a pivot alone must leave pixels/history/saved revision unchanged.
- Clear or partially type either coordinate, press Escape/Return, click Apply,
  or start a canvas drag. Check original-value restoration, single-step undo/redo,
  and a stationary canvas selection. Try a far off-canvas pivot that exceeds
  placement limits, then restore a valid one and apply.
- Cancel a preview, switch layers/tabs/tools, and try locked-position or editable
  text layers. Confirm no pivot or late preview can affect a different target.


## Pass 20 — persistent named gradient library

Implemented and committed as `d4d44ad`:

- Replaced six numbered session slots with a personal library of up to 32 named
  gradients. Save… opens the existing text-field dialog; Rename and Delete act
  on the selected stable ID. Names trim surrounding whitespace, allow Unicode,
  reject controls, and cap at 64 characters; lowercase-equivalent names collide.
  Matching colors may be saved under different meaningful names.
- Entries preserve literal RGBA stops (including hidden RGB at zero alpha),
  Linear/Radial shape, and Reverse. Loading enters Custom mode while keeping
  swatches and canvas endpoints; deleting leaves the current palette available.
  Naming/loading first finishes inline stop edits so empty/restored text cannot
  capture stale values or alter a newly loaded palette afterward.
- Saved gradients is folded by default, shows the selected name or entry count,
  and expands to a bounded scrollable list. Long names truncate with full-name
  tooltips. Storage progress and a compact error/Reload action stay in Tool
  Options; actual write failures also report through the workspace error UI.
- A bounded, versioned JSON file lives in macOS Application Support/com.eimg.lime
  or Windows LOCALAPPDATA/Lime. The library reloads with a new workspace/app;
  loading does not automatically select or apply a gradient. Its data is not
  project metadata and never changes image history or saved revision.
- Initial reading, validation, and persistence run off the UI thread. Mutations
  are serialized, remain candidates until successfully written, and then publish
  to the UI. Same-directory temporary replacement syncs file data before rename;
  OS locks serialize Lime writers, and original-byte checks reject conflicting
  snapshots. Corrupt, oversized, unknown-version, and changed files remain
  protected. Failures preserve accepted in-memory entries and require Reload.
- Close/Quit waits for an active library write before normal unsaved-image checks;
  failed persistence cancels that deferred close and reports the failure. File
  reads are capped at 256 KiB. Decoding validates unique monotonic IDs, names,
  shapes, color bytes, stop count, endpoints, and strict interior ordering.
- Updated help, README, and AGENTS. No new toolbox tool, dependency, project
  format, package version, publishing, installer, or icon changes.

Automatic validation: formatting, Clippy with warnings denied, and all 387 tests
pass (the two existing performance benchmarks remain ignored for this storage/UI
pass). Six new regression tests cover exact named RGBA/shape/direction round
trips and stable IDs, name/capacity rejection without mutation, malformed/future
wire validation, real filesystem save/reopen/rename/delete, stale and locked
writers, protected corrupt/oversized files, I/O failures, and temporary-file
cleanup. Existing gradient sampling, preview/Apply, selection, and history tests
remain passing. `dist/Lime.app` is rebuilt and its signature verified.
No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Save named Linear/Radial/reversed palettes with transparent and intermediate
  stops; restart the app and load them. Confirm literal colors, shape, direction,
  swatches, endpoints, and current document saved state. Renames/deletions should
  survive another restart without changing the current gradient or image undo.
- Try duplicate names with different case/whitespace, long Unicode names, Cancel,
  and the 32-entry limit. Expand/fold the list, scroll it, and check name tooltips
  and Layers space. Load or Save while a stop field is partially typed/empty.
- Exercise reload after a file changed in another window, malformed or newer
  library data, and an unwritable storage location. Failed saves must leave the
  displayed accepted library intact and report the failure without replacing
  the protected file. Use a disposable test profile for malformed-file checks.
- Save a library change and immediately close/quit, including with unsaved image
  work. The write should finish before the normal close decision. Changing tools
  or colors during a write must not replace the active gradient with old data.

## Pass 21 — precise Scale anchors and Perspective corners

Implemented and committed as `de9082f`:

- Scale Layer offers a nine-point Size anchor for typed Width/Height edits,
  initially Center. The numeric session captures that choice and calculates
  placement from original bounds, preserving absolute-coordinate rounding.
  Choosing an anchor alone changes no image/history; pointer handles retain
  their opposite-edge/corner behavior. Unplaceable coordinates are rejected
  before pixel resampling rather than narrowed or clamped into another location.
- Perspective offers a spatial 2×2 corner selector and signed canvas X/Y fields
  in tenths of a pixel. Pointer grips select the corresponding point, quantize
  to the same precision, and highlight it with an accent while retaining the
  dark contrast rim. Rotate, Scale, and Perspective share the compact point-grid
  control; labels use regular option styling.
- Coordinate editors carry the original corner index. Corner selection, reset,
  Apply, and a new canvas gesture finish the prior numeric edit first; empty
  text and Escape restore its exact original coordinate. Existing tab/tool/layer
  cancellation, locked-position, and editable-text guards remain in force.
- Reset Corner restores just the selected point to its source location, keeping
  other edits; Reset All restores the original rectangle. These are draft-only
  changes. Invalid/crossed/concave or oversized quads retain their correction
  controls with Apply disabled. Full-resolution Apply uses the immutable source
  and commits one undo step with stationary canvas selections.
- Updated help, README, and AGENTS. No new toolbox tool, project metadata,
  dependency, package version, publishing, installer, or icon changes.

Automatic validation: formatting, Clippy with warnings denied, and all 390 tests
pass. Three new regression tests cover all nine anchored placements, negative
and odd-size rounding, coordinate overflow without partial resampling, and
Perspective precision, invalid input, invalid geometry recovery, partial/all
reset, and source sharing. Expanded existing checks cover all ten transform
coordinate fields with unique IDs, exact numeric restoration, and both custom
rotation and Perspective Apply/undo/redo with saved-state and canvas selection
preservation. Both release benchmarks pass. `dist/Lime.app` is rebuilt and its
signature verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Resize offset, odd-sized layers using each Size anchor with aspect lock on/off,
  then undo/redo. Change the anchor after typing a dimension; only the completed
  edit should land. Confirm pointer handles retain their existing anchors and
  choosing an anchor alone does not dirty the document.
- Drag and select all four Perspective corners at Fit/high zoom after pan. Check
  highlighted grip/selector correspondence, tenths readouts, negative coordinates,
  and numeric X/Y preview. Type an empty/partial field, then Escape, switch
  corners, reset a corner, or Apply; the previous field must resolve correctly.
- Cross corners or enter an unplaceable point, then correct it or use Reset
  Corner/Reset All. Verify the other corner edits survive a single reset, Apply
  stays unavailable for invalid geometry, and no discarded preview reappears.
- Apply under each resampler, undo/redo, and switch tools/layers/tabs during a
  preview. Check unchanged selection coordinates, source layer opacity/order,
  and the existing position-lock and editable-text restrictions.

## Pass 22 — proportional Fit Layer to Canvas

Implemented and committed as `2d46327`:

- Reviewed [GitHub issue #1](https://github.com/eimg/lime/issues/1), “Layer resizing
  issues.” Aspect locking and Shift reversal were already implemented in Pass 1.
  Off-viewport handles still needed a convenient alternative; typed dimensions
  worked, but Layer to Image Size crops/pads and does not proportionally fit.
- Added Fit Layer to Canvas to the native Layer menu and Scale Layer options,
  using the same action and existing scale icon. It scales the whole raster,
  including transparent margins, to the largest proportional size inside the
  canvas and centers it. Smaller layers may enlarge. Integer rounding bounds the
  result to at least one pixel per dimension; odd leftover margins differ by one
  pixel. The selected Nearest/Bilinear/Bicubic resampling controls the result.
- Computes target bounds without pixel allocation, validates image/stack budgets,
  then scales and positions through the background edit pipeline. Requires a
  visible raster layer with position unlocked; alpha lock permits geometry as
  before. Canvas size, selections, other layers, and layer metadata stay intact.
- One fit is one undo step. Repeating an already fitted result is a no-op; an
  already correctly sized but displaced layer only recenters and shares its
  pixel buffer. The source's old off-canvas location does not crop the scaled
  content. Existing generation checks prevent a stale job publishing elsewhere.
- Fit finishes a numeric edit first and waits for asynchronous Scale/Move/
  selection commits before running. The deferred operation validates tab,
  revision, tool, and original active layer, and the option button lets that
  action own numeric commit timing. Stale context cancels the deferred action.
- Updated tool help, README, and AGENTS to distinguish proportional fitting from
  Layer to Image Size. No new toolbox tool, dependency, project metadata, package
  version, publishing, installer, or app icon changes. GitHub issue remains open;
  its expected capabilities are now implemented locally, with interaction QA
  left to the maintainer.

Automatic validation: formatting, Clippy with warnings denied, and all 394 tests
pass. Four new regression tests cover landscape/portrait fits, enlargement,
rounding, extreme dimensions, zero-size rejection, complete off-canvas source
sampling, all resamplers, metadata/alpha-lock preservation, hidden/position-lock/
text rejection, stack-budget rejection before mutation, same-size recentering,
repeat no-ops, and one-step undo/redo with stationary selections and saved-state
restoration. Existing numeric follow-up checks now reject a different active
layer for deferred Fit. The shared native-menu tree and menu-icon tests cover
both menu variants and the reused scale glyph. Both release benchmarks pass.
`dist/Lime.app` is rebuilt and its signature verified. No interactive or visual
smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Paste/import a layer much larger than the canvas so no corners can be reached.
  Use Fit Layer to Canvas from both entry points; verify the complete image fits,
  is centered, and keeps its proportions. Try wide/tall layers, transparent
  margins, positive/negative offsets, each resampler, and smaller layers.
- Repeat Fit, move a fitted layer and Fit again, then undo/redo. Check no history
  for a repeated fit, one step for recentering, unchanged canvas/selection, and
  unchanged layer opacity/order/locks. Compare Layer to Image Size separately:
  it should retain its crop/pad behavior.
- Type a Scale dimension or Move coordinate, leave it empty/partial, and invoke
  Fit through the option button or native menu. Check the field finishes or
  restores first, then Fit applies to the settled layer. Switch tab/tool/layer
  during a pending numeric commit to confirm stale follow-ups do not fit another
  target. Try hidden, position-locked, editable-text, and memory-limited layers.
- Confirm aspect locking starts on, Shift reverses it during a handle drag, and
  turning the lock off allows Shift to constrain the original proportions.

## Pass 23 — optional Magic Wand contour antialiasing

Implemented and committed as `ebe7b19`:

- Magic Wand has an independent Antialias edges toggle in the existing Edges
  options. It starts off, is remembered for the workspace session, and affects
  new wand selections only. Toggling the preference does not alter the current
  selection, source artwork, document revision, or undo history.
- Region matching, transparent-RGB treatment, four-connected traversal, and gap
  closure still determine a hard region first. Optional antialiasing then
  reconstructs that contour with sixteen bilinear subpixel samples per pixel,
  before creation feathering and Replace/Add/Subtract/Intersect combinations.
  Fractional coverage never becomes input to flood connectivity.
- Straight boundaries and uniform interiors stay sharp. Corners and thin details
  can receive partial coverage; disable antialiasing for exact pixel coverage.
  Sealed gap barriers and explicit selection limits remain strictly excluded.
  This is contour reconstruction, separate from color-aware edge estimation or
  source-color decontamination around already antialiased artwork; that roadmap
  item remains unchecked. Bucket Fill keeps its existing hard matching behavior.
- A 512-entry lookup handles 3×3 neighborhoods without a supersized intermediate
  image. The flood's existing byte-per-pixel visitation buffer records admitted
  pixels in place, and the clipped output is written into its existing mask.
  Source-border neighborhoods clamp instead of fading; canvas clipping retains
  off-canvas source neighbors so it cannot create a false soft seam.
- Off retains the existing hard flood path, and zero gap radius still allocates
  no gap barrier. Both active-layer and merged sampling keep their background
  selection jobs and existing generation/cancellation rules. Soft coverage uses
  the normal mask consumers, selection history, and saved-revision behavior.
- Updated help, README, and AGENTS. No new toolbox tool, dependency, project
  format, package version, publishing, installer, or app icon changes. The
  GitHub issue remains untouched as requested.

Automatic validation: formatting, Clippy with warnings denied, and all 399 tests
pass. Five new regression tests cover contour symmetry/complements, exact
straight edges and uniform interiors, fractional corners, disconnected diagonal
components, invalid seeds, one-pixel-wide sources, negative offsets and clipping,
transparent hidden RGB and alpha boundaries, sealed gaps and strict limits,
feather-before-combination ordering, and selection undo/redo targets without
pixel mutation or dirtying the saved revision. Expanded existing fill/Clear and
clipboard tests verify the same fractional coverage reaches real pixel writes.
Existing hard flood/connectivity and independent gap-closing oracle tests pass.
Both release benchmarks pass. `dist/Lime.app` is rebuilt and its signature
verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Toggle Wand Antialias edges and select staircase/diagonal, curved, and very thin
  regions on small images. Compare Off and On at high zoom; straight boundaries
  and solid interiors should stay sharp, while corner coverage becomes partial.
  Changing the toggle alone should leave the current ants/mask/history intact.
- Try active-layer and Sample All Layers sampling on offset/partly off-canvas
  layers and at Fit/fixed zoom after pan. Compare clipped canvas edges with the
  same region before clipping; there should be no artificial softened seam.
- Combine Antialias edges with Gap radius and creation Feather, then exercise
  Replace/Add/Subtract/Intersect, fill, Clear, painting, and clipboard. Check
  sealed gaps do not reopen and existing selection coverage remains unchanged
  outside the incoming region. Undo/redo selections must preserve saved state.
- Try fully transparent pixels with different hidden RGB, alpha boundaries,
  tolerance endpoints, uniform sources, and one-pixel-wide images. Off should
  preserve the earlier exact hard-selection behavior.

## Pass 24 — noncontiguous PDF page selection

Implemented and committed as `92badf9`:

- PDF import now offers All Pages or Choose Pages. The custom field accepts
  comma-separated one-based pages and inclusive ranges, for example `1, 3-5, 8`.
  Hyphen, en dash, and em dash work as range separators. Duplicates and overlapping
  ranges import once, in ascending document order; this does not reorder pages.
- Replaced contiguous first/last options with an explicit selected-source-page
  list throughout preflight, rendering, new-document preparation, and existing
  layer insertion. Only selected pages contribute dimensions, memory, and canvas
  bounds or get rendered. First Page means the first selected page. Page layer
  names, numbered image titles, and render progress retain source page numbers.
- A bounded parser checks the source count and endpoints before range expansion,
  rejects incomplete/reversed/out-of-range syntax, and caps input at 2048
  characters. Core import validation independently requires a nonempty unique
  ascending list within the existing 128-page import limit and source bounds.
  Preparation also rejects mismatched page counts before creating documents or
  inserting layers, keeping publication atomic.
- Live estimates and Import share the same page parser for both selection modes.
  The dialog shows selected count alongside dimensions/clipping information.
  The page field scrolls to its caret for long lists and supports the existing
  clipboard/navigation behavior. Tab cycles DPI/page fields in Choose Pages;
  it no longer changes All Pages into a custom selection by itself.
- Existing source snapshots, cancellation checks, target-generation checks,
  destination stack preflight, one-step Open as Layers undo, remembered import
  settings, and unsaved-project destinations remain in use. Page choices reset
  per source. Thumbnail previews remain an unchecked, separate follow-up.
- Updated README and AGENTS. No new toolbox tool, dependency, project format,
  package version, publishing, installer, app icon, or GitHub issue changes.

Automatic validation: formatting, Clippy with warnings denied, and all 404 tests
pass. Five new regression tests cover range normalization, Unicode dashes,
ordering/deduplication, malformed/incomplete/bounded input, direct page-list
validation, atomic field paste rejection, selected-only rendering/canvas
preflight with an oversized unselected page, source-page progress, and
cancellation before a later selected page. Existing preparation checks now use
noncontiguous page numbers for document/layer names and cancellation/one-step
undo, and reject duplicate, unsorted, invalid, and incomplete preparation lists.
Both release benchmarks pass. `dist/Lime.app` is rebuilt and its signature
verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Import `1, 3-5, 8`, duplicate/overlapping entries, reversed input order, and
  pasted en/em-dash ranges through Open and Open as Layers. Check source page
  numbers in progress, layer names, image titles, and ascending stack order.
- Compare All Pages and Choose Pages, type incomplete/out-of-range expressions,
  and exercise Tab/Shift-Tab, arrows, Home/End, selection, and copy/cut/paste.
  Long lists should stay within the field and follow the caret. Switching modes
  must keep the hidden custom text while All Pages ignores it.
- Omit a large page and compare First Page/Fit All Pages canvas sizes, clipping
  warnings, page backgrounds, and memory admission. Choose a first page other
  than PDF page 1; its dimensions should establish the First Page canvas.
- Cancel during a later selected page, cancel preparation, change the target
  while work is pending, and undo/redo a successful layer import. Check no
  partial pages publish and that one undo removes the whole imported subset.

## Pass 25 — PDF page picker and import dialog

Implemented; pending commit, including maintainer feedback on input behavior,
thumbnail selection, and the previously narrow/tall layout:

- The PDF dialog uses a wider two-column layout: settings beside the page picker
  at ordinary window widths, stacked content in narrow windows. Its body scrolls
  while the title and Cancel/Import footer remain visible. Source filename,
  selected count, canvas estimate, and clipping/admission messages stay available.
- Two-choice dialog controls reuse Tool Options' shared segmented picker,
  covering PDF All/None, import mode, background, canvas policy, New Image
  background, and effect Before/After. Previous/Next show left/right chevrons
  beside their labels, preserving navigation availability and shared action style.
- Page thumbnails load automatically in batches of three. Previous/Next browse;
  Selected brings the first chosen page into view. Clicking a card toggles that
  page, with check marks and accent borders showing inclusion. All/None shortcuts
  and an always-visible page field keep the same selection in sync.
- Thumbnail changes preserve every other chosen page and canonicalize the field
  into compact ranges. Removing the final page leaves an empty selection rather
  than reverting to All. Invalid partially typed lists are preserved and report
  an error on a thumbnail toggle. Empty/invalid selections, invalid resolution,
  memory rejection, and stale targets disable Import with an explanatory readout.
- PDF inputs now place the caret at a clicked glyph boundary, support drag and
  Shift-click ranges, and select all on double-click. Shared Field editing keeps
  partial-range typing, deletion, clipboard, and Shift+arrow/Home/End behavior.
  Hit testing accounts for actual font shaping, padding, horizontal scrolling,
  and Unicode dashes; long lists follow the caret. Tab cycles both inputs.
- Thumbnails use retained PDF bytes independently of import DPI, with each axis
  bounded to 1–144 pixels, including huge/thin pages. White/transparent backgrounds
  and the existing layer thumbnail checker are respected. One displayed batch
  is retained, without a persistent cache.
- Background preview work coalesces source, batch, and background requests. A
  single preview worker per workspace cancels old work, waits for it to return,
  and rejects stale generations across closed/reopened dialogs. Published/stale
  images are explicitly dropped. A per-source gate serializes thumbnail and final
  import parsing/rendering; cancellation is checked around acquisition and between
  calls. Active parser/render calls still finish before cancellation settles.
- Per-page render errors stay local with details and Retry previews; parser/batch
  failures are shown in the picker. Preview failure does not replace final import
  validation. Existing source snapshots, page/stack budgets, publication checks,
  one-step layer import undo, and session import defaults remain in use.
- Updated README/AGENTS. No new toolbox tool, dependency, project metadata,
  package version, publishing, installer, app icon, or GitHub issue changes.

Automatic validation: formatting, Clippy with warnings denied, and all 416 tests
pass. Twelve new regression tests cover bounded thumbnail scaling and real PDF
alpha/painted white, huge/thin pages, source deletion, renderer fallback geometry,
invalid batches, queued source cancellation, stale/coalesced requests, batch
navigation, pointer byte/character boundaries, padding/scroll hit coordinates,
caret insertion, partial Unicode replacement/deletion and field limits,
All/custom/empty thumbnail toggles, compact ranges, invalid edits, and text-budget
rejection. Existing PDF import/selection/history/clipboard tests remain passing.
Both release benchmarks pass. `dist/Lime.app` is rebuilt and its signature
verified. No interactive or visual smoke tests are run.

Manual interaction testing stays with the maintainer. Useful checks:

- Click before/between/after DPI digits and page-list characters, drag selections
  inside and outside the field, Shift-click/Shift-arrow/Home/End, and double-click.
  Type/cut/paste over a partial range; check Unicode dashes and scrolled long lists.
- Toggle thumbnail pages from All, custom ranges, and None; remove the last page,
  then select again. Check list/card synchronization, compact ranges, Import
  availability, and preserved partial invalid input on an attempted card toggle.
- Check the wider layout at ordinary sizes and stacked fallback in narrow windows;
  shrink the height and scroll while keeping the title and footer reachable.
- Navigate and change backgrounds quickly, retry a failure, close/reopen another
  PDF, or start/cancel Import during a preview. No stale images should reappear;
  final DPI, page selection, source numbers, and one-step undo must stay correct.
