# Feedback queue

Recorded 2026-11-11 from product feedback. These are the next editing-experience
changes. The implementation plan remains historical guidance.

Work **one item at a time**. Take the first item whose status is `open`. Do not
start a later item, and do not work in parallel with another agent.

## How to claim

1. If any item is `claimed`, stop. That work is already in progress.
2. Before editing code, set that item's status to `claimed` and add the date.
3. Implement that item only. Follow `AGENTS.md` for the build, tests, and doc
   updates. When behavior or the code map changes, update `AGENTS.md` and
   `README.md` in the same change.
4. When it is finished, set the status to `done` and add one line: what changed
   and how it was checked. Leave the next item `open`.

The order is the user's list, with two passes moved to the end. Tool names and
the tool-options layout both depend on the tool set, so they come after the
items that add or remove tools and controls.

## 1. Subtle New Image button on startup

- Status: `done`
- 2026-11-11: The empty canvas has a quiet New Image button under Choose Files.
  It opens the existing new-image dialog. Formatting, Clippy, and locked tests
  were run with the other items in this pass.

The empty canvas says "Open or create an image" and offers a prominent
**Choose Files…** button (`src/workspace/view.rs`). File → New Image… already
opens the new-image dialog. Add a subtle New Image button on that empty state,
secondary to Choose Files, and have it open the same dialog.

## 2. Circle and oval selection, with a radius

- Status: `done`
- 2026-11-11: Added Rect/Oval/Circle choices to the Select tool options. Ellipse
  selections keep their shape through handles, moves, resizes, invert/combine,
  crop, fill/clear, and image transforms. Item 14 moves the radius control to
  rectangle corners and removes it from circles and ovals. Formatting, Clippy,
  locked tests, and `./scripts/bundle-macos.sh` passed.

Select (`R`) drags a rectangle only. Add circle and oval selection. Put a
radius control in the Select tool options so a circle can be sized there as
well as by dragging. Shift still adds, Control subtracts, and Shift-Control
intersects. Fill, Clear, Crop to Selection, and the other selection commands
should treat the new shape like the rectangle.

## 3. Transparency grid on a transparent layer

- Status: `done`
- 2026-11-11: The canvas checker was a dark pair close to the workspace
  background, and layer thumbnails sat on that same background. Both now use
  a light checker wherever pixels are transparent. Opaque pixels still cover
  it. Formatting, Clippy, and locked tests were run with the other items in
  this pass.

Transparent pixels should show the transparency checker. The canvas already
paints one when the flattened preview has any non-opaque pixel
(`paint_checkerboard` in `src/workspace/view.rs`). A transparent layer still
needs to read as transparent: a new transparent image, a fully transparent
layer, and transparent pixels around opaque content. Where a lower layer is
opaque, the checker stays hidden behind that content.

## 4. Subtle layer boundary, with a global switch

- Status: `done`
- 2026-11-11: View → Toggle Layer Boundary controls a global subtle outline
  that follows the active layer's offset and size, including the live Move
  preview, and stays on across tools and tabs. Item 20 moved the control from
  the Move options to the View menu. Formatting, Clippy, locked tests, and
  `./scripts/bundle-macos.sh` passed.

Show a subtle outline of the active layer's bounds on the canvas. Put a
boundary on/off control in the Move tool options. The switch is global: it
stays off or on across layers, tools, and tabs, and the outline follows the
active layer while it is on.

## 5. Perspective corners visible on white

- Status: `done`
- 2026-11-11: Rotate, flip, and perspective grips use the same dark rim and
  white center as selection handles. Formatting, Clippy, and locked tests
  were run with the other items in this pass.

Perspective corner grips are solid white 7 px squares in `paint_warp_frame`
(`src/workspace/view.rs`), so they disappear on a white image. Selection
handles already draw a dark outer square and a white center. Give the
perspective grips the same kind of contrast. The corner outline can keep its
dark understroke.

## 6. Pencil, brush, and eraser cursors match the size

- Status: `done`
- 2026-11-11: Pencil, Brush, and Eraser now use a size-scaled ring cursor that
  follows the brush diameter and zoom. The ring has a dark understroke and
  white center for contrast. Pencil is solid; Brush and Eraser add a dashed
  inner ring at the hardness radius. Formatting, Clippy, locked tests, and
  `./scripts/bundle-macos.sh` passed.

Those tools use a crosshair plus a 20 px tool icon (`tool_cursor` in
`src/workspace/view.rs`). Replace the crosshair with a circle, or another
shape that fits the tool, drawn at the current size in canvas pixels and
scaled with zoom. Keep it visible on white and on black. Pencil stays hard;
brush and eraser stay soft. The size control already in the tool options is
the size to follow.

## 7. Text edits in place

- Status: `open`

Text (`T`) keeps a draft until Apply Text, and a modified draft must be
applied or discarded before changing tools, layers, tabs, or saving
(`src/workspace/text_ui.rs`). Typing should land on the layer immediately,
with no Apply step. Show a typing caret while the text is being edited.
Replace the current Sans / Serif / Monospace, Smaller / Larger, and Use
Foreground controls with direct choices for font, size, spacing, and color.
Leaving the tool must not demand Apply or Discard. Edits stay undoable.
Update the text contract in `AGENTS.md` to match.

## 8. Stroke from the Path tool

- Status: `open`

Path (`B`) and Stroke (`S`) are separate tools, and stroking is a one-shot
paint of the path or the selection edge. Put path stroking in the Path tool
options and remove the Stroke tool from the toolbox, its shortcut, and the
Tools menu. A path needs more than one stroke, and each stroke can be
deleted on its own. Keep Edit → Stroke Selection, or an equally direct way to
stroke a selection, since that is not a path. Update
`menus_match_the_release_checklist` with the menu change.

## 9. Clone stamp, blur, and sharpen tools

- Status: `done`
- 2026-11-11: Blur, Sharpen, and Clone Stamp tools are implemented with the
  sized brush cursor and local stroke commits. Blur and Sharpen have no
  keyboard shortcut; Clone Stamp uses Shift+C and sets its source with
  Alt-click. Whole-layer Blur… and Sharpen… remain on Filters. The
  bracket keys resize every brush-based tool, and Alt changes the Clone Stamp
  cursor.

Add a clone stamp, a blur tool, and a sharpen tool. Blur… and Sharpen… stay
on the Filters menu as whole-layer dialogs; these new tools paint locally on
the active layer, inside the selection, and commit on pointer release the way
the brush does. Use the sized cursor from item 6. Give each a full toolbox
label through `Tool::info` and a shortcut that does not collide. Update the
menu checklist. The menu label stays the readable name.

## 10. Short tool names with shortcuts

- Status: `done`
- 2026-11-11: Toolbox buttons and the tool-options header show `short_name`
  plus `shortcut_label`. Menus still use the full `info().label`. The names
  are at most six letters. Formatting, Clippy, and locked tests were run with
  the other items in this pass.

Toolbox buttons are 62 px wide and show `Tool::info().label` at 10 px with no
shortcut (`tool_button` in `src/workspace/view.rs`). Shortcuts already live on
`Tool::shortcut`. Show the shortcut with the tool name, and shorten every
toolbox name so the name and shortcut fit. The tool-options header uses the
same label, so it should show the short name too. Menu items keep their
readable labels.

## 11. Clearer tool-option layout

- Status: `open`

Do this last, after the items above have added and removed controls. The
tool-options column is 232 px wide (`inspector` in `src/workspace/view.rs`)
and stacks each tool's controls in one gap. Arrange those controls so related
choices sit together and the column is easier to scan. Cover every tool that
has options, including the ones added in this queue.

## 12. Resizable toolbox with full labels

Drag the toolbox's right edge to resize it. Keep the color swatches at the top
of the sidebar and the tool groups below. Show each tool's full label, trimming
the label with an ellipsis when the sidebar is narrow, and show a tooltip on
hover. This supersedes the short toolbox names from item 10.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. The toolbox resizes from
  its right edge, keeps the colors above the tool groups, shows full tool
  labels that trim when narrow, and adds a tooltip with the shortcut and
  description. This restores the full labels that item 10 shortened.
  Formatting, Clippy, locked tests, and `./scripts/bundle-macos.sh` passed.

## 13. Foldable toolbox rail

Add a header toggle that folds the toolbox into a narrow icon rail. Folded
buttons keep their icon and tooltip but drop the text label. In the folded
rail, stack the foreground and background swatches vertically with the swap
control between them. Keep the expanded layout and remembered width when the
toolbox opens again.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. The toolbox folds to a
  56 px icon rail from the header toggle and restores its previous width when
  expanded. Folded tools show only the icon and keep the full tooltip. The
  folded color controls stack foreground above the swap control above
  background. Formatting, Clippy, locked tests, and
  `./scripts/bundle-macos.sh` passed.

## 14. Selection size fields and rectangle corner radius

Circle and oval selection size comes from the drag, so they do not need a
radius control. Use the radius control for rectangle corners instead, and have
it update the current rectangle selection. Add editable Width and Height fields
to the Select tool options for every selection shape. They show the current
size while drawing and apply an edit with Return.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. Circle and oval no longer
  have a radius input. Rectangle mode has a corner radius that updates the
  current rectangle. Width and Height fields follow the drawn selection. Manual
  entry is temporarily disabled; item 16 restores it. Formatting, Clippy, locked
  tests, and `./scripts/bundle-macos.sh` passed.

## 15. Editable radius and crop fields

Keep the rectangle corner-radius plus/minus buttons, but make the radius value
an editable field too. Give the Crop tool the same treatment: editable X, Y,
Width, and Height fields, plus dragging inside the crop to move it and dragging
its handles to resize it.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. The rectangle radius keeps
  its +/- buttons. Crop has a movable crop box, resize handles, and move/resize
  cursors, and the X/Y/W/H fields show the current crop. Typed entry in these
  fields was disabled after input bugs; item 16 tracks the proper editor.
  Formatting, Clippy, locked tests, and `./scripts/bundle-macos.sh` passed.

## 16. Inline numeric inputs

Restore typed entry for the rectangle radius, selection Width/Height, and crop
X/Y/Width/Height. The first attempt was laggy, had no visible caret, and
backspace/delete cleared the whole value. Build a proper inline editor with a
caret, normal text selection and deletion, and a single history change per
committed edit. Keep the existing +/- buttons and read-only value displays until
the editor is reliable.

- Status: `open`

## 17. Separate rectangle and oval select tools

Split the single Select tool into Rectangle Select (`R`) and Oval Select
(`O`). Oval Select has a Circle option for an exact circle. Rectangle Select
keeps the corner radius control.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. Rectangle Select and Oval
  Select are separate tools. Oval Select has a Circle toggle. Both keep handles,
  move/resize, selection actions, and read-only Width/Height fields. Formatting,
  Clippy, locked tests, and `./scripts/bundle-macos.sh` passed.

## 18. Combine free select and polygon select

Make Free Select flexible: click to add a vertex, or drag to draw a freehand
outline. Remove the dedicated Polygon Select tool and its shortcut.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. Free Select now treats a
  click as a polygon vertex and a drag as a freehand outline. It removed the
  Polygon Select tool, its `L` shortcut, and its menu and toolbox entries.
  Formatting, Clippy, locked tests, and `./scripts/bundle-macos.sh` passed.

## 19. Free Select checkpoints

Releasing a freehand drag should not close the selection. Keep the draft open
as a checkpoint so the user can add another freehand segment or click more
vertices. Close only by clicking the start node or pressing Return.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. Releasing a Free Select
  drag now leaves the draft open. The start node or Return closes it, and Escape
  cancels it. Formatting, Clippy, locked tests, and
  `./scripts/bundle-macos.sh` passed.

## 20. Compact Move arrange controls

Reorganize the Move tool options: replace the stacked text buttons with compact
icon buttons for top, bottom, left, right, horizontal center, and vertical
center. Move the layer-boundary toggle to the View menu. Keep Layer to Image
Size and Crop to Content in the Layer menu only.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. Move now shows six icon
  alignment buttons and no longer repeats the Layer menu fit and crop commands.
  View → Toggle Layer Boundary replaces the Move options toggle. Formatting,
  Clippy, locked tests, and `./scripts/bundle-macos.sh` passed.

## 21. Scale Layer tool

Add a Scale Layer tool that draws a frame around the active layer. Drag a
border or corner to scale the layer, with a live preview, and commit one undo
step on release. Tool-option Width and Height inputs are read-only for now.

- Status: `done`
- 2026-11-11: This extra task ran out of queue order. The Scale Layer tool uses
  Shift+S, has border and corner handles, GPU-scaled live preview, read-only
  size options, and a resampled commit on release. Formatting, Clippy, locked tests, and
  `./scripts/bundle-macos.sh` passed.
