# ImageEdit implementation plan

Baseline assessed at commit `5c4cfcf` on `main`. This document specifies the proposed behavior and acceptance criteria for future work.

## Latest handoff · P02

P02 switched project saves to the v2 wire format and kept the v1 reader. It is on branch `p02-project-v2`. The first commit, `590f1bc`, adds a v1 golden fixture written by the baseline v1 writer. The P02 commit follows it. No menu, shortcut or tab behavior changed.

- `fixtures/projects/v1-single-layer.imageedit` is the baseline-written 3 × 2 v1 file with opaque, translucent and fully transparent pixels. `fixtures/projects/v2-two-layers.imageedit` is a P02-written stack with non-sequential IDs, a non-ASCII name, a negative offset, a hidden layer, opacity 77 and translucent pixels. Tests read both. Do not regenerate them with a newer writer.
- `src/project.rs` dispatches on the manifest `version`. It reads v1 unchanged and imports its layer with ID 0. It reads and writes v2 as specified under *Project v2 wire format*. The v2 manifest and layer entries reject unknown fields. One `check_limits` function enforces the canvas, layer count, per-layer and total decoded size caps, and unique IDs for both reads and writes before any pixel buffer is allocated. The reader rejects a payload length that is zero or exceeds the remaining file, enforces the 768 MiB encoded total, checks PNG dimensions against the manifest, and rejects trailing bytes. The writer encodes one layer at a time into a reused buffer.
- `src/document/content.rs` adds `DocumentContent::from_layers`, which requires a non-empty canvas, at least one layer, non-empty layer pixels and unique IDs. It also adds the read-only `size()` and `layers()` accessors. `src/document.rs` exports `BlendMode`, `RasterLayer` and `MAX_IMAGE_BYTES`, and its save adapter writes every project through `project::write`. The v1 full-canvas restriction is gone. Atomic same-directory replacement and recent-document handling are unchanged.
- Formatting, Clippy and locked tests passed before and after the change. The baseline had 12 tests and the final run had 19. New tests cover the v1 golden file and its v2 rewrite, v2 round-trips against the writer and the golden file, 17 malformed manifests, 12 malformed payload or framing cases, writer limits, a failed save that leaves the original file reopenable with no leftover temporary file, a truncated project failing `Document::open`, and the constructor's validation. A malformed file fails in `Document::open` before a tab exists, so no tab can be mutated.
- The release bundle built and passed `codesign --verify --strict`. It launched with both fixtures and a truncated project from a temporary directory. The UI smoke test did not run because Computer Use lacked Accessibility and Screen Recording permission. The opened tabs, the error for the truncated file, and a Save and reopen through the UI remain unverified.
- Known gap for P04 and P05: opening a multi-layer v2 file makes the top layer active even if it is hidden, so Bucket Fill can edit an invisible layer. Save As to PNG, JPEG or WebP flattens a layered tab and switches it to that raster format. P04 owns that routing.
- **Next is P03.** Add transactions, history and save identity on top of the current crop and fill edits.

## Previous handoff · P01

P01 introduced the layer stack and Normal compositor behind the existing single-layer UI. It is the commit immediately after baseline `5c4cfcf`, and it was verified against that baseline before commit.

- `src/document/content.rs` holds `DocumentContent`, `RasterLayer`, and document-local `LayerId`. The active layer ID lives on the workspace tab. Cropping clips and rebases layers. If the active layer is removed, the tab selects the top remaining layer. If no layer overlaps the crop, the content retains a transparent canvas-sized layer.
- `src/document/composite.rs` clips signed offsets and composites visible layers in stack order. `src/document.rs` wraps content with file metadata and a rendered canvas. Ordinary single-layer documents reuse their layer pixels as the full-resolution composite. The canvas, eyedropper and raster saves use the composite.
- `src/project.rs` retains the v1 wire format. The save adapter rejects project contents that v1 cannot represent before replacing the destination. No menu or shortcut changed.
- Baseline and final formatting, Clippy, and locked tests passed. The final run had 12 passing tests. The macOS bundle built and passed signature verification. In the isolated app, a pre-P01 v1 file opened, its teal pixel sampled as `#14B8A6`, fill and crop produced a black 7 × 6 canvas, and Save cleared the unsaved marker. The written file contained a v1 manifest, the `Background` layer name and a 7 × 6 PNG payload. Raster open and 100% zoom were also observed. The UI automation intermittently lost the window, so the post-save UI reopen and sampling after pan were unverified. The automated format round-trip tests exercised reopening project and raster files.
- **Next is P02.** Capture a v1 golden fixture before switching writers. Add a validated construction path for loading multiple layers into `DocumentContent`, then implement bounded v2 read/write in `src/project.rs` while retaining the v1 decoder. Use the existing save adapter and keep the layer-creation UI for P05.

## Assessment

The 17-pass proposal has the right foundation. Layers, a single edit/undo boundary, and a preview that can update small regions are prerequisites for brush strokes and other interactive edits. The baseline supported that diagnosis. `Document` owned one `Arc<RgbaImage>`, project v1 stored one PNG, and `Workspace::edit_pixels` produced a replacement image and preview in a background job. The tab-level busy guard serialized edits and saves, which is unsuitable for processing every pointer event.

This plan changes the sequencing and resolves the decisions that would otherwise block agents.

- Split the model, file format, and preview work into independently verifiable slices. Leave working tools and shortcuts available during the migration.
- Deliver raster export before users can add a second layer. Deliver layer management before import, clipboard layer insertion, and layer-based filters. Once a document is project-backed, use Export to produce a raster copy and retain the project as the editing target.
- Move basic layer movement ahead of advanced selection. Keep selected-pixel movement separate from the layer offset operation.
- Keep the central menu order and action declarations in `src/commands.rs`. Move feature handlers out of `src/workspace/view.rs` as each feature lands. Multiple worktrees still share menu registration, workspace composition, and potentially project schemas.
- Use GPUI’s existing image clipboard API first. The pinned source exposes `ClipboardItem::new_image` and `ClipboardEntry::Image`, and the macOS backend reads and writes pasteboard image data. External app interoperability still needs a smoke test.
- Start Windows checks early. This machine has `x86_64-pc-windows-gnu` installed. The baseline offline check stopped at an uncached `core_detect v1.0.0` dependency, before compiling ImageEdit for Windows.
- Measure preview work with a repeatable release benchmark and a visible-stroke smoke test. A single wall-clock assertion in a debug unit test would be unreliable.

## Working product decisions

| Area | Behavior |
| --- | --- |
| Save | Save preserves the format of an existing raster-backed tab while its contents can be represented by that format. A tab with multiple layers or project-only content routes Save to an `.imageedit` destination. A project-backed tab stays project-backed. New documents default to `.imageedit`. |
| Save As | Preserve the existing raster option for eligible single-layer raster documents. Use `.imageedit` for projects and documents with project-only content. Save Project As always writes `.imageedit`. A rejected raster destination leaves the source file, tab path, and saved revision intact. |
| Export | PNG, JPEG and lossless WebP export the canvas composite and keep the current tab path, project format and saved revision. Prompt before replacing a destination and reject export over a file owned by another open tab or in-flight write. JPEG composites against white, with an explicit quality setting defaulting to 92. |
| Layers | Bottom-to-top stack, signed integer canvas offsets, 8-bit RGBA, visibility and per-layer opacity. Start with Normal source-over. Multiply, Screen, Overlay and additional modes are follow-up features. |
| Selection | Canvas-space coverage from 0 to 255. No selection means unrestricted editing. An explicitly empty selection affects zero pixels. Selection and active layer are session state. Selection changes can be undone and do not mark the project dirty by themselves. |
| Move | Move changes the active layer offset and leaves the selection mask in canvas coordinates. Defer floating selections and anchoring. Cut/Paste as New Layer gives users an explicit selected-pixel move workflow. |
| History | Start with 100 undoable transactions per tab, a 256 MiB per-tab target and 512 MiB app-wide target for history-only storage. Evict oldest retained entries first and charge shared backing allocations once. The current state remains usable even when a change exceeds the history budget. Treat these as tunable defaults and review them with the release benchmark. |
| Dependencies | Use existing crates and pinned GPUI first. For a new direct dependency, record its proposed version, license, footprint, alternatives and Windows support before adding it. `lyon` and `tiny-skia` are already transitive in the lockfile. Neither is selected for paths yet. Evaluate text shaping and rasterization separately. |
| Platform | macOS is the native UX target. Keep document math, project codecs, history and edit operations platform-neutral. Use the platform boundary for OS-specific UI and clipboard fallback. |

A single raster layer is eligible for raster Save only if its rendered canvas preserves the document’s editable meaning. Require one visible Normal raster layer at `(0, 0)` with opacity 255, canvas-sized bounds, its default import name, and no persistent extra content such as paths. Individual pixels may still be transparent. JPEG retains its existing lossy raster-save behavior. A project-backed tab uses Export even if it has only one raster layer.

## Contracts for implementation

### Document, edits and coordinates

- Introduce pure `DocumentContent` and raster layer types. Use a stable `LayerId` across reorder, undo, and async work. Store canvas dimensions separately from layer dimensions. Use checked arithmetic for rectangle ends, clipped intersections and byte budgets. A valid document always has at least one layer. Reject deletion of the sole layer.
- Keep path, file format, saved revision, view state, GPUI render images, and jobs outside the persistent content snapshot. Store the active layer by ID. If an operation removes it, select a deterministic neighbor. In a loaded project, select the topmost layer by default.
- Route persistent changes and undoable selection changes through a common transaction boundary. Capture tab ID and starting edit generation for async work. Publish only if both still match. A new committed edit clears the redo branch. A no-op or canceled preview creates zero history entries.
- Track two identities. The edit generation advances on edits and history navigation to invalidate stale jobs. The persistent revision identifies the content that Save writes. Save records the revision captured for the write only after success. Undoing or redoing to the saved revision clears the unsaved marker, even after a selection-only operation.
- Crop uses a canvas rectangle. Clip each layer to the crop, rebase remaining offsets, translate the selection and later any paths, and retain a transparent layer if everything is cropped away. Painting and bucket fill use active-layer pixels, mapping through the layer offset. The eyedropper samples the visible composite at full resolution.
- Preserve source alpha in PNG/WebP, blend source-over in a shared compositor, and use that same compositor for canvas, eyedropper, Merge Down and export. Start with straight-alpha RGBA storage and explicitly test compositing and interpolation at translucent edges. Keep rendering caches and preview pixels out of save, selection, and edit inputs.

### Project v2 wire format

Retain the current `IMGEDIT\0` magic and little-endian `u32` JSON manifest length. The manifest contains `version: 2`, canvas `width` and `height`, and a bottom-to-top `layers` array. Each raster layer entry stores `id`, `name`, signed `x` and `y`, `width`, `height`, `visible`, `opacity` as a byte, and `blend_mode: "normal"`. Each layer’s payload immediately follows in manifest order, prefixed by a little-endian `u64` encoded PNG length.

- Keep the current v1 reader and its original unframed single PNG layout. Import its layer as visible, opacity 255, Normal and origin `(0, 0)`. Open v1 without setting dirty. A subsequent project write uses v2. Capture a v1 golden fixture with the baseline writer before switching formats.
- Start with the existing 64 KiB manifest limit and 256 MiB decoded canvas limit. Limit v2 to 128 layers, 512 MiB total decoded layer pixels and 768 MiB total encoded payload. Apply the same caps to reading and writing. Check manifest lengths and aggregate dimensions before allocating pixel buffers.
- Reject unknown versions, invalid metadata, duplicate IDs, unsupported blend modes, truncated or oversized chunks, extra payloads and PNG dimensions that disagree with their layer. Bound reads to each payload. Encode one layer at a time and use the existing same-directory temporary-file replacement on save.
- Reserve v3 for image mode in P11, v4 for paths in P19, and v5 for editable text in P21. Treat v1 and v2 as RGB. Choose each new payload layout in its owning pass and document the reader/writer matrix. An older reader must reject a project it cannot fully represent instead of opening a reduced version that can later overwrite the source. Keep older readers and fixtures for all shipped versions. Omit transient previews, selections and history from saved projects.

## Execution and handoff

1. Read `AGENTS.md`, the host guide it references, this plan, and the most recent pass handoff. Check the working tree and baseline. Run `cargo fmt --check`, `cargo clippy --locked -- -D warnings`, and `cargo test --locked`. Attribute any baseline failure before editing and record it separately from pass regressions.
2. Implement the assigned slice through normal menu, toolbox or file entry points. Verify pinned GPUI APIs in local source. Put pixel and geometry tests in pure modules. Enable a planned action only when its normal entry point works and its availability reflects tab, selection, modal and job state.
3. Rerun the checks and build with `./scripts/bundle-macos.sh` when native behavior changes. Smoke-test the slice using synthetic files in a temporary directory and an isolated test bundle if a different ImageEdit build is already open. Record the actual checks, results, and any UI action the environment prevented. Capture screenshots where they demonstrate a visible change. Follow the host’s Olex instructions if network access is needed and restore its prior state.
4. Update `AGENTS.md` and `README.md` when user-visible behavior or the code map changes. In the handoff record the pass ID, tested commit or diff, changed modules, changed file-format or action contracts, verification and next eligible pass. Treat roughly 1,500 changed lines as a review signal. If exceeded, split at a tested boundary without enabling half a workflow.

## Implementation slices

The dependency list gives the earliest safe starting point. New module names are suggested ownership boundaries.

### P01 · Layered core and compositor

**Depends on** baseline. **Own** `src/document.rs`, new `src/document/composite.rs`, and the minimum workspace/file adapters.

Introduce the content model and compositor contracts above. Use one-layer documents for all existing entry points. Move full-resolution eyedropper, crop, fill, and raster save callers onto the layer/composite interfaces. Keep the existing preview and project v1 writes in this pass.

**Accept** source-over fixtures cover partial alpha and opacity, hidden layers, negative offsets and clipping. Import, fill, crop, sample, save, and v1 reopen through the existing UI.

### P02 · Project v2 and compatibility fixtures

**Depends on** P01. **Own** `src/project.rs`, project fixtures, and the save adapter.

Implement the v2 wire format and bounded readers. Write v2, read v1 and v2. Generate a two-layer fixture in tests before the UI creates layers. Keep atomic replacement and recent-document behavior.

**Accept** v1 golden file opens, v2 round-trips IDs/order/offset/alpha/name/opacity/visibility, malformed variants fail without mutating a tab, and a failed replacement leaves a valid original file.

### P03 · Transactions, history and save identity

**Depends on** P01 and P02. **Own** new `src/workspace/history.rs`, `src/workspace.rs`, `src/workspace/files.rs`, and Edit menu hooks.

Add one transaction per applied crop or fill. Store shared snapshots for now, with memory accounting and deterministic eviction across tabs. Bind Undo/Redo to `⌘Z` / `⌘⇧Z` on macOS, and `Ctrl+Z` / `Ctrl+Y` on Windows. Use the shared job and revision contracts.

**Accept** undo/redo a crop then a fill, undo to the last save, branch by editing after undo, hit both history caps, and simulate a failed or stale completion. History keeps its saved revision identity after eviction.

### P04 · Raster export and save routing

**Depends on** P03. **Own** `src/workspace/files.rs` and File menu hooks.

Add export for PNG, JPEG and WebP. Reuse atomic writes, canonical path ownership, and existing codecs. Apply the Save and Save As routing rules before making layered authoring available.

**Accept** export a dirty v2 fixture in each format, verify flattened pixels and format-specific alpha, and confirm the project path and dirty state survive. Exercise cancellation, invalid filename, failed write and an open-tab destination. Save on an edited raster tab continues to update its source when eligible.

### P05 · Layer panel and commands

**Depends on** P04. **Own** new `src/workspace/layers.rs`, inspector delegation, and Layer menu hooks.

Add layer selection, thumbnails, visibility, opacity, New, Duplicate, Rename, Delete, Raise and Lower. Add drag reordering after button/menu ordering works. New Layer defaults to a transparent canvas-sized layer and can use FG/BG fill. Merge Down composites the active layer and its immediate lower neighbor into their checked union, then replaces both with a visible Normal layer at full opacity. Disable Merge Down if either layer is hidden or there is no lower neighbor. Coalesce an opacity drag or reorder into one transaction.

**Accept** create, reorder, change opacity, duplicate, hide, rename, merge and delete in the real panel. Undo each mutation and reopen the saved v2 project. Verify Merge Down preserves the composite within 8-bit rounding in an alpha fixture and Save never flattens an edited layered document.

### P06 · Preview feasibility and benchmarks

**Depends on** P03. **Own** new preview modules and a release benchmark harness. No user-visible renderer switch.

Test 256 px tiles first and compare 512 px if measured overhead warrants it. Verify GPUI texture reuse, `drop_image` on replacement/close, visible-tile culling, mip or downsample strategy for Fit, and the effect of linear sampling at tile edges. Add a reproducible warm release benchmark on an 8192 × 8192 RGBA canvas and record macOS version, hardware, scale factor and fixture.

**Accept** produce a clear choice, baseline timings, invalidation counts and a native zoom/pan seam check for the prototype. Initial target is p95 under 8 ms of CPU work for a 64 × 64 interior update on the reference Apple Silicon Mac, excluding GPU presentation. Report boundary-crossing and full refresh separately. If tiling misses the target, record the bottleneck and evaluate a bounded live overlay before switching the renderer.

### P07 · Incremental preview and live edit path

**Depends on** P06 and P05. **Own** preview modules and canvas paint integration.

Key cached composite tiles by tab, resolution level and tile coordinate. Use the edit generation to validate in-flight results and carry unaffected cached tiles forward after an edit. Rebuild only tiles intersecting dirty bounds, including filter/sampling overlap. Use existing full rebuild behavior for canvas-wide edits until measured otherwise. Coalesce redraws while a gesture is active, discard stale work and preserve a fast display-only overlay path. Preview stays independent of save and history snapshots.

**Accept** a 64 × 64 update wholly inside one tile changes one tile at the base level. A boundary edit invalidates only touched tiles and required sampling neighbors. Measure P06’s benchmark, pan and zoom across seams, switch and close tabs during preview work, and verify resources are reclaimed.

### P08 · Selection basics

**Depends on** P03 and P05. **Own** new `src/selection.rs` and `src/workspace/selection.rs`; use the existing canvas geometry.

Implement a canvas-space selection with cheap unrestricted, empty and full cases. Bound any dense allocation to the canvas. Add Rectangle Select (`R`), Select All (`⌘A`), Select None (`⌘⇧A`) and Invert Selection (`⌘I`), and a visible contour that tracks zoom and pan. Use Ctrl for the corresponding Windows menu shortcuts. Use the selection for bucket fill and Clear to Transparency. Crop to Selection uses its nonzero bounding box and stays disabled for unrestricted or empty selection. Route selection changes through session history.

**Accept** selection tools work after panning and on offset layers, mask coverage affects writes, clear respects transparency, crop rebases the mask, and selection-only undo keeps saved documents clean.

### P09 · Layer movement and geometry

**Depends on** P07 and P08. **Own** `src/workspace/layers.rs` and new geometry helpers.

Implement Move (`M`) as a live offset preview with one transaction on release. Cancel restores the original offset. Add canvas-relative horizontal and vertical center alignment, Layer to Image Size, and layer/image Crop to Content based on nonzero alpha. For fully transparent content, keep dimensions and show a no-op. Leave Scale Layer for P11 so it shares resampling.

**Accept** move and align off-canvas and partly visible layers, compare before/after composite bounds, and verify Cancel, Undo, save and reopen preserve offsets. Active selections stay in canvas coordinates.

### P10 · Clipboard and Open as Layers

**Depends on** P08 and P05. **Own** new `src/workspace/clipboard.rs` and import hooks.

Use GPUI clipboard image entries with PNG first. Copy uses the active raster layer clipped by the selection. Cut encodes before clearing anything. GPUI returns no write status, so verify the image through clipboard readback or a platform API that reports success. Clear the copied pixels in one transaction after successful transfer. Paste and Paste as New Layer insert an independent raster layer centered on the canvas. New from Clipboard makes a canvas from the decoded image dimensions. Open as Layers imports selected raster files above the active layer in picker order, centered on the canvas, and reports failed decodes. Commit successful imports together.

**Accept** copy transparent pixels to macOS Preview or GIMP and paste an image from another app. Check cut/undo, empty or unsupported clipboard, multi-file import with one bad file, and the existing hex dialog’s text clipboard shortcuts. Add an OS fallback only for an observed backend gap.

### P11 · Fixed transforms and sizing

**Depends on** P09. **Own** new `src/transform.rs` and image/layer command handlers.

Add 90° left/right, 180° and flips. Image transforms move all layers and selection coordinates together. Layer transforms affect the whole active layer, leave the selection mask in canvas coordinates, and preserve the layer center as closely as integer offsets allow. Implement Scale Image and Scale Layer with aspect lock and shared nearest/bilinear/bicubic resampling, then Canvas Size with a nine-point anchor. Canvas Size translates content relative to the anchor and preserves off-canvas layer pixels. Grayscale converts existing pixels, preserves alpha, and constrains later pixel writes to gray while retaining RGBA storage. RGB enables color editing again and keeps the converted pixels. Persist the mode in v3 before enabling its menu. Route grayscale documents to project Save and allow raster Export.

**Accept** golden images include asymmetric pixels, offsets, transparent edges and odd-sized canvases. Verify command scope, anchor behavior, selection geometry, Undo and project reopen.

### P12 · Advanced selection

**Depends on** P08. **Own** selection modules. Use separate review checkpoints for geometry, region tools, and morphology.

Add Free Select (`F`) with freehand and click-to-close polygon modes. Add Magic Wand (`U`) and a shared tolerance control for connected bucket fill. Preserve exact RGBA matching at threshold zero. Use four-neighbor connectivity and compare the maximum absolute difference among RGBA channels to a tolerance from 0 to 255. Use Replace by default, Shift for Add, Control for Subtract and Shift+Control for Intersect when starting a selection gesture. Add Feather, Grow and Shrink dialogs using bounded mask operations.

**Accept** test polygon winding and clipping, corner-touching regions, alpha and tolerance endpoints, mask combination and morphology at image edges. Verify Escape discards an unfinished selection and selection dialogs create one history entry.

### P13 · Interactive transforms

**Depends on** P11 and P07. **Own** transform feature modules and canvas overlays.

Add Rotate (`Shift+R`), Flip (`Shift+F`) and Perspective (`Shift+P`). Reuse resampling and geometry from P11. Preview the whole active layer with handles while keeping the selection mask fixed in canvas coordinates. Commit only on Apply. Reject singular, self-crossing or resource-excessive geometry. Keep flipped/rotated content bounds and offset consistent with the preview.

**Accept** compare known perspective mappings and filter results. Exercise Apply, Cancel, a panned view, an invalid perspective quad, selection scope and Undo.

### P14 · Stroke engine and basic paint tools

**Depends on** P07 and P08. **Own** new `src/paint.rs` and paint tool options.

Build a dirty-region stroke buffer and stamp interpolation between sampled points. Add Pencil (`N`), Brush (`P`) and Eraser (`Shift+E`), with size, spacing, hardness, opacity and selection coverage as applicable. Eraser reduces alpha. Shift-click draws from the last committed stroke endpoint on the same layer. Release commits one edit, and Escape discards the overlay.

**Accept** compare equivalent fast and slow event sequences, test alpha and selection edges, undo one whole stroke, and draw at 100% on a 4K canvas with the P06 harness. Confirm no full-image clone or preview rebuild per pointer event.

### P15 · Gradient and expanded bucket fill

**Depends on** P12 and P14. **Own** paint and fill feature modules.

Add Gradient (`G`) with linear/radial geometry, FG→BG and FG→transparent, live handles and Apply/Cancel. Add bucket modes for connected region or entire selection. Sample All Layers computes connectivity from the composite and writes to the active layer only.

**Accept** check gradient endpoints, zero-length gesture, alpha, selection coverage and mask offset. Use a two-layer fixture where enabling Sample All Layers changes the region while leaving the other layer unchanged.

### P16 · Adjustment infrastructure and common adjustments

**Depends on** P07 and P08. **Own** new `src/adjustments.rs` and adjustment dialogs.

Build a shared modal with reduced-resolution preview, generation-aware slider updates and background full-resolution Apply. Scope to the active layer and blend effects by selection coverage while preserving alpha. Ship Brightness & Contrast, Hue & Saturation, Desaturate and Invert.

**Accept** identity settings produce no history entry. Rapid slider changes leave the latest preview visible. Cancel restores the original pixels and dirty state. Check Undo and pixel fixtures for each operation.

### P17 · Color Balance, Levels and Curves

**Depends on** P16. **Own** adjustment modules and histogram/curve controls.

Add Color Balance, Levels with histogram, and Curves with draggable control points. Share the adjustment pipeline and a bounded per-channel lookup table for Levels/Curves. Give histogram and curve endpoints defined behavior for uniform and transparent images.

**Accept** test identity and fixed control-point mappings, empty/flat histograms, bounds and monotonic x ordering. Verify parameter editing, Apply, Cancel and Undo.

### P18 · Filters

**Depends on** P05 and P16. **Own** new `src/filters.rs` and filter dialogs.

Add separable Gaussian Blur, Sharpen with an unsharp mask, and Drop Shadow. Blur and Sharpen process the active layer with selection coverage. Drop Shadow creates an undoable layer beneath the source from blurred source alpha, with a configurable offset and clipping/size checks.

**Accept** test impulse and constant images, transparent edges, a partial selection, shadow placement and Undo. Verify the shadow layer round-trips through a project.

### P19 · Editable paths

**Depends on** P12 and P03. **Own** new path modules and an explicit project format update.

Add Path (`B`) with cubic anchors, handles, close and edit gestures. Store paths in canvas coordinates. Implement Path to Selection using a defined fill rule and testable rasterization. Try adaptive curve flattening plus the existing polygon mask first. Add a vector dependency only if measured quality or functionality needs it.

**Accept** edit and reopen an open and closed path, round-trip control points and IDs, rasterize overlapping contours consistently, and undo handle changes. Update canvas-level transforms to move paths with the image.

### P20 · Stroke paths and selection edges

**Depends on** P19 and P14. **Own** path-to-paint integration.

Stroke either the active path or the selection contour using the paint engine. Define contour extraction and stroke placement in canvas space and convert to active-layer coordinates. Reuse brush settings and commit one transaction.

**Accept** verify open and closed paths, contours with holes, layer offsets, repeated endpoints and Undo.

### P21 · Editable text layers

**Depends on** P05, P19 and P03. **Own** new text-layer modules and a project format update.

Begin with a short spike that produces RGBA pixels offscreen and verifies export at multiple scales. GPUI’s public line layout is available, while its `TextSystem::rasterize_glyph` helper is crate-private. Compare a portable shaping/rasterization crate such as `cosmic-text` against available APIs and choose based on an actual render. Then add Text (`T`), an editable layer holding source text, font, pixel size and color, and cached raster output. Save source plus fallback pixels for machines with missing fonts. Define font substitution visibly and add explicit Rasterize Text Layer. Pixel tools offer rasterization or stay disabled on text layers.

**Accept** round-trip styled multiline text, export equivalent pixels, test a missing font and non-ASCII shaping, edit a reopened layer, and verify undo after explicit rasterization. Text input consumes typing and clipboard shortcuts while focused.

### P22 · Release hardening and Windows

**Depends on** all shipped slices. **Own** verification, fixes and the release handoff.

Repeat representative large-document benchmarks, check history and texture memory with multiple tabs, exercise interrupted saves and malformed projects, and compare shortcuts and native menu availability. Run `cargo check --locked --target x86_64-pc-windows-gnu` after resolving cache/toolchain dependencies. Prefer a native Windows build and smoke test when a runner is available. Report the distinction between cross-checking and running the app.

**Accept** publish a checklist covering every intended menu item and toolbox tool. Each entry is functional with verified availability or explicitly deferred in the release scope. Round-trip all shipped project versions, verify Finder association on the signed macOS bundle, and record performance results and platform coverage.

## Worktree scheduling

Run P01–P05 serially. P06 can run in an isolated worktree after P03 because it owns the benchmark and prototype. Merge P07 after P05 and P06. P08 can proceed once P05 lands if the selection worktree owns the mask and overlay, and one integrator resolves the canvas changes. After P08, clipboard/import (P10), selection algorithms (P12), and pure geometry prototypes can run in separate worktrees. P09 can land once P07 is also ready, and P11 follows P09. Land shared canvas, action and modal wiring in short sequential merges. P14 and P16 can run after P07 and P08 with distinct modules. Serialize every project schema update with `src/project.rs` and its fixtures under one owner.

Worktree assignments must specify the files each session owns and the commit to rebase onto. A dependency is ready when its interface and acceptance tests are merged, even if another feature can prototype against a fixture sooner.
