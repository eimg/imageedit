# ImageEdit implementation plan

Baseline assessed at commit `5c4cfcf` on `main`. This document specifies the proposed behavior and acceptance criteria for future work.

## Latest handoff · P10

P10 adds clipboard images and Open as Layers on branch `p10-clipboard`, which starts from P09's commit on `p09-geometry`.

- Copy (`⌘C`) writes the active layer, clipped to the selection, as a PNG clipboard image. Transparent pixels are kept. An unrestricted selection copies the whole layer, including pixels that sit off the canvas. An empty selection, or one that misses the layer, writes nothing. The write is accepted only when reading the clipboard back returns the same pixels. Cut (`⌘X`) does that first, then clears those pixels in one history entry. A hidden layer can be copied and cannot be cut. If the document changes while the PNG is encoding, Cut leaves the pixels in place.
- Paste (`⌘V`) and Paste as New Layer (`⌘⇧V`) each add one centered layer named Pasted Layer. There is no floating paste, so the two commands do the same thing. New from Clipboard makes an unsaved canvas the size of the clipboard image. Open as Layers places PNG, JPEG, and WebP files above the active layer in picker order, centered, in one history entry. A project or other file is reported and skipped; the rest of the batch is still added. A full stack or an image past the pixel budget is reported too.
- These commands are not registered while a dialog or form is open, so the color field keeps its own text copy, cut, and paste. The project format is unchanged.
- Formatting, Clippy and locked tests passed. P09 ended with 59 tests and P10 ends with 65, plus the ignored release benchmark. The tests cover a translucent pixel through PNG, an inverse hole, centered insertion, a full stack, a failed project import, and an undo that removes a pasted layer from a saved document.
- Copy into Preview or GIMP, and paste from another app, were not driven in the app.
- **Next is P11**, fixed transforms and sizing. It depends on the layer geometry from P09.

## Previous handoff · P09

P09 adds layer movement and geometry on branch `p09-geometry`, which starts from P08's commit on `p08-selection`.

- Move (`M`) drags the active layer. The canvas cursor is the four-arrow move glyph, centered on the pointer. The tool options repeat Center Horizontally, Center Vertically, Center in Image, Layer to Image Size, and the layer's Crop to Content; they disable while a drag or another edit is in progress. The offset is not written during the drag. The canvas paints a composite with that layer removed, plus the layer itself with its opacity baked into the preview, and the sprite may extend into the margin. Releasing commits one history entry when the offset changed. A press that moves fewer than 3 screen pixels does not. Escape, the Move tool's Cancel button, switching tools or tabs, and undo drop the preview and leave the stored offset alone. A hidden active layer shows a notice and does not start. If a save is in progress when the pointer is released, the preview is cancelled instead of committing.
- Layer → Align centers on the canvas. One axis keeps the other coordinate. The new coordinate is `(canvas − layer) / 2`, truncating toward zero, so a layer larger than the canvas gets a negative offset. Layer to Image Size copies the on-canvas pixels into a canvas-sized layer at the origin and drops the rest. An exact fit is left unchanged.
- Crop to Content uses pixels whose alpha is greater than zero. On a layer, the remaining pixels stay in the same canvas place. On the image, hidden layers, zero opacity, and pixels outside the canvas do not count, and the selection is rebased the way other canvas crops rebase it. Moving or aligning a layer does not move the selection. Fully transparent content keeps its dimensions and shows a notice. Content that is already tight is a silent no-op.
- A move dirties the union of the old and new layer bounds. The project format is unchanged; layer offsets already round-trip. Scale Layer stays planned for P11.
- Formatting, Clippy and locked tests passed. P08 ended with 51 tests and P09 ends with 59, plus the ignored release benchmark. The tests cover centering, partial alpha, on-canvas fitting, image crops that ignore hidden and off-canvas pixels, the dirty union, and an undo that restores an offset and leaves a saved document clean.
- The drag, Cancel, and menu commands were not driven in the app.
- **Next is P10**, clipboard and Open as Layers, or P11 once this geometry is the base it needs. P12 can proceed from P08 without this pass.

## Previous handoff · P08

P08 adds canvas-space selection on branch `p08-selection`, which starts from P07's commit on `p07-preview`.

- `src/selection.rs` keeps unrestricted, empty, and full selections allocation-free, plus a rectangle and the inverse of a rectangle. Coverage is tested in canvas coordinates, including a layer whose pixels are offset from the canvas. Crop bounds are absent for unrestricted, empty, and a selection that already covers the canvas. Cropping rebases a rectangle and drops it when the crop misses it.
- Rectangle Select (`R`, and the Select toolbox tool) commits one history entry on release. The contour is a black-and-white dash in screen space and follows zoom and pan. While the Select tool is active, a rectangle or an inverse hole shows handles: dragging a handle resizes that edge, dragging inside moves the box without letting it leave the canvas, and a click that misses the box returns to unrestricted. Over a handle or the inside of the box the painted crosshair gives way to the system resize or hand cursor. Select All (`⌘A`), Select None (`⌘⇧A`), and Invert (`⌘I`) use Ctrl on Windows. Deselect and Invert are also buttons in the Select tool options. Select None returns to unrestricted, so later edits are not masked. Invert of unrestricted is empty, and invert of empty is the full canvas.
- A drag replaces the selection. Shift to add, Control to subtract, and Shift-Control to intersect need a canvas-bounded mask, because this selection stores one rectangle or its inverse. P12 already owns those modifiers. Do not emulate them by storing only the latest rectangle.
- Bucket Fill stops at the selection edge. Clear to Transparency writes transparent pixels only where the selection covers the active layer, and a pixel that is already clear does not become a change. A hidden active layer is refused with a status notice, as fill already was. Crop to Selection uses the bounding box and rebases the selection; the manual crop tool rebases it too.
- Selection changes share the tab history. A selection-only undo returns a saved document to a clean state without changing pixels. The project file still does not store the selection.
- Formatting, Clippy and locked tests passed. P07 ended with 38 tests and P08 ends with 51, plus the ignored release benchmark.
- The rectangle drag, contour, and menu commands were not driven in the app. Computer Use can screenshot ImageEdit, but the window stayed covered and clicks did not reach it.
- **Next is P09**, layer movement and geometry. It can use this selection's canvas coordinates.

## Previous handoff · P07

P07 paints the 256 px tiles from P06 on branch `p07-preview`, which starts from P06's commit on `p06-preview`.

- The canvas paints cached tiles instead of the single 4096 px preview. Each tile is drawn into its stored rectangle and clipped to its content rectangle, so the one-pixel apron is what the linear sampler reads and neighboring tiles do not cover the same pixels. Tiles outside the viewport are skipped. At 8 screen pixels per source pixel or more, the canvas still paints source pixels. If the preferred reduction is missing, the full-resolution tiles are shown until it arrives.
- Fit and zoom choose a shift with `fit_level_shift`, using the image's on-screen edge in physical pixels. Level 0 is built with the document. Other shifts are built one at a time on a background job. The job installs only while the composite `Arc` is still the one it started from, so a rename does not cancel it and a painted edit does. A painted edit also drops a level that arrived after that edit's patch was prepared.
- `RenderedCanvas::prepare_patch` runs on the edit job. `Dirty::Region` (fill, opacity, visibility) rebuilds tiles that meet the dirty rectangle, including the apron, and patches the full composite used by the eyedropper and the high-zoom path. `Dirty::Canvas` (crop, undo, redo, and structural layer edits) replaces the levels with a new base cache. `Dirty::Unpainted` (rename) keeps the composite and the tile images. `apply` returns the GPU images it replaced, and the workspace drops those. Closing a tab still releases every image the canvas owns, including tiles.
- Opacity drags and the crop overlay still commit one edit on release, so a gesture does not rebuild tiles on each pointer move. Preview work does not change the saved revision.
- A 600 × 300 document test checks the accept path: a 64 × 64 region at `(300, 20)` replaces one base-level GPU image, a rect crossing `x = 256` replaces two, and a rename replaces none. The P06 CPU benchmark was not re-timed; shift 0 still composites the dirty rectangle and the ignored `preview_benchmark` harness is unchanged.
- Formatting, Clippy and locked tests passed. P06 ended with 37 tests and P07 ends with 38, plus the ignored release benchmark.
- Computer Use could screenshot the app. An 800×600 gradient fitted in the window showed a continuous grid across the 256 px boundaries. The window stayed covered, so pan, zoom, tab switch, and close were not driven.
- **Next is P08**, selection basics. P09 waits on both P07 and P08.

## Previous handoff · P06

P06 measured a tiled preview on branch `p06-preview` and left the canvas renderer unchanged. `main` was fast-forwarded through P05 before this branch.

- **Choice: 256 px tiles.** A 64 × 64 interior edit rebuilds one tile. The same edit on a 512 px grid rebuilds a tile four times larger and costs about four times as much. A full refresh costs the same either way, because both walk every canvas pixel. 256 px also matches the invalidation the next pass wants. A live overlay is not needed.
- Release benchmark on an 8192 × 8192 RGBA canvas, 2 warmup runs and 20 timed interior samples (`cargo test --release --locked preview_benchmark -- --ignored --nocapture`). Machine: macOS 27.3 (26C5035r), Apple M3 Pro, 11 cores, 18 GB, built-in Liquid Retina XDR at 3024 × 1964 (scale 2). 256 px: interior p95 **85 µs** (1 tile), boundary p95 **373 µs** (4 tiles), full refresh **113 ms** (1024 tiles). 512 px: interior p95 **323 µs** (1 tile), boundary p95 **1534 µs** (4 tiles), full refresh **113 ms** (256 tiles). The current full-preview triangle resize to 4096 took **640 ms**. The 8 ms interior target is met.
- `src/preview.rs` keeps a one-pixel apron because GPUI's Metal shaders sample atlas textures with a linear filter (`mag_filter::linear` in `gpui-0.2.2` `shaders.metal`). The apron test checks that a 50% blend across a tile edge matches the composite. Tiles without an apron stop at the edge. Unchanged tiles keep the same `Arc`, which is the stand-in for texture reuse; a future painter should call `drop_image` only on the replaced images. `fit_level_shift(8192, 1600)` is 2, so Fit uses a 2048 px source. Visible-tile counts are tested for a pan that crosses a tile edge.
- `DocumentContent::composite_rect` composites one canvas rectangle for those tiles. Nothing in the window calls it yet.
- Formatting, Clippy and locked tests passed. P05 ended with 32 tests and P06 ends with 37, plus the ignored release benchmark.
- The native zoom and pan seam was not checked in the app, because this pass does not paint tiles. The seam check above is the CPU equivalent of the linear sample.
- **Next is P07.** It should paint 256 px tiles, drop replaced images, and keep the Fit level from this pass.

## Earlier handoff · P05

P05 added the Layers panel and layer commands on branch `p05-layers`, which starts from P04's branch `p04-export`.

- `src/document/content.rs` adds New, Duplicate, Rename, Delete, Raise/Lower (`move_layer`), visibility, opacity and Merge Down. New layers are canvas-sized, visible, Normal and full opacity. Duplicate shares pixel buffers until edited and appends ` copy`, truncated to `MAX_LAYER_NAME_CHARS` (64). Delete refuses the last layer and selects the neighbor below, or else the one above. Merge Down requires a visible lower neighbor, composites both into their checked union with `composite::flatten_region`, keeps the lower ID and name, and becomes a visible Normal layer at opacity 255. `MAX_LAYERS` (128) is shared with the project codec. Opening a document selects `preferred_active_layer`, the topmost visible layer.
- `src/workspace/layers.rs` owns the panel (top-to-bottom rows, thumbnails, visibility, drag reorder, opacity slider and command buttons) and menu enablement. An opacity drag updates a temporary value and commits one history entry on release. Layer actions register only while enabled, so the native menu disables itself. Bucket Fill now refuses a hidden active layer and shows a status notice instead of editing invisible pixels.
- `src/workspace/dialogs.rs` adds New Layer (name plus Transparent/Foreground/Background fill), Rename Layer and Layer Opacity (0–100%). Field editing uses character indices so non-ASCII names work. `RenderedCanvas` builds a 64 px thumbnail per layer and `drop_canvas` releases every GPUI image a canvas owns.
- `src/commands.rs` enables the Layer menu items and binds New Layer to `⌘⇧N` and Duplicate to `⌘⇧D`.
- Formatting, Clippy and locked tests passed. P04 ended with 29 tests and P05 ends with 32. New tests cover layer stack mutations and caps, Merge Down composite preservation within 1 level on an alpha fixture, and opacity percent round-trips.
- No native UI smoke test ran. Computer Use still lacked Accessibility and Screen Recording permission.

## Earlier handoff · P04

P04 added raster export and Save routing on branch `p04-export`, which starts from P03's branch `p03-history`.

- `src/document.rs` adds `export_image` with an explicit JPEG quality and `DEFAULT_JPEG_QUALITY` (92), used by raster Save. `save_image` and `export_image` share `write_atomically`, the existing same-directory temporary write and replacement. `Document::can_save_as_raster` requires a non-project format and `DocumentContent::is_plain_raster`: one visible Normal layer at `(0, 0)` with opacity 255, canvas-sized and named `IMPORTED_LAYER_NAME` ("Image").
- `src/workspace/files.rs` routes Save through the pure `save_route`. Save overwrites a project-backed tab or an eligible raster tab. Otherwise it prompts for an `.imageedit` destination. Save As offers raster formats only to eligible raster tabs. Untitled documents are projects and use Export for raster copies. `prepare_destination` rejects a raster extension where a project is required before anything is written, so the source file, tab path and saved revision stay intact.
- Export as PNG (`⌘⇧E`), JPEG and WebP are in the File menu. JPEG first shows a quality form (1–100), remembered for the session. `prepare_export` requires the extension of the chosen format (`.jpg` or `.jpeg` for JPEG). `write_file` handles Save and Export, rejects a destination owned by another tab or an in-flight write, and on export success leaves path, format, name and saved revision unchanged. The status bar shows "Exporting…" and then "Exported name" until the next edit or write. The macOS save panel asks before replacing an existing file.
- Formatting, Clippy and locked tests passed before and after the change. P03 ended with 25 tests and P04 ends with 29. The new tests cover the routing table, destination and export extension checks (including a missing folder), export of the v2 fixture with transparent and translucent pixels in all three formats (PNG and WebP pixel-exact, JPEG opaque and within 6 levels of the composite over white at quality 100), an unchanged project file, a rejected project-format export, a failed write leaving no file or temporary file, and raster-Save eligibility after fill, crop, opacity and rename changes.
- No native UI smoke test ran. Computer Use still lacked Accessibility and Screen Recording permission.
- The P02 gap where Save As flattened a layered tab is fixed by routing. The hidden-active-layer gap remains for P05.

## Earlier handoff · P03

P03 added per-tab undo/redo history and save identity on branch `p03-history`, which starts from P02's branch `p02-project-v2`.

- `src/workspace/history.rs` is pure and unit-tested. `History` keeps undo entries oldest-first and redo entries with the next target last. Each entry stores a `State` (content snapshot and active layer), its revision and a global stamp. Snapshots are `DocumentContent` clones that share layer pixel `Arc`s. `retained_bytes` charges only the buffers history keeps alive beyond the current content, counting each shared buffer once. `LIMITS` holds 100 entries per tab, 256 MiB per tab and 512 MiB app-wide. Eviction removes the entry with the oldest stamp, so redo entries are evictable too. `enforce_app_budget` evicts the globally oldest entry across tabs. An entry that alone exceeds a budget is evicted at once, leaving the current state intact.
- Revisions identify persistent content. They are allocated from a per-tab counter and never reused, so a discarded redo branch cannot collide with a later edit. A tab is dirty when its current revision differs from the saved revision. Untitled documents start with no saved revision. `write_image` captures the revision when the save starts and records it only on success. The edit generation advances on commit, undo and redo.
- `src/workspace.rs` replaced `edit_content`'s private job with `start_change`/`finish_change` for `Change::Edit`, `Undo` and `Redo`. Every change renders the target in the background, and `finish_change` publishes only if the tab still exists and its generation matches the one captured at start. A stale result's preview is dropped. A no-op edit returns `None` and creates no entry. Pan resets when the canvas size changes, which replaces the old `reset_view` flag. The tab-level busy guard still serializes edits, history steps and saves.
- `src/commands.rs` adds `Undo` and `Redo` in the Edit menu, bound to `⌘Z`/`⌘⇧Z` on macOS and `Ctrl+Z`/`Ctrl+Y` elsewhere. `src/workspace/view.rs` registers each handler only when no form or dialog is open, the tab is idle and a target exists, so the menu items disable themselves otherwise. Undo and Redo clear an in-progress crop rectangle. Menu labels do not name the step yet.
- Formatting, Clippy and locked tests passed before and after the change. P02 ended with 18 tests and P03 ends with 25. The new tests cover undo and redo of a crop then a fill back to a clean saved revision, saving after undo and branching by editing, a no-op edit, the entry cap with saved identity kept after eviction, shared-buffer byte accounting with an over-budget entry, app-wide eviction across tabs, and generation change on undo. The workspace's generation comparison itself has no GPUI test harness.
- The release bundle built and passed `codesign --verify --strict` and launched with the v2 fixture. Computer Use still lacked Accessibility and Screen Recording permission, so Undo/Redo through the menu and shortcuts, the disabled menu state, and the unsaved marker were not exercised in the native UI.

## Earlier handoff · P02

P02 replaced the project format with the v2 layer-stack format on branch `p02-project-v2`. ImageEdit has no released users, so v1 support was dropped by product decision: v1 files and any other version are rejected. No menu, shortcut or tab behavior changed.

- `fixtures/projects/v2-two-layers.imageedit` is a P02-written stack with non-sequential IDs, a non-ASCII name, a negative offset, a hidden layer, opacity 77 and translucent pixels. Tests read it to catch accidental format drift. Regenerate it only when the format intentionally changes.
- `src/project.rs` reads and writes only v2, as specified under *Project v2 wire format*, and rejects any other `version`. The manifest and layer entries reject unknown fields. One `check_limits` function enforces the canvas, layer count, per-layer and total decoded size caps, and unique IDs for both reads and writes before any pixel buffer is allocated. The reader rejects a payload length that is zero or exceeds the remaining file, enforces the 768 MiB encoded total, checks PNG dimensions against the manifest, and rejects trailing bytes. The writer encodes one layer at a time into a reused buffer.
- `src/document/content.rs` adds `DocumentContent::from_layers`, which requires a non-empty canvas, at least one layer, non-empty layer pixels and unique IDs. It also adds the read-only `size()` and `layers()` accessors. `src/document.rs` exports `BlendMode`, `RasterLayer` and `MAX_IMAGE_BYTES`, and its save adapter writes every project through `project::write`. The v1 full-canvas restriction is gone. Atomic same-directory replacement and recent-document handling are unchanged.
- Formatting, Clippy and locked tests passed before and after the change. The baseline had 12 tests and the final run had 18. New tests cover v2 round-trips against the writer and the golden file, 17 malformed manifests, 12 malformed payload or framing cases, writer limits, a failed save that leaves the original file reopenable with no leftover temporary file, a truncated project failing `Document::open`, and the constructor's validation. A malformed file fails in `Document::open` before a tab exists, so no tab can be mutated.
- The release bundle built and passed `codesign --verify --strict`. It launched with the fixtures and a truncated project from a temporary directory. The UI smoke test did not run because Computer Use lacked Accessibility and Screen Recording permission. The opened tabs, the error for the truncated file, and a Save and reopen through the UI remain unverified.
- Known gap for P04 and P05: opening a multi-layer v2 file makes the top layer active even if it is hidden, so Bucket Fill can edit an invisible layer. Save As to PNG, JPEG or WebP flattens a layered tab and switches it to that raster format. P04 owns that routing.

## Earlier handoff · P01

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

- v1 is not supported. ImageEdit had no released users when v2 replaced it, so a v1 file fails to open like any other unsupported version.
- Start with the existing 64 KiB manifest limit and 256 MiB decoded canvas limit. Limit v2 to 128 layers, 512 MiB total decoded layer pixels and 768 MiB total encoded payload. Apply the same caps to reading and writing. Check manifest lengths and aggregate dimensions before allocating pixel buffers.
- Reject unknown versions, invalid metadata, duplicate IDs, unsupported blend modes, truncated or oversized chunks, extra payloads and PNG dimensions that disagree with their layer. Bound reads to each payload. Encode one layer at a time and use the existing same-directory temporary-file replacement on save.
- Reserve v3 for image mode in P11, v4 for paths in P19, and v5 for editable text in P21. Treat v2 as RGB. Choose each new payload layout in its owning pass. An older reader must reject a project it cannot fully represent instead of opening a reduced version that can later overwrite the source. Until the first release, a format change may replace the previous version and its fixture outright. After release, keep readers and fixtures for every shipped version. Omit transient previews, selections and history from saved projects.

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

Implement the v2 wire format and bounded readers. Read and write v2 only. Generate a two-layer fixture in tests before the UI creates layers. Keep atomic replacement and recent-document behavior.

**Accept** v2 round-trips IDs/order/offset/alpha/name/opacity/visibility, malformed variants fail without mutating a tab, and a failed replacement leaves a valid original file.

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

Add Free Select (`F`) with freehand and click-to-close polygon modes. Add Magic Wand (`U`) and a shared tolerance control for connected bucket fill. Preserve exact RGBA matching at threshold zero. Use four-neighbor connectivity and compare the maximum absolute difference among RGBA channels to a tolerance from 0 to 255. P08's rectangle replaces the selection. Use Replace by default, Shift for Add, Control for Subtract and Shift+Control for Intersect when starting a selection gesture, once the selection can hold a combined mask. Add Feather, Grow and Shrink dialogs using bounded mask operations.

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
