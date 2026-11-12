# Lime — Lite Image Editor

<img src="macos/AppIcon.svg" width="96" alt="Lime app icon">

Lime is a lite image editor for the desktop. Version 1.0.0 uses Rust and
[GPUI](https://gpui.rs/) with a GIMP-inspired workspace, native menus, and
layered raster editing.

<p align="center">
  <img src="lime-screenshot.jpg" alt="Lime editing a night skyline image with the toolbox, canvas, text options, and Paths panel visible" width="100%">
</p>

## Install

### macOS

Download `Lime-1.0.0.dmg`, open it, and drag **Lime** to **Applications**.
Lime is ad hoc signed but not notarized; if macOS blocks the first launch,
Control-click the app and choose **Open**. It needs a macOS release with Metal
support.

### Windows

Download `Lime-1.0.0-windows-x64.zip`, extract it, and run `Lime.exe`. This is a
portable program, not an installer. It needs 64-bit Windows 10 or later with
DirectX, plus the Microsoft Visual C++ Redistributable
2015–2022 x64 if the runtime DLLs are not already installed.

### Build from source

Install a recent Rust toolchain with Cargo and select an Xcode installation on
macOS. Clone [eimg/lime](https://github.com/eimg/lime), then run from its root:

```sh
./scripts/bundle-macos.sh
open dist/Lime.app

./scripts/package-macos.sh
open dist/Lime-1.0.0.dmg
```

`bundle-macos.sh` builds an optimized, ad hoc signed app with the Lime icon and
Finder file associations. `package-macos.sh` also creates the drag-to-Applications
DMG. Run `cargo run --locked` for a debug build.

## Editing and saving

### Documents

- **File → New Image** accepts pixel dimensions and creates a transparent
  canvas or fills it with the background swatch. The empty workspace offers the
  same action as a quiet button under Choose Files.
- **File → Open** accepts Lime projects, PNG, JPEG, and WebP. You can also drop
  files onto the canvas or use Finder's **Open With** menu. Reopening the same
  file focuses its existing tab.
- **Save** updates the current file. New canvases are saved as `.lime`
  projects. An opened PNG, JPEG, or WebP keeps saving to its own format while it
  is still a single plain layer without paths or editable text.
- **Save As** adopts the new filename and uses its extension to choose the
  format. Projects, new canvases, and layered documents use the project format,
  with `.lime` as the default extension. **Save Project As** suggests `.lime`
  when converting an opened image to a project. Legacy `.imageedit` projects
  still open and save; Save As suggests `.lime` for them.
- **File → Export as PNG, JPEG, or WebP** writes a flattened copy of the visible
  image to a separate file and preserves the tab's unsaved marker. Choose a
  destination that is closed in Lime. PNG and WebP keep transparency. JPEG asks
  for a quality from 1 to 100 (default 92) and places transparent areas on white.
- Projects preserve the canvas dimensions and layer stack: each layer's order,
  name, position, visibility, opacity, and lossless pixels with transparency.
  PNG and lossless WebP preserve transparency. JPEG saves against white. The
  working pixel format is 8-bit RGBA.
- Apply or discard an active transform, gradient, or crop preview before saving
  or exporting. Text being typed is committed first. Closing an image with a
  pending edit asks before discarding it.

### Canvas and view

- Drag a rectangle with **Crop** (`C`). Drag inside the box to move it, drag a
  handle to resize it, and edit X, Y, Width, and Height in the Crop options.
  Typed values apply immediately; an empty field leaves the crop unchanged.
  Press Return to apply the crop or Escape to cancel.
- **Canvas Size** grows or shrinks the canvas from one of nine anchors and keeps
  pixels that fall outside it.
- **Image → Mode** can make the document grayscale. Grayscale images save as
  projects; Export still writes a flattened copy. Switching back to RGB keeps
  the gray pixels and allows color again.
- Use the Zoom tool, View menu, or status bar to zoom. Control-scroll zooms
  around the pointer. Scroll or use a middle-button drag to pan a zoomed image.
  Hold Space and drag to pan without leaving the current tool, or use the Pan
  tool for left-button dragging. Space still types while editing text. Fitting
  or zooming out uses a reduced preview; zooming in shows source pixels.
- **View** toggles the grid, center guides, and the layer boundary. The boundary
  is a global subtle outline that follows the active layer.

### Selections

- **Rectangle Select** (`R`) drags a rectangle, with a corner-radius control
  that has minus and plus buttons. **Oval Select** (`O`) drags an oval; enable
  **Circle** in its options to constrain it to an exact circle.
- Drags may start or extend outside the canvas, while selected pixels stay
  clipped to the image. Each editable outline is a dashed box or ellipse with
  handles: drag a handle to resize it, drag inside to move it, and click outside
  it to deselect.
- The Width and Height fields show the current size while drawing, and typed
  values resize the selection immediately. Empty text leaves the selection
  unchanged.
- Cropping or shrinking the canvas clips curved selections without reshaping
  them; a partially clipped curve becomes a mask. Scaling a rounded selection
  scales its corners with the image, also using a mask.
- **Free Select** (`F`) works two ways: drag to add a freehand segment, or click
  to add a vertex. Releasing keeps the draft open as a checkpoint. Click the
  start marker or press Return to close it, and Escape cancels the draft.
- **Magic Wand** (`W`) selects a connected region. Its tolerance is shared with
  Bucket Fill: zero matches every channel exactly, and a higher value allows
  that much difference in the most changed channel. Hold Shift to add, Control
  to subtract, or both to intersect.
- **Select → Feather**, **Grow**, and **Shrink** change the current mask by one
  radius. **Select All**, **Select None**, and **Invert Selection** manage the
  whole canvas.
- Fill and **Edit → Clear to Transparency** change only selected pixels. With no
  selection, they change the whole image. **Image → Crop to Selection** uses the
  selection's bounding box and is disabled when nothing is selected or the whole
  image is selected.

### Layers

- **Move** (`M`) drags the active layer. Its crosshair has a four-arrow badge.
  The layer follows the pointer, and releasing it saves one undo step. Escape or
  Cancel puts it back. A hidden layer cannot be moved.
- The Move tool options and **Layer → Align** align the layer to any canvas edge
  or center it. **Layer to Image Size** keeps the pixels that already lie on the
  canvas and drops the rest. **Crop to Content**, on the Layer and Image menus,
  trims to pixels that are not fully transparent. A fully transparent layer or
  image is left unchanged.
- **Copy** and **Cut** put the selected part of the active layer on the
  clipboard, including transparent pixels. Cut then clears those pixels.
  **Paste** and **Paste as New Layer** add that image as a centered layer.
- **File → New from Clipboard** makes a canvas the size of the clipboard image.
  **Open as Layers** adds PNG, JPEG, and WebP files above the active layer. A
  file that cannot be read is reported, and the others are still added.
- The **Layers** panel lists layers top to bottom with thumbnails. A
  transparency checker shows through clear pixels there and on the canvas.
  Each thumbnail keeps the layer's proportions, and its checker fills only the
  layer's own area, not the square slot around it.
- Click a layer to select it, double-click to rename it, or use the eye button
  to hide it. Drag a row to reorder. The opacity slider and the Layer menu cover
  New, Duplicate, Rename, Merge Down, Delete, Raise, and Lower. New Layer can
  start transparent or filled with the foreground or background color. Merge
  Down stays disabled when either layer is hidden.

### Transforms and scale

- **Image → Transform** rotates or flips the whole image, and the selection
  moves with it. **Layer → Transform** does the same to the active layer and
  leaves the selection where it is.
- **Scale Image** and **Scale Layer** resize with nearest, bilinear, or bicubic
  sampling and an optional aspect lock.
- **Rotate** (`Shift+R`), **Flip** (`Shift+F`), and **Perspective** (`Shift+P`)
  preview the active layer. Drag to turn it, choose or drag a flip direction, or
  drag the perspective corners. Return applies the preview and Escape cancels
  it. Click the canvas to start another transform. The selection stays in place.
  Perspective requires a convex shape within the layer size limit.
- The **Scale Layer** tool (`Shift+S`) draws a frame around the active layer;
  drag a border or corner to scale it, and release commits one undo step. Its
  Width and Height options are read-only for now. Escape or changing layers or
  tabs cancels a scale drag.

### Painting and filling

- **Pencil**, **Brush**, **Eraser** (`E`), **Blur**, **Sharpen**, and
  **Clone Stamp** (`Shift+C`) draw on the active layer. Pencil starts at one
  pixel and uses a hard edge. Brush, Eraser, Blur, Sharpen, and Clone Stamp also
  offer hardness and opacity. Blur and Sharpen have no keyboard shortcut.
- Use `[` and `]` to decrease or increase the brush size for all paint tools.
  Alt-click with Clone Stamp to set its source, then paint to copy pixels from
  that offset. Holding Option (Alt) shows the source-picking cursor; releasing
  it restores the brush cursor without requiring a pointer move.
- Blur softens the pixels under the brush, and Sharpen raises local contrast.
  The pointer is a size-accurate ring that follows the brush diameter and zoom:
  solid for Pencil, with a soft inner ring for the other paint tools. Release
  commits the whole stroke and Escape discards it. Shift-click extends from the
  last committed stroke in the current tab when it was on the same layer.
- The eraser lowers alpha, and a selection limits where the stroke lands.
  The whole-layer **Filters → Blur…** and **Sharpen…** dialogs stay on the
  Filters menu.
- **Bucket Fill** (`B`) uses the foreground color on connected pixels within the
  shared tolerance, or covers the whole selection. Sample All Layers uses the
  colors you see to choose that region and paints the active layer.
- **Gradient** (`G`) drags a line across the active layer. It can be linear or
  radial, and it fades from the foreground color to the background or to
  transparent. Return applies the preview and Escape cancels it. A click with
  no drag fills with the foreground color.

### Paths

- **Path** (`P`) clicks anchors onto the canvas and drags to pull cubic
  handles. Click the first anchor, or press Return, to close a path. Escape
  cancels the drag and Backspace removes the selected anchor.
- The **Paths** tab next to Layers lists every path with its anchor count; click
  one to select it, use the eye to show or hide it, or use the trash button to
  delete it. Escape or switching tools stops the current path, and the next Path
  click starts a new one instead of extending the last node.
- **Select → Path to Selection** fills every visible closed path. **Selection to
  Path**, just below it in the Paths tab, creates closed paths from the current
  selection. Overlapping contours use nonzero winding, so a reversed contour
  cancels the area it shares. Open paths do not fill. Undo restores the previous
  anchors.
- Image transforms, scaling, canvas size, and crop move the paths with the
  image.
- **Stroke** in the Paths tab paints the selected visible path onto the active
  layer with the foreground color. Choose Pencil or Brush and adjust the stroke
  size there. **Edit → Stroke Path** and **Stroke Selection** remain available,
  and a repeated endpoint does not add a dab.

### Text

- **Text** (`T`) types straight onto the canvas with a visible caret. Click
  empty canvas to start new text, or click a text layer to edit it; a click
  inside or just beside the text moves the caret. A dashed frame marks the text
  being edited, and the caret blinks.
- Return starts a new line, the arrow keys move the caret, and the copy, cut,
  paste, and select-all shortcuts and native Edit menu actions edit the text
  instead of the image. Undo and Redo step through the typing first, then the
  image history.
- There is no Apply step: Escape, another tool, layer, or tab, a click outside
  the text, or any image command commits the text as one undo step. Commands
  resolve the active layer after that commit. If the text exceeds rendering or
  layer limits, the draft stays available on its tab and Save/Export stops so it
  can be corrected.
- The tool options choose the font from the installed families (or Sans Serif,
  Serif, Monospace), with an Aa sample in each font. Scrolling inside the font
  list keeps Tool Options still. Click the font box and type part of a name to
  filter it without regard to case. Use the arrow keys to browse, Return to
  choose, or Escape to close.
- Bold and Italic are independent toggles for the whole text layer. Choose the
  pixel size from 1 to 256, letter spacing in pixels, line spacing as a
  percentage of the size, and the text color. New text starts with the last
  style and the foreground color.
- A missing named font shows a substitution notice. Reopening a text layer keeps
  its saved pixels until its words or style change. Previews render in the
  background and follow the layer's opacity and stack position.
- The layer keeps the words and a raster copy, so export matches the canvas.
  **Layer → Rasterize Text Layer** keeps those pixels and lets the pixel tools
  edit them. Until then, painting, fills, filters, and transforms ask you to
  rasterize first.

### Colors and filters

- **Colors** opens Brightness & Contrast, Hue & Saturation, Desaturate, or
  Invert for the active layer. The dialog shows a reduced preview that follows
  the latest slider position. Return applies the full-resolution change and
  Escape cancels it. A selection blends the effect, and the layer's alpha stays
  in place. Leaving the sliders at zero does not change the image. Undo restores
  the previous pixels.
- **Color Balance** shifts shadows, midtones, or highlights and can preserve
  luminosity. **Levels** and **Curves** edit the value channel or a single color
  channel. Levels shows a histogram. Click the curve to add a point, drag a
  point to move it, and right-click an interior point to remove it. The
  endpoints stay at 0 and 255. A fully transparent selection shows an empty
  histogram.
- **Filters** opens Blur, Sharpen, or Drop Shadow for the active layer. The
  dialog shows a reduced preview. Return applies the change and Escape cancels
  it. Blur and Sharpen follow the selection. Drop Shadow adds a layer
  underneath, offset from the source and colored with the foreground color.
  Zero blur radius gives a sharp shadow. Blur and Sharpen use zero radius to
  preserve the source pixels. Undo restores the previous pixels, and undoing a
  shadow removes its layer.
- Click either color swatch for the hue and saturation wheel, brightness
  slider, and hex input. The last 12 distinct colors set or sampled in the
  workspace are available in Recent Colors. Click the swap icon between the
  swatches or press X to flip them. Press D to restore black foreground and
  white background.

### Undo and history

- **Edit → Undo** and **Redo** step through crops, fills, selections, and layer
  changes in the current tab. Undoing back to the last save clears the unsaved
  marker. Selection changes preserve the document's saved state.
- Each tab keeps up to 100 steps, and the oldest steps are dropped first when
  history uses too much memory.

## Workspace

The toolbox can be resized by dragging its right edge or folded to an icon rail
with the header toggle. An expanded tool shows its full name with the shortcut,
trimming the name when the sidebar is narrow, and a tooltip on hover. Folded
tools keep the icon and tooltip. The foreground and background swatches stack
vertically in the folded rail, with the swap control between them. Menus keep
the full names used above.

Tool Options keeps controls and editing values visible. Hover the question-mark
help icon beside the tool name for essential gestures, modifiers, and how to
apply or cancel an edit. The Paths panel has its own help icon. Warnings and
progress remain visible in the panel.

Click a numeric value in Tool Options to type it, including tolerance, brush
settings, path stroke size, and text spacing. Values update immediately within
their allowed range; Return accepts the edit, and Escape or an empty field
restores the previous value. Letter spacing accepts negative values. Scale
Layer's Width and Height displays remain read-only.

## Keyboard shortcuts

| Action | macOS shortcut |
| --- | --- |
| New / Open | ⌘N / ⌘O |
| Save / Save As / Save Project As | ⌘S / ⌘⇧S / ⌘⌥S |
| Export as PNG | ⌘⇧E |
| New Layer / Duplicate Layer | ⌘⇧N / ⌘⇧D |
| Undo / Redo | ⌘Z / ⌘⇧Z |
| Close Tab | ⌘W |
| Next / Previous Tab | Control-Tab / Control-Shift-Tab |
| Crop / Bucket / Color Picker | C / B / K |
| Rectangle Select / Oval Select / Free Select / Magic Wand / Move | R / O / F / W / M |
| Rotate / Flip / Perspective | Shift+R / Shift+F / Shift+P |
| Scale Layer / Clone Stamp | Shift+S / Shift+C |
| Eraser | E |
| Gradient / Path / Text | G / P / T |
| Pan | hold Space |
| Copy / Cut / Paste / Paste as New Layer | ⌘C / ⌘X / ⌘V / ⌘⇧V |
| Select All / Select None / Invert | ⌘A / ⌘⇧A / ⌘I |
| Foreground / Background color dialog | ⌥O / ⌥⇧O |
| Swap / Reset colors | X / D |
| Zoom tool / Zoom In / Zoom Out | Z / + / − |
| Actual pixels / Fit | 1 / ⌘⇧J |

## Windows status

Windows key bindings use Ctrl, including Ctrl+Q to quit and Ctrl+Y to redo. A
Windows-only adapter (`src/windows_menu.rs`) converts the same GPUI menu tree
into a native menu bar with `muda`, attaches it to the Win32 window, follows the
current enabled state of each action, and forwards activated items back through
the existing action handlers. It omits the macOS application menu and keeps
File → Exit. Native menu labels use Alt mnemonics, and accelerator text comes
from the GPUI key bindings; keyboard execution still goes through GPUI's
existing Ctrl shortcuts. Alt menu mnemonics and F10 go to native menu handling
before canvas input. Space inserts text even when Windows supplies no character
data. Menu availability updates after workspace changes and menu opening, without
idle frame polling. The Windows renderer uses an HWND DirectX swap chain so the
native menu bar has an opaque backing surface.

GitHub Actions runs formatting, Clippy, tests, and a native
`x86_64-pc-windows-msvc` release build, then uploads the portable
`Lime-1.0.0-windows-x64` ZIP. The menus and the artifact have not been
runtime-verified on Windows yet. A Windows release still needs code signing,
packaging work and `.lime`/Open With registration. The executable embeds the
original blue icon for Explorer, the title bar, and the taskbar. There is
no installer or file association yet.

## Development

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
```

Run the optional rendering and stroke benchmarks with
`cargo test --release --locked benchmark -- --ignored --nocapture`.
`cargo clean` clears generated Rust build files while keeping the current
`dist/Lime.app` and DMG available to run. The next build regenerates that cache.

See [AGENTS.md](AGENTS.md) for the architecture, current scope, and macOS
smoke checks.
