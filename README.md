# ImageEdit

<img src="macos/AppIcon.svg" width="96" alt="ImageEdit app icon">

ImageEdit is a lightweight desktop image editor in development. It uses Rust
and [GPUI](https://gpui.rs/) with a GIMP-inspired workspace and native macOS
menus. The current milestone supports layered image editing.

## Try it on macOS

Install a recent Rust toolchain with Cargo and select an Xcode installation.
Clone the repository, then run these commands from its root.

```sh
./scripts/bundle-macos.sh
open dist/ImageEdit.app
```

The script builds an optimized, ad hoc signed app with the ImageEdit icon and
Finder file associations. Use the bundle to check image loading performance
and Finder integration. Run `cargo run --locked` for a debug build.

## Editing and saving

- **File → New Image** accepts pixel dimensions and creates a transparent
  canvas or fills it with the background swatch.
- **File → Open** accepts ImageEdit projects, PNG, JPEG, and WebP. You can
  also drop files onto the canvas or use Finder’s **Open With** menu.
  Reopening the same file focuses its existing tab.
- **Save** updates the current file. New canvases are saved as `.imageedit`
  projects. An opened PNG, JPEG, or WebP keeps saving to its own format while
  it is still a single plain layer. **Save As** adopts the new filename and
  uses its extension to choose the format. Projects, new canvases, and layered
  documents can only be saved as `.imageedit`. **Save Project As** suggests
  `.imageedit` when converting an opened image to a project.
- **File → Export as PNG, JPEG, or WebP** writes a flattened copy of the
  visible image to a separate file and preserves the tab's unsaved marker.
  Choose a destination that is closed in ImageEdit. PNG and WebP keep
  transparency. JPEG asks for a quality from 1 to
  100 (default 92) and places transparent areas on white.
- Projects preserve the canvas dimensions and the layer stack: each layer's
  order, name, position, visibility, opacity, and lossless pixels with
  transparency. PNG and lossless WebP preserve transparency. JPEG saves
  against white. The working pixel format is 8-bit RGBA.
- Drag a rectangle with **Crop** and press Return to apply it or Escape to
  cancel. **Bucket Fill** uses the foreground color on connected pixels with
  matching RGBA values. **Color Picker** samples the image at full resolution.
  **Select** (`R`) drags a rectangle. The outline is a dashed box with handles:
  drag a handle to resize it, drag inside to move it, and click outside it to
  deselect. The dashes move along the outline. The pointer becomes a resize
  arrow on a handle and a hand inside the box. Select All, Deselect, and Invert
  are in the Select menu and in the Select tool options.
  Fill and **Edit → Clear to Transparency**
  change only selected pixels. With no selection, they change the whole image.
  **Image → Crop to Selection** uses the selection's bounding box and is
  disabled when nothing is selected or the whole image is selected.
  **Move** (`M`) drags the active layer. Its crosshair has a four-arrow badge.
  The layer follows the pointer, and releasing it saves one undo step. Escape or
  Cancel puts it back. A hidden layer cannot be moved. The Move tool options
  also center the layer, fit it to the canvas, and crop it to its opaque
  pixels. **Layer → Align** centers the active layer on the
  canvas, horizontally, vertically, or both. **Layer to Image Size** keeps
  the pixels that already lie on the canvas and drops the rest. **Crop to
  Content**, on the Layer and Image menus, trims to pixels that are not fully
  transparent. A fully transparent layer or image is left unchanged.
  **Copy** and **Cut** put the selected part of the active layer on the
  clipboard, including transparent pixels. Cut then clears those pixels.
  **Paste** and **Paste as New Layer** add that image as a centered layer.
  **File → New from Clipboard** makes a canvas the size of the clipboard
  image. **Open as Layers** adds PNG, JPEG, and WebP files above the active
  layer. A file that cannot be read is reported, and the others are still
  added.
  Click either color swatch for the hue and saturation wheel, brightness
  slider, and hex input. The last 12 distinct colors set or sampled in the
  workspace are available in Recent Colors. Click the swap icon between the
  swatches or press X to flip them. Press D to restore black foreground and
  white background.
- Use the Zoom tool, View menu, or status bar to zoom. Control-scroll zooms
  around the pointer. Scroll or use a middle-button drag to pan a zoomed
  image. Pan is also available from the toolbox for left-button dragging.
  Fitting or zooming out uses a reduced preview; zooming in shows source
  pixels. The cursor shows the selected tool beside the canvas crosshair.
- **Edit → Undo** and **Redo** step through crops, fills, selections, and layer changes in
  the current tab. Undoing back to the last save clears the unsaved marker.
  Selection changes preserve the document’s saved state.
  Each tab keeps up to 100 steps, and the oldest steps are dropped first when
  history uses too much memory.
- The **Layers** panel lists layers top to bottom with thumbnails. Click a
  layer to select it, double-click to rename it, or use the eye button to hide
  it. Drag a row to reorder. The opacity slider and the Layer menu cover New,
  Duplicate, Rename, Merge Down, Delete, Raise and Lower. New Layer can start
  transparent or filled with the foreground or background color. Merge Down
  stays disabled when either layer is hidden.

| Action | macOS shortcut |
| --- | --- |
| New / Open | ⌘N / ⌘O |
| Save / Save As / Save Project As | ⌘S / ⌘⇧S / ⌘⌥S |
| Export as PNG | ⌘⇧E |
| New Layer / Duplicate Layer | ⌘⇧N / ⌘⇧D |
| Undo / Redo | ⌘Z / ⌘⇧Z |
| Close Tab | ⌘W |
| Next / Previous Tab | Control-Tab / Control-Shift-Tab |
| Crop / Bucket Fill / Color Picker | Shift+C / Shift+B / O |
| Rectangle Select / Move | R / M |
| Copy / Cut / Paste / Paste as New Layer | ⌘C / ⌘X / ⌘V / ⌘⇧V |
| Select All / Select None / Invert | ⌘A / ⌘⇧A / ⌘I |
| Foreground / Background color dialog | ⌥O / ⌥⇧O |
| Swap / Reset colors | X / D |
| Zoom tool / Zoom In / Zoom Out | Z / + / − |
| Actual pixels / Fit | 1 / ⌘⇧J |

Menus and the toolbox still show planned features such as text, color
adjustments, and filters. Commands without handlers appear
disabled. Windows is a future target.

## Development

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
```

See [AGENTS.md](AGENTS.md) for the architecture, current scope, and macOS
smoke checks.
