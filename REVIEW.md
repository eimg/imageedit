# feedback-pass correctness review

Reviewed 2026-11-12: `feedback-pass` at `b1dd346`, compared with `main`.
The fixes below are in the working tree. This review covered source, state
transitions, background edit publication, pixel/geometry behavior, and automated
checks. Interactive UI testing was omitted at the user's request.

## Findings and fixes

| Priority | Finding | Correction |
| --- | --- | --- |
| P1 | Finishing text removed the draft before rasterization and layer-budget validation succeeded. A rejected commit returned no patch, but its deferred Save or other command still ran. | Each tab retains the draft until successful publication. Failure cancels the dependent command, keeps the text as unsaved work, and restores editing on that tab, including after switching away and back. See `Tab::settle_text_commit`, `finish_change`, and `recover_pending_text`. |
| P1 | Text hit-testing could use a previous asynchronous layout after the source changed. Its byte offset could exceed the new string or split a UTF-8 character, causing a panic on the next edit. | `TextDraft::hit` accepts only a layout matching the current source/style. A pending layout leaves the existing caret intact. |
| P1 | A Scale Layer drag and its overlay survived tab changes, tab closure, and active-layer changes. Releasing on a different document could scale a layer with the same numeric ID there. | Those transitions cancel the drag and release its preview. Save/Export and close confirmation recognize an active scale gesture. Escape cancels it too. |
| P2 | Layer commands captured the active layer before the text commit inserted a new text layer. Commands such as Duplicate or Align could then modify the previous raster layer. Transform/scale entry points also checked stale content. | Relevant command handlers finish text first and rerun after publication, resolving their target and checking pixel-tool restrictions against the committed state. |
| P2 | Cropping or shrinking the canvas rebuilt ellipses and rounded rectangles inside their clipped bounding box, changing which remaining pixels were selected. | Preserve the curve's original coverage and convert a partially clipped shape to a bounded mask. |
| P2 | Scale Image kept a rounded selection's original corner radius, even when image dimensions doubled or the axes scaled differently. | Scale its coverage as a mask so corner dimensions follow both axes. |
| P2 | Clone Stamp rounded the destination pointer, while the source picker used the containing pixel. Clicks in the second half of a destination pixel, including its center, sampled one pixel away. | Compute the offset using the destination's containing pixel; test it on an offset layer. |
| P2 | Closed-path simplification removed the shared farthest vertex twice while joining its two halves, distorting complex Selection to Path results. | Keep the shared vertex once and remove only the repeated closing endpoint. |
| P2 | Switching tabs cleared a live numeric selection preview without recording its value in the original tab. | Commit the numeric edit before clearing the tab's transient selection state. |

Related corrections preserve the opposite scale edge at the size limit, consume
the release position before scaling, and carry the last edited text style into
new text even when that style came from an existing layer.

## Verification

- Baseline: formatting and Clippy passed; 208 tests passed, two benchmarks ignored.
- Added regression coverage for stale UTF-8 caret offsets, rejected text
  rasterization, full layer stacks, retained unsaved drafts, curve clipping,
  Clone Stamp sampling, rounded-selection scaling, and closed-path simplification. Extended scale geometry
  coverage for the maximum-size clamp.
- Final checks and bundle verification are recorded in `IMPLEMENTATION_PLAN.md`.
- No interactive UI or Windows runtime checks were performed.

For the next manual pass, concentrate on text followed by Duplicate/Align/Save,
an oversized text draft followed by Save and tab switching, changing tabs during
a scale drag, curved selections after cropping, and a one-pixel Clone Stamp.
