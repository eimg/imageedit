# Lime capability roadmap — local working document

Requested by the maintainer. Keep this document uncommitted so we can revisit
scope, implementation status, and manual feedback between passes. GIMP is a
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

- [ ] Optional antialiasing for curved, polygon, and wand selection edges.
- [ ] Optional feathering at selection creation with an explicit radius.
- [x] Preserve soft coverage through Grow/Shrink and combinations.
- [ ] Define feathering at canvas boundaries consistently.
- [ ] Exact selection position/size controls and fixed-ratio constraints.

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
- [ ] Crop aspect ratios and consistent dimension constraints.

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

- [ ] Partial range selection using dragging and Shift+arrows.
- [ ] Word navigation and deletion, with Unicode-aware boundaries.
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

Implemented, uncommitted:

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
rebuilt. All Pass 3 changes remain uncommitted, including the new guide modules;
the roadmap itself remains untracked as requested.

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
