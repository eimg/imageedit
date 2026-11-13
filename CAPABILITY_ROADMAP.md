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
- [ ] Optional antialiasing for polygon and wand selection edges.
- [ ] Optional feathering at selection creation with an explicit radius.
- [x] Preserve soft coverage through Grow/Shrink and combinations.
- [ ] Define feathering at canvas boundaries consistently.
- [x] Exact geometric selection position/size controls and fixed-ratio constraints.

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
- [ ] Define stroke opacity separately from dab/flow buildup; expose incremental
      behavior deliberately rather than making spacing determine the meaning.
- [ ] Optional stroke smoothing with bounded latency.
- [ ] Predictable strength and repeat-pass behavior for Blur/Sharpen.
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

- [ ] Alpha lock for safe recoloring.
- [ ] Position lock for protected backgrounds.
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
- [ ] Add per-page progress and cancellation, including long probes/renders.
- [ ] Preflight the destination stack budget before Open as Layers renders pages;
      consider incremental rendering/publication to bound peak working memory.
- [ ] Preview page thumbnails and allow noncontiguous page selection.
- [ ] Offer white versus transparent page background and remember import options.
- [ ] Offer layered canvas sizing (first page versus largest selected page), with
      an indication when larger page layers extend outside the first-page canvas.
- [ ] Detect a source PDF changed between probe and render, or retain a consistent
      source snapshot for the import session.
- [ ] Expand automated fixtures for CropBox/rotation, fonts, transparency, images,
      and representative malformed/encrypted documents on macOS and Windows.
- [ ] Consider storing source/page/DPI metadata for explicit reimport later.

The fixes above are part of Pass 4. Cancellation, previews, source tracking, and
the other unchecked options are follow-up capabilities rather than claims of
current support. Existing source PDFs are never overwritten by Save.

## Adjustments and filters

- [ ] Preview within the full composition, including selection and layer opacity.
- [ ] Before/after comparison and convenient reset.
- [ ] Remember prior parameters and repeat the last operation.
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

Implemented, uncommitted:

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
