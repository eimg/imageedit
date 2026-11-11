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
  canvas or fills it with the background swatch. The empty workspace offers
  the same action as a quiet button under Choose Files.
- **File → Open** accepts ImageEdit projects, PNG, JPEG, and WebP. You can
  also drop files onto the canvas or use Finder’s **Open With** menu.
  Reopening the same file focuses its existing tab.
- Apply or discard an active text, transform, gradient, or crop preview before
  saving or exporting. Closing an image with a pending edit asks before discarding it.
- **Save** updates the current file. New canvases are saved as `.imageedit`
  projects. An opened PNG, JPEG, or WebP keeps saving to its own format while
  it is still a single plain layer without paths or editable text. **Save As** adopts the new filename and
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
- Drag a rectangle with **Crop** (`C`). Drag inside the box to move it, drag a
  handle to resize it, and read X, Y, Width, and Height in the Crop options.
  Manual entry in those fields is temporarily disabled. Press Return to apply
  the crop or Escape to cancel. **Bucket Fill** (`B`) uses the foreground color
  on connected pixels within the shared tolerance. **Color Picker** (`K`)
  samples the image at full resolution.
  **Rectangle Select** (`R`) drags a rectangle, with a corner-radius control
  that has minus and plus buttons. **Oval Select** (`O`) drags an oval; enable
  **Circle** in its options to constrain it to an exact circle. Each outline is
  a dashed box or ellipse with handles: drag a handle to resize it, drag inside
  to move it, and click outside it to deselect. The Width and Height fields show
  the current size while drawing; manual entry is temporarily disabled. The
  dashes move along the outline. The pointer becomes a resize arrow on a handle
  and a hand inside the box. Select All, Deselect, and Invert are in the Select
  menu and in the tool options.
  **Free Select** (`F`) works two ways: drag to add a freehand segment, or click
  to add a vertex. Releasing keeps the draft open as a checkpoint. Click the
  start marker or press Return to close it, and Escape cancels the draft.
  **Magic Wand**
  (`W`) selects a connected region. Its tolerance is shared with Bucket Fill:
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
  align the layer to any canvas edge or center it with icon buttons.
  **View → Toggle Layer Boundary** shows a global subtle outline that follows
  the active layer. **Layer → Align** offers the same placements. **Layer to
  Image Size** keeps
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
  **Pencil**, **Brush**, and **Eraser** (`E`) draw on the
  active layer. Pencil starts at one pixel and uses a hard edge. Brush and
  Eraser also offer a hardness control. Release commits the whole stroke and
  Escape discards it. Shift-click extends from the last committed stroke in
  the current tab when it was on the same layer. The eraser lowers alpha, and
  a selection limits where the stroke lands.
  **Gradient** (`G`) drags a line across the active layer. It can be linear or
  radial, and it fades from the foreground color to the background or to
  transparent. Return applies the preview and Escape cancels it. A click with
  no drag fills with the foreground color.
  **Path** (`P`) clicks anchors onto the canvas and drags to pull cubic
  handles. Click the first anchor, or press Return, to close a path. Escape
  cancels the drag and Backspace removes the selected anchor. **Select → Path
  to Selection** fills every closed path. Overlapping contours use nonzero
  winding, so a reversed contour cancels the area it shares. Open paths do
  not fill. Undo restores the previous anchors. Image transforms, scaling,
  canvas size, and crop move the paths with the image.
  **Stroke** (`S`) paints the active path, or the selection edge, with the
  brush. Edit → Stroke Path and Stroke Selection do the same. Return commits
  the stroke shown in the tool options. A repeated endpoint does not add a
  dab, and a hole is stroked on both boundaries. The brush is not clipped by
  the selection. Undo restores the previous pixels and leaves the path in place.
  **Text** (`T`) clicks an origin, or loads the active text layer. Type into
  the draft. Return starts a new line, and the copy, cut, paste, and select-all
  shortcuts and native Edit menu actions edit the text instead of the image.
  Undo and Redo stay inside the draft. Apply commits one image undo step
  and Escape discards the draft. Apply or discard modified text before switching
  tools, layers, or tabs. Sans, serif, and monospace are the font
  choices, with a pixel size from 1 to 256 and the foreground color. A missing
  named font shows a substitution notice. Reopening a text layer keeps its saved
  pixels until its words or style change. Previews render in the background and
  follow the layer's opacity and stack position. The layer keeps the words and
  a raster copy, so export matches the canvas. **Layer → Rasterize Text
  Layer** keeps those pixels and lets the pixel tools edit them. Until then,
  painting, fills, filters, and transforms ask you to rasterize first.
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
  underneath, offset from the source and colored with the foreground color.
  Zero blur radius gives a sharp shadow. Blur and Sharpen use zero radius to
  preserve the source pixels. Undo restores the
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
  image. Hold Space and drag to pan without leaving the current tool, or use
  the Pan tool for left-button dragging. Space still types while editing text.
  Fitting or zooming out uses a reduced preview; zooming in shows source
  pixels. The cursor shows the selected tool beside the canvas crosshair.
- **Edit → Undo** and **Redo** step through crops, fills, selections, and layer changes in
  the current tab. Undoing back to the last save clears the unsaved marker.
  Selection changes preserve the document’s saved state.
  Each tab keeps up to 100 steps, and the oldest steps are dropped first when
  history uses too much memory.
- The **Layers** panel lists layers top to bottom with thumbnails. A
  transparency checker shows through clear pixels there and on the canvas.
  Click a layer to select it, double-click to rename it, or use the eye button to hide
  it. Drag a row to reorder. The opacity slider and the Layer menu cover New,
  Duplicate, Rename, Merge Down, Delete, Raise and Lower. New Layer can start
  transparent or filled with the foreground or background color. Merge Down
  stays disabled when either layer is hidden.

The toolbox can be resized by dragging its right edge or folded to an icon rail
with the header toggle. An expanded tool shows its full name with the shortcut,
trimming the name when the sidebar is narrow, and a tooltip on hover. Folded
tools keep the icon and tooltip. The foreground and background swatches stack
vertically in the folded rail, with the swap control between them. Menus keep
the full names used below.

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
| Eraser | E |
| Gradient / Path / Stroke / Text | G / P / S / T |
| Pan | hold Space |
| Copy / Cut / Paste / Paste as New Layer | ⌘C / ⌘X / ⌘V / ⌘⇧V |
| Select All / Select None / Invert | ⌘A / ⌘⇧A / ⌘I |
| Foreground / Background color dialog | ⌥O / ⌥⇧O |
| Swap / Reset colors | X / D |
| Zoom tool / Zoom In / Zoom Out | Z / + / − |
| Actual pixels / Fit | 1 / ⌘⇧J |

Commands without handlers appear disabled. On Windows, Quit is File → Exit
(Ctrl+Q) and Redo is Ctrl+Y. The Windows gnu target is cross-checked from
macOS. The app has not been run on Windows.

## Development

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
```

See [AGENTS.md](AGENTS.md) for the architecture, current scope, and macOS
smoke checks.
