use gpui::{Action, App, KeyBinding, Menu, MenuItem, NoAction, SystemMenuType, actions};

actions!(
    lime,
    [
        NewImage,
        OpenImage,
        SaveImage,
        SaveImageAs,
        SaveProjectAs,
        ExportPng,
        ExportJpeg,
        ExportWebp,
        Undo,
        Redo,
        NewLayer,
        DuplicateLayer,
        RenameLayer,
        MergeDown,
        DeleteLayer,
        ToggleLayerVisibility,
        LayerOpacity,
        RaiseLayer,
        LowerLayer,
        ForegroundColor,
        BackgroundColor,
        SwapColors,
        ResetColors,
        ZoomIn,
        ZoomOut,
        ApplyCrop,
        CancelCrop,
        SelectAll,
        SelectNone,
        InvertSelection,
        ClearToTransparency,
        CropToSelection,
        CropLayerToContent,
        CropImageToContent,
        LayerToImageSize,
        CancelMove,
        NewFromClipboard,
        OpenAsLayers,
        CutImage,
        CopyImage,
        PasteImage,
        PasteAsLayer,
        FeatherSelection,
        GrowSelection,
        ShrinkSelection,
        CancelFreeSelect,
        CloseFreeSelect,
        ApplyTransform,
        CancelTransform,
        ApplyGradient,
        CancelGradient,
        CancelStroke,
        ScaleImage,
        ScaleLayer,
        CanvasSize,
        SetRgb,
        SetGrayscale,
        BrightnessContrast,
        HueSaturation,
        Desaturate,
        InvertColors,
        ColorBalance,
        Levels,
        Curves,
        GaussianBlur,
        Sharpen,
        DropShadow,
        PathToSelection,
        SelectionToPath,
        ClosePath,
        CancelPath,
        DeletePathAnchor,
        StrokePath,
        StrokeSelection,
        BrushSizeDown,
        BrushSizeUp,
        FinishText,
        RasterizeText,
        TextCopy,
        TextCut,
        TextPaste,
        TextSelectAll,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleGrid,
        ToggleGuides,
        ToggleSnapGrid,
        ToggleSnapGuides,
        ToggleLayerBoundary,
        MinimizeWindow,
        ZoomWindow,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
    ]
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Pan,
    Zoom,
    Eyedropper,
    Crop,
    Rotate,
    Flip,
    Perspective,
    ScaleLayer,
    Select,
    OvalSelect,
    FreeSelect,
    MagicWand,
    Pencil,
    Brush,
    Eraser,
    Blur,
    Sharpen,
    CloneStamp,
    BucketFill,
    Gradient,
    Text,
    Path,
}

pub struct ToolInfo {
    pub label: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
}

impl Tool {
    pub fn info(self) -> ToolInfo {
        let (label, icon, description) = match self {
            Self::Move => (
                "Move",
                "move",
                "Drag to reposition the active layer. Escape cancels.",
            ),
            Self::Pan => (
                "Pan",
                "hand",
                "Hold Space and drag, or choose this tool and drag, to move the canvas.",
            ),
            Self::Zoom => (
                "Zoom",
                "zoom",
                "Click to zoom in. Control-click to zoom out.",
            ),
            Self::Eyedropper => (
                "Color Picker",
                "eyedropper",
                "Click a pixel to set the foreground color.",
            ),
            Self::Crop => (
                "Crop",
                "crop",
                "Drag a rectangle, then press Return or choose Apply Crop.",
            ),
            Self::Rotate => (
                "Rotate",
                "rotate",
                "Drag to turn the active layer. Return applies it and Escape cancels.",
            ),
            Self::Flip => (
                "Flip",
                "flip",
                "Mirror the active layer horizontally or vertically, then apply.",
            ),
            Self::Perspective => (
                "Perspective",
                "perspective",
                "Drag the corners, then apply. The selection stays where it is.",
            ),
            Self::ScaleLayer => (
                "Scale Layer",
                "scale",
                "Drag a border or corner to scale the active layer.",
            ),
            Self::Select => (
                "Select",
                "select",
                "Drag a rectangle. Shift adds, Control subtracts, and Shift-Control intersects.",
            ),
            Self::OvalSelect => (
                "Oval Select",
                "oval",
                "Drag an oval, or enable Circle to constrain it to an exact circle.",
            ),
            Self::FreeSelect => (
                "Free Select",
                "lasso",
                "Drag a freehand outline, or click to add vertices. Click the start or press Return to close. Escape cancels.",
            ),
            Self::MagicWand => (
                "Magic Wand",
                "wand",
                "Select a connected region. Tolerance 0 matches the color exactly.",
            ),
            Self::Pencil => (
                "Pencil",
                "pencil",
                "Draw with the foreground color. Release commits the stroke.",
            ),
            Self::Brush => (
                "Brush",
                "brush",
                "Paint a soft stroke. Shift-click continues from the last point.",
            ),
            Self::Eraser => (
                "Eraser",
                "eraser",
                "Reduce the layer's alpha. Escape discards the stroke.",
            ),
            Self::Blur => (
                "Blur",
                "blur",
                "Soften the active layer under the brush. Release commits the stroke.",
            ),
            Self::Sharpen => (
                "Sharpen",
                "sharpen",
                "Sharpen the active layer under the brush. Release commits the stroke.",
            ),
            Self::CloneStamp => (
                "Clone Stamp",
                "clone",
                "Alt-click to set the source, then paint to copy pixels from it.",
            ),
            Self::BucketFill => (
                "Bucket Fill",
                "bucket",
                "Fill a connected region or the whole selection. Sample All Layers reads the composite.",
            ),
            Self::Gradient => (
                "Gradient Fill",
                "gradient",
                "Drag the line, then apply. Escape cancels. The selection limits the fill.",
            ),
            Self::Text => (
                "Text",
                "text",
                "Click to add or edit text, then type. Escape or leaving the text commits it.",
            ),
            Self::Path => (
                "Path",
                "path",
                "Click to add anchors and drag for handles. Click the first anchor to close.",
            ),
        };
        ToolInfo {
            label,
            icon,
            description,
        }
    }

    /// Essential instructions shown on demand in Tool Options.
    pub fn help(self) -> &'static str {
        match self {
            Self::Move => {
                "Drag to move the active layer; Escape cancels. Snap to Grid/Guides aligns layer edges and centers; Control temporarily bypasses snapping and guide grabs. Typed X/Y positions and arrow nudges stay exact. Return or leaving an X/Y field commits; Escape or empty text restores it. Arrows nudge one image pixel, or ten with Shift; release commits the sequence. View → Toggle Rulers and Guides shows pixel rulers: drag from the top/left ruler to create a horizontal/vertical guide, drag a guide to move it, or release outside the canvas workspace to remove it. Guides are saved in Lime projects."
            }
            Self::Pan => {
                "Drag to move the canvas. Hold Space and drag to pan temporarily from another tool."
            }
            Self::Zoom => {
                "Click to zoom in. Control-click or right-click zooms out. Zoom follows the pointer; hold Space and drag to pan."
            }
            Self::Eyedropper => {
                "Click the image to sample the foreground color from the visible layers. The color swatches are at the top of the toolbox."
            }
            Self::Crop => {
                "Drag a crop box. Drag inside to move it or drag a handle to resize it. Choose Free or a fixed aspect ratio; edit Ratio W/H for a custom ratio. The ratio applies on the next drag, resize, or dimension edit. X, Y, Width, and Height update immediately; empty text restores the field's original value. Return applies the crop; Escape cancels."
            }
            Self::Rotate => {
                "Drag to rotate the active layer or type an angle in degrees. Hold Shift while dragging to snap to 15-degree angles. Return finishes the numeric edit; Apply or Return on the canvas applies the preview. Escape cancels the numeric edit or preview. The selection stays in canvas coordinates."
            }
            Self::Flip => {
                "Choose Horizontal or Vertical, or drag to choose a direction. Return applies the flip; Escape cancels. The selection stays in canvas coordinates."
            }
            Self::Perspective => {
                "Drag the corner grips to change perspective. Keep the corners in a convex shape. Return applies the preview; Escape cancels. The selection stays in canvas coordinates."
            }
            Self::ScaleLayer => {
                "Drag a border or corner to scale the active layer. Release commits one undo step; Escape cancels. Type Width or Height for a centered live preview; Return or clicking outside commits, and empty text restores the original size. Lock aspect ratio links dimensions; Shift temporarily reverses the lock while dragging. Choose the resampling used at commit. Rasterize editable text before scaling it."
            }
            Self::Select => {
                "Drag a rectangle. Drag inside to move it or drag a handle to resize it; click outside to deselect. X, Y, Width, Height, and corner radius update immediately; empty text or Escape restores the numeric edit. Choose Free or a fixed aspect ratio; Ratio W/H accepts a custom ratio for the next drag, resize, or dimension edit. Antialias edges smooths rounded corners. Shift adds, Control subtracts, and Shift-Control intersects."
            }
            Self::OvalSelect => {
                "Drag an oval; enable Circle for a circle, or choose a fixed aspect ratio. Ratio W/H accepts a custom ratio for the next drag, resize, or dimension edit. Drag inside to move it or drag a handle to resize it; click outside to deselect. X, Y, Width, and Height update immediately; empty text or Escape restores the numeric edit. Antialias edges smooths the curve. Shift adds, Control subtracts, and Shift-Control intersects."
            }
            Self::FreeSelect => {
                "Click to add vertices or drag to draw a freehand segment. Releasing keeps the outline open. Click its start or press Return to close it; Escape cancels. Shift adds, Control subtracts, and Shift-Control intersects."
            }
            Self::MagicWand => {
                "Click a connected region to select it. Sample All Layers uses the visible composite; otherwise only the active layer is sampled. Tolerance is shared with Bucket Fill; zero matches exactly, ignoring stored RGB when both pixels are fully transparent. Shift adds, Control subtracts, and Shift-Control intersects."
            }
            Self::Pencil | Self::Brush => {
                "Paint with the foreground color inside the selection. Release commits one stroke; Escape cancels. Shift-click continues from the last stroke endpoint. The bracket keys [ and ] change the brush size. Pencil has a hard edge; Brush uses the chosen hardness."
            }
            Self::Eraser => {
                "Paint to erase to transparency inside the selection. Release commits one stroke; Escape cancels. Shift-click continues from the last stroke endpoint. The bracket keys [ and ] change the brush size."
            }
            Self::Blur | Self::Sharpen => {
                "Paint to change local detail on the active layer, inside the selection. Release commits one stroke; Escape cancels. Shift-click continues from the last stroke endpoint. The bracket keys [ and ] change the brush size."
            }
            Self::CloneStamp => {
                "Alt-click inside the active raster layer to set the source. Aligned keeps the source offset across strokes; turn it off to restart each stroke at the source. Changing the mode resets alignment. Choose a new source after changing tabs or layers, or moving/resizing the layer. The selection limits the destination. Release commits one stroke; Escape cancels. The bracket keys [ and ] change the brush size."
            }
            Self::BucketFill => {
                "Click to fill with the foreground color. Connected fills a connected region; Selection covers the whole selection. Sample All Layers reads visible layers to find the region, then paints only the active layer. Tolerance 0 matches exactly, ignoring stored RGB when both pixels are fully transparent."
            }
            Self::Gradient => {
                "Drag a line to preview the gradient, then adjust its endpoints. FG → BG blends the foreground and background colors; FG → Clear fades to transparency. The selection limits the fill. Return applies it; Escape cancels."
            }
            Self::Text => {
                "Click empty canvas to add text, or click a text layer to edit it. Drag, Shift-click, or Shift+arrows select a range; Copy/Cut use it and typing/Paste replace it. Option+Left/Right and Option+Backspace/Delete on macOS (Ctrl on Windows) work by word; Shift extends the selection. Return adds a line. Font/style controls affect the whole layer. In the font picker, type to filter; arrows browse, Return chooses, and Escape closes it. Outside the picker, Escape, Cmd/Ctrl-Enter, clicking outside, or changing tools/layers/tabs commits. Undo steps through typing first. Rasterize Text enables pixel tools."
            }
            Self::Path => {
                "Click to add anchors; drag to pull their handles. Click the first anchor to close the path. Escape or changing tools ends the current path; the next click starts a new one. Use the Paths tab to select, show, hide, delete, convert, or stroke paths. Stroke uses the foreground color and needs a visible path and a paintable raster layer."
            }
        }
    }

    /// Compact shortcut drawn beside the tool label. Pan shows the hold key.
    pub fn shortcut_label(self) -> Option<&'static str> {
        if self == Self::Pan {
            return Some("␣");
        }
        Some(match self.shortcut()? {
            "m" => "M",
            "c" => "C",
            "z" => "Z",
            "k" => "K",
            "shift-r" => "⇧R",
            "shift-f" => "⇧F",
            "shift-p" => "⇧P",
            "shift-s" => "⇧S",
            "shift-c" => "⇧C",
            "r" => "R",
            "o" => "O",
            "f" => "F",
            "w" => "W",
            "e" => "E",
            "b" => "B",
            "g" => "G",
            "t" => "T",
            "p" => "P",
            "s" => "S",
            _ => return None,
        })
    }

    /// Toolbox shortcut. Pan is hold-Space. Pencil and Brush stay in the toolbox.
    pub fn shortcut(self) -> Option<&'static str> {
        Some(match self {
            Self::Move => "m",
            Self::Pan | Self::Pencil | Self::Brush | Self::Blur | Self::Sharpen => return None,
            Self::Zoom => "z",
            Self::Eyedropper => "k",
            Self::Crop => "c",
            Self::Rotate => "shift-r",
            Self::Flip => "shift-f",
            Self::Perspective => "shift-p",
            Self::ScaleLayer => "shift-s",
            Self::CloneStamp => "shift-c",
            Self::Select => "r",
            Self::OvalSelect => "o",
            Self::FreeSelect => "f",
            Self::MagicWand => "w",
            Self::Eraser => "e",
            Self::BucketFill => "b",
            Self::Gradient => "g",
            Self::Text => "t",
            Self::Path => "p",
        })
    }
}

const TRANSFORM_TOOLS: &[Tool] = &[
    Tool::Move,
    Tool::Crop,
    Tool::Rotate,
    Tool::Flip,
    Tool::Perspective,
    Tool::ScaleLayer,
];
const SELECTION_TOOLS: &[Tool] = &[
    Tool::Select,
    Tool::OvalSelect,
    Tool::FreeSelect,
    Tool::MagicWand,
];
const PAINT_TOOLS: &[Tool] = &[
    Tool::Pencil,
    Tool::Brush,
    Tool::Eraser,
    Tool::Blur,
    Tool::Sharpen,
    Tool::CloneStamp,
];
const FILL_TOOLS: &[Tool] = &[Tool::BucketFill, Tool::Gradient];
const DRAW_TOOLS: &[Tool] = &[Tool::Text, Tool::Path];

pub const TOOL_GROUPS: &[(&str, &[Tool])] = &[
    ("Transform", TRANSFORM_TOOLS),
    ("Selection", SELECTION_TOOLS),
    ("Paint", PAINT_TOOLS),
    ("Fill", FILL_TOOLS),
    ("Draw", DRAW_TOOLS),
    ("Utilities", &[Tool::Pan, Tool::Zoom, Tool::Eyedropper]),
];

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = lime, no_json)]
pub struct SelectTool(pub Tool);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Action)]
#[action(namespace = lime, no_json)]
pub struct NudgeLayer(pub i32, pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = lime, no_json)]
pub enum AlignLayer {
    Left,
    Right,
    Top,
    Bottom,
    Horizontal,
    Vertical,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = lime, no_json)]
pub enum ImageAdjust {
    RotateLeft,
    RotateRight,
    RotateHalf,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = lime, no_json)]
pub enum LayerAdjust {
    RotateLeft,
    RotateRight,
    RotateHalf,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Action)]
#[action(namespace = lime, no_json)]
pub enum Zoom {
    #[default]
    Fit,
    Percent(f32),
}

pub const OPEN_HINT: &str = if cfg!(target_os = "macos") {
    "⌘O"
} else {
    "Ctrl+O"
};

pub const NEW_HINT: &str = if cfg!(target_os = "macos") {
    "⌘N"
} else {
    "Ctrl+N"
};

pub fn register(cx: &mut App) {
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{primary}-n"), NewImage, None),
        KeyBinding::new(&format!("{primary}-o"), OpenImage, None),
        KeyBinding::new(&format!("{primary}-s"), SaveImage, None),
        KeyBinding::new(&format!("{primary}-shift-s"), SaveImageAs, None),
        KeyBinding::new(&format!("{primary}-="), ZoomIn, None),
        KeyBinding::new(&format!("{primary}--"), ZoomOut, None),
        KeyBinding::new(&format!("{primary}-alt-s"), SaveProjectAs, None),
        KeyBinding::new(&format!("{primary}-shift-e"), ExportPng, None),
        KeyBinding::new(&format!("{primary}-shift-n"), NewLayer, None),
        KeyBinding::new(&format!("{primary}-shift-d"), DuplicateLayer, None),
        KeyBinding::new(&format!("{primary}-shift-j"), Zoom::Fit, None),
        KeyBinding::new("shift-=", ZoomIn, None),
        KeyBinding::new("+", ZoomIn, None),
        KeyBinding::new("-", ZoomOut, None),
        KeyBinding::new("1", Zoom::Percent(100.), None),
        KeyBinding::new("escape", CancelMove, Some("Move")),
        KeyBinding::new("escape", CancelMove, Some("ScaleLayer")),
        KeyBinding::new("left", NudgeLayer(-1, 0), Some("Move")),
        KeyBinding::new("right", NudgeLayer(1, 0), Some("Move")),
        KeyBinding::new("up", NudgeLayer(0, -1), Some("Move")),
        KeyBinding::new("down", NudgeLayer(0, 1), Some("Move")),
        KeyBinding::new("shift-left", NudgeLayer(-10, 0), Some("Move")),
        KeyBinding::new("shift-right", NudgeLayer(10, 0), Some("Move")),
        KeyBinding::new("shift-up", NudgeLayer(0, -10), Some("Move")),
        KeyBinding::new("shift-down", NudgeLayer(0, 10), Some("Move")),
        KeyBinding::new("escape", CancelFreeSelect, Some("FreeSelect")),
        KeyBinding::new("enter", CloseFreeSelect, Some("FreeSelect")),
        KeyBinding::new(&format!("{primary}-c"), CopyImage, None),
        KeyBinding::new(&format!("{primary}-x"), CutImage, None),
        KeyBinding::new(&format!("{primary}-v"), PasteImage, None),
        KeyBinding::new(&format!("{primary}-shift-v"), PasteAsLayer, None),
        KeyBinding::new(&format!("{primary}-a"), SelectAll, None),
        KeyBinding::new(&format!("{primary}-shift-a"), SelectNone, None),
        KeyBinding::new(&format!("{primary}-i"), InvertSelection, None),
        KeyBinding::new("escape", CancelStroke, Some("Paint")),
        KeyBinding::new("enter", ApplyTransform, Some("Transform")),
        KeyBinding::new("escape", CancelTransform, Some("Transform")),
        KeyBinding::new("[", BrushSizeDown, None),
        KeyBinding::new("]", BrushSizeUp, None),
        KeyBinding::new("escape", CancelPath, Some("Path")),
        KeyBinding::new("enter", ClosePath, Some("Path")),
        KeyBinding::new("backspace", DeletePathAnchor, Some("Path")),
        KeyBinding::new("delete", DeletePathAnchor, Some("Path")),
        KeyBinding::new("enter", ApplyGradient, Some("Gradient")),
        KeyBinding::new("escape", CancelGradient, Some("Gradient")),
        KeyBinding::new("x", SwapColors, None),
        KeyBinding::new("d", ResetColors, None),
        KeyBinding::new("alt-o", ForegroundColor, None),
        KeyBinding::new("alt-shift-o", BackgroundColor, None),
        KeyBinding::new("enter", ApplyCrop, Some("Crop")),
        KeyBinding::new("escape", CancelCrop, Some("Crop")),
        KeyBinding::new(&format!("{primary}-w"), CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-z", Undo, None),
            KeyBinding::new("cmd-shift-z", Redo, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-y", Redo, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
    }
    bind_tool_shortcuts(cx);
    // A NoAction here is higher than the global tool letters, so key_down can type them.
    cx.bind_keys([
        KeyBinding::new("m", NoAction, Some("Text")),
        KeyBinding::new("c", NoAction, Some("Text")),
        KeyBinding::new("r", NoAction, Some("Text")),
        KeyBinding::new("o", NoAction, Some("Text")),
        KeyBinding::new("f", NoAction, Some("Text")),
        KeyBinding::new("w", NoAction, Some("Text")),
        KeyBinding::new("e", NoAction, Some("Text")),
        KeyBinding::new("b", NoAction, Some("Text")),
        KeyBinding::new("g", NoAction, Some("Text")),
        KeyBinding::new("t", NoAction, Some("Text")),
        KeyBinding::new("p", NoAction, Some("Text")),
        KeyBinding::new("s", NoAction, Some("Text")),
        KeyBinding::new("z", NoAction, Some("Text")),
        KeyBinding::new("k", NoAction, Some("Text")),
        KeyBinding::new("x", NoAction, Some("Text")),
        KeyBinding::new("d", NoAction, Some("Text")),
        KeyBinding::new("shift-r", NoAction, Some("Text")),
        KeyBinding::new("shift-f", NoAction, Some("Text")),
        KeyBinding::new("shift-p", NoAction, Some("Text")),
        KeyBinding::new("shift-s", NoAction, Some("Text")),
        KeyBinding::new("shift-c", NoAction, Some("Text")),
        KeyBinding::new("1", NoAction, Some("Text")),
        KeyBinding::new("+", NoAction, Some("Text")),
        KeyBinding::new("-", NoAction, Some("Text")),
        KeyBinding::new("=", NoAction, Some("Text")),
        KeyBinding::new("shift-=", NoAction, Some("Text")),
        KeyBinding::new(&format!("{primary}-c"), TextCopy, Some("Text")),
        KeyBinding::new(&format!("{primary}-x"), TextCut, Some("Text")),
        KeyBinding::new(&format!("{primary}-v"), TextPaste, Some("Text")),
        KeyBinding::new(&format!("{primary}-a"), TextSelectAll, Some("Text")),
        KeyBinding::new(&format!("{primary}-shift-v"), NoAction, Some("Text")),
        KeyBinding::new(&format!("{primary}-enter"), FinishText, Some("Text")),
        KeyBinding::new("escape", FinishText, Some("Text")),
    ]);

    let word_modifier = if cfg!(target_os = "macos") {
        "alt"
    } else {
        "ctrl"
    };
    for key in [
        "left",
        "right",
        "up",
        "down",
        "home",
        "end",
        "backspace",
        "delete",
    ] {
        cx.bind_keys([
            KeyBinding::new(&format!("{word_modifier}-{key}"), NoAction, Some("Text")),
            KeyBinding::new(
                &format!("{word_modifier}-shift-{key}"),
                NoAction,
                Some("Text"),
            ),
        ]);
    }
    if cfg!(target_os = "macos") {
        for key in ["left", "right", "up", "down"] {
            cx.bind_keys([
                KeyBinding::new(&format!("cmd-{key}"), NoAction, Some("Text")),
                KeyBinding::new(&format!("cmd-shift-{key}"), NoAction, Some("Text")),
            ]);
        }
    }

    // Window actions run while GPUI has the active window borrowed.
    cx.on_action(|_: &OpenImage, cx| cx.defer(crate::open_dialog));
    cx.on_action(|_: &Quit, cx| cx.defer(crate::quit));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

    cx.set_menus(application_menus(cfg!(target_os = "macos")));
    #[cfg(target_os = "macos")]
    crate::macos::set_menu_icons();
}

fn bind_tool_shortcuts(cx: &mut App) {
    for (_, tools) in TOOL_GROUPS {
        for tool in *tools {
            let Some(key) = tool.shortcut() else {
                continue;
            };
            cx.bind_keys([KeyBinding::new(key, SelectTool(*tool), None)]);
        }
    }
}

pub(crate) fn application_menus(macos: bool) -> Vec<Menu> {
    let mut file = vec![
        MenuItem::action("New Image…", NewImage),
        MenuItem::action("New from Clipboard", NewFromClipboard),
        MenuItem::separator(),
        MenuItem::action("Open…", OpenImage),
        MenuItem::action("Open as Layers…", OpenAsLayers),
        MenuItem::action("Close Tab", CloseTab),
        MenuItem::separator(),
        MenuItem::action("Save", SaveImage),
        MenuItem::action("Save As…", SaveImageAs),
        MenuItem::action("Save Project As…", SaveProjectAs),
        MenuItem::separator(),
        MenuItem::action("Export as PNG…", ExportPng),
        MenuItem::action("Export as JPEG…", ExportJpeg),
        MenuItem::action("Export as WebP…", ExportWebp),
    ];
    if !macos {
        file.extend([MenuItem::separator(), MenuItem::action("Exit", Quit)]);
    }

    let mut menus = Vec::new();
    if macos {
        menus.push(menu(
            "Lime",
            vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Lime", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Lime", Quit),
            ],
        ));
    }
    menus.extend([
        menu("File", file),
        menu(
            "Edit",
            vec![
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", CutImage),
                MenuItem::action("Copy", CopyImage),
                MenuItem::action("Paste", PasteImage),
                MenuItem::action("Paste as New Layer", PasteAsLayer),
                MenuItem::action("Clear to Transparency", ClearToTransparency),
                MenuItem::separator(),
                MenuItem::action("Stroke Path", StrokePath),
                MenuItem::action("Stroke Selection", StrokeSelection),
            ],
        ),
        menu(
            "Select",
            vec![
                MenuItem::action("Rectangle Select", SelectTool(Tool::Select)),
                MenuItem::action("Oval Select", SelectTool(Tool::OvalSelect)),
                MenuItem::separator(),
                MenuItem::action("Select All", SelectAll),
                MenuItem::action("Select None", SelectNone),
                MenuItem::action("Invert Selection", InvertSelection),
                MenuItem::separator(),
                MenuItem::action("Feather…", FeatherSelection),
                MenuItem::action("Grow…", GrowSelection),
                MenuItem::action("Shrink…", ShrinkSelection),
                MenuItem::separator(),
                MenuItem::action("Path to Selection", PathToSelection),
                MenuItem::action("Selection to Path", SelectionToPath),
            ],
        ),
        menu(
            "View",
            vec![
                zoom_menu(),
                tool(Tool::Pan),
                MenuItem::separator(),
                MenuItem::action("Toggle Grid", ToggleGrid),
                MenuItem::action("Toggle Rulers and Guides", ToggleGuides),
                MenuItem::action("Snap to Grid", ToggleSnapGrid),
                MenuItem::action("Snap to Guides", ToggleSnapGuides),
                MenuItem::action("Toggle Layer Boundary", ToggleLayerBoundary),
            ],
        ),
        menu(
            "Image",
            vec![
                MenuItem::submenu(menu(
                    "Mode",
                    vec![
                        MenuItem::action("RGB", SetRgb),
                        MenuItem::action("Grayscale", SetGrayscale),
                    ],
                )),
                image_transform_menu(),
                MenuItem::separator(),
                MenuItem::action("Scale Image…", ScaleImage),
                MenuItem::action("Canvas Size…", CanvasSize),
                tool(Tool::Crop),
                MenuItem::action("Apply Crop", ApplyCrop),
                MenuItem::action("Crop to Selection", CropToSelection),
                MenuItem::action("Crop to Content", CropImageToContent),
            ],
        ),
        menu(
            "Layer",
            vec![
                MenuItem::action("New Layer…", NewLayer),
                MenuItem::action("Duplicate Layer", DuplicateLayer),
                MenuItem::action("Rename Layer…", RenameLayer),
                MenuItem::action("Merge Down", MergeDown),
                MenuItem::action("Delete Layer", DeleteLayer),
                MenuItem::separator(),
                MenuItem::action("Toggle Layer Visibility", ToggleLayerVisibility),
                MenuItem::action("Layer Opacity…", LayerOpacity),
                MenuItem::action("Raise Layer", RaiseLayer),
                MenuItem::action("Lower Layer", LowerLayer),
                MenuItem::separator(),
                layer_transform_menu(),
                MenuItem::submenu(menu(
                    "Align",
                    vec![
                        MenuItem::action("Align Top", AlignLayer::Top),
                        MenuItem::action("Align Bottom", AlignLayer::Bottom),
                        MenuItem::action("Align Left", AlignLayer::Left),
                        MenuItem::action("Align Right", AlignLayer::Right),
                        MenuItem::action("Center Horizontally in Image", AlignLayer::Horizontal),
                        MenuItem::action("Center Vertically in Image", AlignLayer::Vertical),
                        MenuItem::action("Center in Image", AlignLayer::Both),
                    ],
                )),
                MenuItem::action("Scale Layer…", ScaleLayer),
                MenuItem::action("Layer to Image Size", LayerToImageSize),
                MenuItem::action("Crop to Content", CropLayerToContent),
                MenuItem::action("Rasterize Text Layer", RasterizeText),
            ],
        ),
        menu(
            "Tools",
            TOOL_GROUPS
                .iter()
                .map(|(label, tools)| {
                    MenuItem::submenu(menu(label, tools.iter().copied().map(tool).collect()))
                })
                .collect(),
        ),
        menu(
            "Colors",
            vec![
                MenuItem::action("Foreground Color…", ForegroundColor),
                MenuItem::action("Background Color…", BackgroundColor),
                MenuItem::action("Swap Foreground and Background", SwapColors),
                MenuItem::action("Default Colors", ResetColors),
                MenuItem::separator(),
                MenuItem::action("Brightness & Contrast…", BrightnessContrast),
                MenuItem::action("Hue & Saturation…", HueSaturation),
                MenuItem::action("Color Balance…", ColorBalance),
                MenuItem::action("Levels…", Levels),
                MenuItem::action("Curves…", Curves),
                MenuItem::separator(),
                MenuItem::action("Desaturate…", Desaturate),
                MenuItem::action("Invert", InvertColors),
            ],
        ),
        menu(
            "Filters",
            vec![
                MenuItem::action("Blur…", GaussianBlur),
                MenuItem::action("Sharpen…", Sharpen),
                MenuItem::action("Drop Shadow…", DropShadow),
            ],
        ),
        menu(
            "Window",
            vec![
                MenuItem::action("Minimize", MinimizeWindow),
                MenuItem::action("Zoom", ZoomWindow),
                MenuItem::separator(),
                MenuItem::action("Next Tab", NextTab),
                MenuItem::action("Previous Tab", PreviousTab),
            ],
        ),
    ]);
    menus
}

fn image_transform_menu() -> MenuItem {
    MenuItem::submenu(menu(
        "Transform",
        vec![
            MenuItem::action("Rotate 90° Left", ImageAdjust::RotateLeft),
            MenuItem::action("Rotate 90° Right", ImageAdjust::RotateRight),
            MenuItem::action("Rotate 180°", ImageAdjust::RotateHalf),
            MenuItem::separator(),
            MenuItem::action("Flip Horizontally", ImageAdjust::FlipHorizontal),
            MenuItem::action("Flip Vertically", ImageAdjust::FlipVertical),
            MenuItem::separator(),
            tool(Tool::Perspective),
        ],
    ))
}

fn layer_transform_menu() -> MenuItem {
    MenuItem::submenu(menu(
        "Transform",
        vec![
            MenuItem::action("Rotate 90° Left", LayerAdjust::RotateLeft),
            MenuItem::action("Rotate 90° Right", LayerAdjust::RotateRight),
            MenuItem::action("Rotate 180°", LayerAdjust::RotateHalf),
            MenuItem::separator(),
            MenuItem::action("Flip Horizontally", LayerAdjust::FlipHorizontal),
            MenuItem::action("Flip Vertically", LayerAdjust::FlipVertical),
            MenuItem::separator(),
            tool(Tool::Perspective),
        ],
    ))
}

fn zoom_menu() -> MenuItem {
    let mut items = vec![
        MenuItem::action("Zoom In", ZoomIn),
        MenuItem::action("Zoom Out", ZoomOut),
        MenuItem::separator(),
        MenuItem::action("Fit Image in Window", Zoom::Fit),
        MenuItem::separator(),
    ];
    items.extend(
        [25_u16, 50, 100, 200, 400].map(|percent| {
            MenuItem::action(format!("{percent}%"), Zoom::Percent(f32::from(percent)))
        }),
    );
    MenuItem::submenu(menu("Zoom", items))
}

fn menu(name: &'static str, items: Vec<MenuItem>) -> Menu {
    Menu {
        name: name.into(),
        items,
    }
}

fn tool(tool: Tool) -> MenuItem {
    MenuItem::action(tool.info().label, SelectTool(tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_in_one_group_with_a_distinct_shortcut() {
        let tools = [
            Tool::Move,
            Tool::Pan,
            Tool::Zoom,
            Tool::Eyedropper,
            Tool::Crop,
            Tool::Rotate,
            Tool::Flip,
            Tool::Perspective,
            Tool::ScaleLayer,
            Tool::Select,
            Tool::OvalSelect,
            Tool::FreeSelect,
            Tool::MagicWand,
            Tool::Pencil,
            Tool::Brush,
            Tool::Eraser,
            Tool::Blur,
            Tool::Sharpen,
            Tool::CloneStamp,
            Tool::BucketFill,
            Tool::Gradient,
            Tool::Text,
            Tool::Path,
        ];
        let mut shortcuts = std::collections::HashSet::new();
        for tool in tools {
            let groups = TOOL_GROUPS
                .iter()
                .filter(|(_, group)| group.contains(&tool))
                .count();
            assert_eq!(groups, 1, "{tool:?}");
            match tool {
                Tool::Pan => {
                    assert_eq!(tool.shortcut(), None);
                    assert_eq!(tool.shortcut_label(), Some("␣"));
                }
                Tool::Pencil | Tool::Brush | Tool::Blur | Tool::Sharpen => {
                    assert_eq!(tool.shortcut(), None);
                    assert_eq!(tool.shortcut_label(), None);
                }
                Tool::Move => assert_eq!(tool.shortcut(), Some("m")),
                Tool::Zoom => assert_eq!(tool.shortcut(), Some("z")),
                Tool::Eyedropper => assert_eq!(tool.shortcut(), Some("k")),
                Tool::Crop => assert_eq!(tool.shortcut(), Some("c")),
                Tool::Rotate => assert_eq!(tool.shortcut(), Some("shift-r")),
                Tool::Flip => assert_eq!(tool.shortcut(), Some("shift-f")),
                Tool::Perspective => assert_eq!(tool.shortcut(), Some("shift-p")),
                Tool::ScaleLayer => assert_eq!(tool.shortcut(), Some("shift-s")),
                Tool::CloneStamp => assert_eq!(tool.shortcut(), Some("shift-c")),
                Tool::Select => assert_eq!(tool.shortcut(), Some("r")),
                Tool::OvalSelect => assert_eq!(tool.shortcut(), Some("o")),
                Tool::FreeSelect => assert_eq!(tool.shortcut(), Some("f")),
                Tool::MagicWand => assert_eq!(tool.shortcut(), Some("w")),
                Tool::Eraser => assert_eq!(tool.shortcut(), Some("e")),
                Tool::BucketFill => assert_eq!(tool.shortcut(), Some("b")),
                Tool::Gradient => assert_eq!(tool.shortcut(), Some("g")),
                Tool::Text => assert_eq!(tool.shortcut(), Some("t")),
                Tool::Path => assert_eq!(tool.shortcut(), Some("p")),
            }
            if let Some(shortcut) = tool.shortcut() {
                assert!(shortcuts.insert(shortcut), "{shortcut}");
            }
            match (tool.shortcut(), tool.shortcut_label()) {
                (None, None) => assert!(matches!(
                    tool,
                    Tool::Pencil | Tool::Brush | Tool::Blur | Tool::Sharpen
                )),
                (None, Some("␣")) => assert_eq!(tool, Tool::Pan),
                (Some(shortcut), Some(label)) => {
                    let key = shortcut.rsplit('-').next().unwrap();
                    assert_eq!(
                        label
                            .chars()
                            .last()
                            .unwrap()
                            .to_ascii_lowercase()
                            .to_string(),
                        key
                    );
                    assert_eq!(label.starts_with('⇧'), shortcut.starts_with("shift-"));
                    assert!(label.chars().count() <= 2, "{label}");
                }
                (shortcut, label) => panic!("{tool:?} shortcut {shortcut:?} label {label:?}"),
            }
        }
        let listed: usize = TOOL_GROUPS.iter().map(|(_, group)| group.len()).sum();
        assert_eq!(listed, tools.len());
    }

    #[test]
    fn menus_match_the_release_checklist() {
        let macos = menu_labels(&application_menus(true));
        let windows = menu_labels(&application_menus(false));
        let checklist = [
            "menu:Lime",
            "Services",
            "Hide Lime",
            "Hide Others",
            "Show All",
            "Quit Lime",
            "menu:File",
            "New Image…",
            "New from Clipboard",
            "Open…",
            "Open as Layers…",
            "Close Tab",
            "Save",
            "Save As…",
            "Save Project As…",
            "Export as PNG…",
            "Export as JPEG…",
            "Export as WebP…",
            "menu:Edit",
            "Undo",
            "Redo",
            "Cut",
            "Copy",
            "Paste",
            "Paste as New Layer",
            "Clear to Transparency",
            "Stroke Path",
            "Stroke Selection",
            "menu:Select",
            "Rectangle Select",
            "Oval Select",
            "Select All",
            "Select None",
            "Invert Selection",
            "Feather…",
            "Grow…",
            "Shrink…",
            "Path to Selection",
            "Selection to Path",
            "menu:View",
            "menu:Zoom",
            "Zoom In",
            "Zoom Out",
            "Fit Image in Window",
            "25%",
            "50%",
            "100%",
            "200%",
            "400%",
            "Pan",
            "Toggle Grid",
            "Toggle Rulers and Guides",
            "Snap to Grid",
            "Snap to Guides",
            "Toggle Layer Boundary",
            "menu:Image",
            "menu:Mode",
            "RGB",
            "Grayscale",
            "menu:Transform",
            "Rotate 90° Left",
            "Rotate 90° Right",
            "Rotate 180°",
            "Flip Horizontally",
            "Flip Vertically",
            "Perspective",
            "Scale Image…",
            "Canvas Size…",
            "Crop",
            "Apply Crop",
            "Crop to Selection",
            "Crop to Content",
            "menu:Layer",
            "New Layer…",
            "Duplicate Layer",
            "Rename Layer…",
            "Merge Down",
            "Delete Layer",
            "Toggle Layer Visibility",
            "Layer Opacity…",
            "Raise Layer",
            "Lower Layer",
            "menu:Transform",
            "Rotate 90° Left",
            "Rotate 90° Right",
            "Rotate 180°",
            "Flip Horizontally",
            "Flip Vertically",
            "Perspective",
            "menu:Align",
            "Align Top",
            "Align Bottom",
            "Align Left",
            "Align Right",
            "Center Horizontally in Image",
            "Center Vertically in Image",
            "Center in Image",
            "Scale Layer…",
            "Layer to Image Size",
            "Crop to Content",
            "Rasterize Text Layer",
            "menu:Tools",
            "menu:Transform",
            "Move",
            "Crop",
            "Rotate",
            "Flip",
            "Perspective",
            "Scale Layer",
            "menu:Selection",
            "Select",
            "Oval Select",
            "Free Select",
            "Magic Wand",
            "menu:Paint",
            "Pencil",
            "Brush",
            "Eraser",
            "Blur",
            "Sharpen",
            "Clone Stamp",
            "menu:Fill",
            "Bucket Fill",
            "Gradient Fill",
            "menu:Draw",
            "Text",
            "Path",
            "menu:Utilities",
            "Pan",
            "Zoom",
            "Color Picker",
            "menu:Colors",
            "Foreground Color…",
            "Background Color…",
            "Swap Foreground and Background",
            "Default Colors",
            "Brightness & Contrast…",
            "Hue & Saturation…",
            "Color Balance…",
            "Levels…",
            "Curves…",
            "Desaturate…",
            "Invert",
            "menu:Filters",
            "Blur…",
            "Sharpen…",
            "Drop Shadow…",
            "menu:Window",
            "Minimize",
            "Zoom",
            "Next Tab",
            "Previous Tab",
        ];
        assert_eq!(macos, checklist.map(str::to_owned));
        let mut expected_windows: Vec<&str> = checklist
            .into_iter()
            .filter(|label| {
                !matches!(
                    *label,
                    "menu:Lime"
                        | "Services"
                        | "Hide Lime"
                        | "Hide Others"
                        | "Show All"
                        | "Quit Lime"
                )
            })
            .collect();
        let export = expected_windows
            .iter()
            .position(|label| *label == "Export as WebP…")
            .unwrap();
        expected_windows.insert(export + 1, "Exit");
        assert_eq!(
            windows,
            expected_windows
                .iter()
                .copied()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
        assert!(!macos.iter().any(|label| label.contains("Planned")));
    }

    #[test]
    fn finder_claims_projects_and_raster_images() {
        let plist = include_str!("../macos/Info.plist");
        for needle in [
            "com.eimg.lime",
            "com.eimg.lime.project",
            "com.imageedit.project",
            "public.png",
            "public.jpeg",
            "org.webmproject.webp",
            "com.adobe.pdf",
            "public.filename-extension",
            "<string>lime</string>",
            "<string>imageedit</string>",
            "<string>Editor</string>",
            "<string>Owner</string>",
            "<string>Alternate</string>",
        ] {
            assert!(plist.contains(needle), "{needle}");
        }
    }

    #[test]
    fn package_and_bundle_versions_match() {
        let cargo = include_str!("../Cargo.toml");
        let version = cargo
            .lines()
            .find_map(|line| line.strip_prefix("version = \""))
            .expect("Cargo.toml has a package version")
            .trim_end_matches('"');
        let plist = include_str!("../macos/Info.plist").replace("\r\n", "\n");
        assert!(
            plist.contains(&format!(
                "<key>CFBundleShortVersionString</key>\n    <string>{version}</string>"
            )),
            "the macOS bundle version does not match Cargo.toml"
        );
    }

    fn menu_labels(menus: &[Menu]) -> Vec<String> {
        let mut labels = Vec::new();
        for menu in menus {
            labels.push(format!("menu:{}", menu.name));
            collect_items(&menu.items, &mut labels);
        }
        labels
    }

    fn collect_items(items: &[MenuItem], labels: &mut Vec<String>) {
        for item in items {
            match item {
                MenuItem::Separator => {}
                MenuItem::Submenu(menu) => {
                    labels.push(format!("menu:{}", menu.name));
                    collect_items(&menu.items, labels);
                }
                MenuItem::SystemMenu(menu) => labels.push(menu.name.to_string()),
                MenuItem::Action { name, .. } => labels.push(name.to_string()),
            }
        }
    }
}
