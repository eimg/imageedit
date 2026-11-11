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
  cancel. **Bucket Fill** uses the foreground color on connected pixels within
  the shared tolerance. **Color Picker** samples the image at full resolution.
  **Select** (`R`) drags a rectangle. The outline is a dashed box with handles:
  drag a handle to resize it, drag inside to move it, and click outside it to
  deselect. The dashes move along the outline. The pointer becomes a resize
  arrow on a handle and a hand inside the box. Select All, Deselect, and Invert
  are in the Select menu and in the Select tool options.
  **Free Select** (`F`) has two modes. In Freehand, drag and release to close
  the outline. In Polygon, click to place vertices, then click the start marker
  or press Return to close it. Escape cancels either draft. **Magic Wand**
  (`U`) selects a connected region. Its tolerance is shared with Bucket Fill:
  zero matches every channel exactly, and a higher value allows that much
  difference in the most changed channel. Hold Shift to add, Control to
  subtract, or both to intersect. **Select → Feather**, **Grow**, and
  **Shrink** change the current mask by one radius.
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
  **Image → Transform** rotates or flips the whole image, and the selection
  moves with it. **Layer → Transform** does the same to the active layer and
  leaves the selection where it is. **Scale Image** and **Scale Layer** resize
  with nearest, bilinear, or bicubic sampling and an optional aspect lock.
  **Rotate** (`Shift+R`), **Flip** (`Shift+F`), and **Perspective** (`Shift+P`)
  preview the active layer. Drag to turn it, choose or drag a flip direction,
  or drag the perspective corners. Return applies the preview and Escape
  cancels it. Click the canvas to start another transform. The selection stays
  in place. Perspective requires a convex shape within the layer size limit.
  **Pencil** (`N`), **Brush** (`P`), and **Eraser** (`Shift+E`) draw on the
  active layer. Pencil starts at one pixel and uses a hard edge. Brush and
  Eraser also offer a hardness control. Release commits the whole stroke and
  Escape discards it. Shift-click extends from the last committed stroke in
  the current tab when it was on the same layer. The eraser lowers alpha, and
  a selection limits where the stroke lands.
  **Gradient** (`G`) drags a line across the active layer. It can be linear or
  radial, and it fades from the foreground color to the background or to
  transparent. Return applies the preview and Escape cancels it. A click with
  no drag fills with the foreground color.
  **Bucket Fill** fills either the connected region or the whole selection.
  Sample All Layers uses the colors you see to choose that region and paints
  the active layer.
  **Colors** opens Brightness & Contrast, Hue & Saturation, Desaturate, or
  Invert for the active layer. The dialog shows a reduced preview that follows
  the latest slider position. Return applies the full-resolution change and
  Escape cancels it. A selection blends the effect, and the layer's alpha stays
  in place. Leaving the sliders at zero does not change the image. Undo
  restores the previous pixels.
  **Color Balance** shifts shadows, midtones, or highlights and can preserve
  luminosity. **Levels** and **Curves** edit the value channel or a single
  color channel. Levels shows a histogram. Click the curve to add a point, drag
  a point to move it, and right-click an interior point to remove it. The
  endpoints stay at 0 and 255. A fully transparent selection shows an empty
  histogram.
  **Filters** opens Blur, Sharpen, or Drop Shadow for the active layer. The
  dialog shows a reduced preview. Return applies the change and Escape cancels
  it. Blur and Sharpen follow the selection. Drop Shadow adds a layer
  underneath, offset from the source and colored with the foreground color. A
  radius or amount of zero does not change the image. Undo restores the
  previous pixels, and undoing a shadow removes its layer.
  **Canvas Size** grows or shrinks the canvas from one of nine anchors and
  keeps pixels that fall outside it. **Image → Mode** can make the document
  grayscale. Grayscale images save as projects; Export still writes a flattened
  copy. Switching back to RGB keeps the gray pixels and allows color again.
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
| Rectangle Select / Free Select / Magic Wand / Move | R / F / U / M |
| Rotate / Flip / Perspective | Shift+R / Shift+F / Shift+P |
| Pencil / Brush / Eraser | N / P / Shift+E |
| Gradient | G |
| Copy / Cut / Paste / Paste as New Layer | ⌘C / ⌘X / ⌘V / ⌘⇧V |
| Select All / Select None / Invert | ⌘A / ⌘⇧A / ⌘I |
| Foreground / Background color dialog | ⌥O / ⌥⇧O |
| Swap / Reset colors | X / D |
| Zoom tool / Zoom In / Zoom Out | Z / + / − |
| Actual pixels / Fit | 1 / ⌘⇧J |

Menus and the toolbox still show planned features such as text.
Commands without handlers appear disabled. Windows is a future target.

## Development

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
```

See [AGENTS.md](AGENTS.md) for the architecture, current scope, and macOS
smoke checks.
