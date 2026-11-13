# Lime

**Lite Image Editor** for macOS and Windows. Layered editing, native menus with
tool and command icons, and a workspace that keeps the canvas in focus.

![Lime editing a night skyline, with its toolbox, text controls, and Paths panel](lime-screenshot.jpg)

## Features

- **Layers** — move, align, reorder, merge, and adjust opacity.
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

## Download

**Lime 1.0.1** · [Release notes](https://github.com/eimg/lime/releases/tag/v1.0.1)

| Platform | Download | Install |
| --- | --- | --- |
| macOS (Apple silicon) | [DMG](https://github.com/eimg/lime/releases/download/v1.0.1/Lime-1.0.1.dmg) | Open the disk image and drag Lime to Applications. |
| Windows x64 | [EXE](https://github.com/eimg/lime/releases/download/v1.0.1/Lime-1.0.1-windows-x64.exe) | Download and run. No installer. |

Windows needs Windows 10 or later, DirectX, and the Microsoft Visual C++
2015–2022 x64 runtime if it is not already installed. Windows builds are unsigned;
macOS builds are ad hoc signed and not notarized.

## Getting started

Open a PNG, JPEG, WebP, or PDF, drop one into the workspace, or create a new image.
PDF files open an import dialog: choose all pages or a range, set the resolution
in DPI, and add the pages as layers or as separate images. A PDF import becomes a
new unsaved Lime project; PDF is not a save or export format. Encrypted PDFs are
not supported. Open as Layers accepts one PDF at a time. Multiple PDFs opened
together keep their import dialogs queued. Pages are rasterized at the chosen
DPI; PDF text and vector objects do not become editable text or paths.
Pick a tool on the left and adjust its settings on the right. Hover the **?**
beside a tool name for gestures and shortcuts.

Save as **`.lime`** to keep layers, editable text, and paths. Use **Export** for
a flattened PNG, JPEG, or WebP copy. Apply or cancel an active preview before saving.
Finish or cancel an unfinished free selection first. If an image gains layers
while the save dialog is open, save it as a `.lime` project or export a copy.
Smooth scaling keeps the colors of nearly transparent pixels intact; show a
hidden layer before dragging its Scale Layer frame.
Reduced previews and layer thumbnails also preserve transparent edge colors.
Save and Undo shortcuts finish an active numeric selection edit first.

With **Scale Layer** (Shift+S), type Width or Height to preview a centered resize.
Return or clicking outside commits one undo step; Escape or empty text restores
the original size. Aspect locking starts on; Shift temporarily reverses the lock
while dragging. Choose Nearest, Bilinear, or Bicubic for the committed result.
In **Rotate**, enter an angle in degrees (including negative values and tenths),
then use Apply after finishing the numeric edit. **Sample All Layers** on the
Magic Wand samples the visible composite and can select beyond the active layer.

**Rectangle Select** and **Oval Select** accept X/Y and Width/Height for an
editable geometric selection. Return or leaving the field commits one selection
undo step; Escape or empty text restores it. Selection changes keep the image's
saved state. **Antialias edges** optionally smooths oval/circle and rounded
rectangle edges; it starts off for hard pixel selections. Toggling it also
updates the current editable curve. Combined, clipped, feathered, and wand masks
keep their existing coverage when this setting changes.

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

**Clone Stamp** starts with Aligned enabled, preserving the source offset across
strokes. Turn it off to start each stroke at the chosen source. Alt-click to set
a source again after changing tabs/layers or moving/resizing the layer. Wand and
connected Fill treat fully transparent pixels as the same color even when their
stored RGB differs; other alpha and tolerance comparisons stay in effect.

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

Use **View → Toggle Grid** for a dotted grid anchored to image pixels, with
contrasting dots and a default spacing of 16 image pixels.
**View → Toggle Rulers and Guides** shows pixel rulers. With **Move**, drag from
the top ruler for a horizontal guide or from the left ruler for a vertical one.
Drag an existing guide to reposition it; drag back onto a ruler or outside the
canvas workspace to remove it. Escape cancels the drag. Guide edits undo/redo,
and guides follow image crop, resize, scale, rotate, and flip.

Move's **Snap to Grid** and **Snap to Guides** controls work independently of
overlay visibility. They snap layer edges and centers within a small screen
distance. Hold Control while dragging to bypass snapping and guide grabs.
Typed positions and arrow nudges remain exact. Grid spacing is in image pixels;
dots thin out at low zoom while snapping keeps the chosen spacing.

Guides are stored in **`.lime`** projects, never flattened exports. A project
containing guides uses format version 7 and needs this build or newer. Existing
version 6 projects still open; projects without guides retain version 6 on save.

In **Text**, drag over characters, Shift-click, or use Shift+arrows to select a
range. Copy/Cut use that range; typing and Paste replace it. Selection highlights
follow individual lines. Option+Left/Right and Option+Backspace/Delete on macOS
(Ctrl on Windows) move or delete by word; Shift extends word selections. Normal
arrows and deletion keep combining characters and emoji sequences together.
Font/style controls still affect the entire text layer. Undo/Redo restore typing
and its selection before falling through to document history.

| Action | macOS | Windows |
| --- | --- | --- |
| Open / Save | ⌘O / ⌘S | Ctrl+O / Ctrl+S |
| Undo / Redo | ⌘Z / ⌘⇧Z | Ctrl+Z / Ctrl+Y |
| Copy / Cut / Paste | ⌘C / ⌘X / ⌘V | Ctrl+C / Ctrl+X / Ctrl+V |
| Crop / Move / Text | C / M / T | C / M / T |
| Pan | Hold Space and drag | Hold Space and drag |
| Brush size | [ / ] | [ / ] |
