use gpui::{Action, App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

actions!(
    imageedit,
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
        ScaleImage,
        ScaleLayer,
        CanvasSize,
        SetRgb,
        SetGrayscale,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleGrid,
        ToggleGuides,
        MinimizeWindow,
        ZoomWindow,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
        Planned
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
    Select,
    FreeSelect,
    MagicWand,
    Pencil,
    Brush,
    Eraser,
    BucketFill,
    Gradient,
    Text,
    Path,
    Stroke,
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
            Self::Pan => ("Pan", "hand", "Drag the canvas to navigate a zoomed image."),
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
            Self::Select => (
                "Select",
                "select",
                "Drag a rectangle. Shift adds, Control subtracts, and Shift-Control intersects.",
            ),
            Self::FreeSelect => (
                "Free Select",
                "lasso",
                "Draw a freehand outline or click a polygon. Escape cancels.",
            ),
            Self::MagicWand => (
                "Magic Wand",
                "wand",
                "Select a connected region. Tolerance 0 matches the color exactly.",
            ),
            Self::Pencil => (
                "Pencil",
                "pencil",
                "Draw hard-edged marks with the foreground color.",
            ),
            Self::Brush => (
                "Brush",
                "brush",
                "Paint with an adjustable brush and the foreground color.",
            ),
            Self::Eraser => ("Eraser", "eraser", "Remove pixels from a layer."),
            Self::BucketFill => (
                "Bucket Fill",
                "bucket",
                "Fill a connected region. The tolerance is shared with the Magic Wand.",
            ),
            Self::Gradient => (
                "Gradient Fill",
                "gradient",
                "Fill an area with a transition between colors.",
            ),
            Self::Text => (
                "Text",
                "text",
                "Add and style editable text on its own layer.",
            ),
            Self::Path => (
                "Path",
                "path",
                "Draw and edit shapes with curves and anchor points.",
            ),
            Self::Stroke => (
                "Stroke",
                "stroke",
                "Draw a line along a path or the edge of a selection.",
            ),
        };
        ToolInfo {
            label,
            icon,
            description,
        }
    }
}

const TRANSFORM_TOOLS: &[Tool] = &[
    Tool::Move,
    Tool::Crop,
    Tool::Rotate,
    Tool::Flip,
    Tool::Perspective,
];
const SELECTION_TOOLS: &[Tool] = &[Tool::Select, Tool::FreeSelect, Tool::MagicWand];
const PAINT_TOOLS: &[Tool] = &[Tool::Pencil, Tool::Brush, Tool::Eraser];
const FILL_TOOLS: &[Tool] = &[Tool::BucketFill, Tool::Gradient];
const DRAW_TOOLS: &[Tool] = &[Tool::Text, Tool::Path, Tool::Stroke];

pub const TOOL_GROUPS: &[(&str, &[Tool])] = &[
    ("Transform", TRANSFORM_TOOLS),
    ("Selection", SELECTION_TOOLS),
    ("Paint", PAINT_TOOLS),
    ("Fill", FILL_TOOLS),
    ("Draw", DRAW_TOOLS),
    ("Utilities", &[Tool::Pan, Tool::Zoom, Tool::Eyedropper]),
];

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = imageedit, no_json)]
pub struct SelectTool(pub Tool);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = imageedit, no_json)]
pub enum AlignLayer {
    Horizontal,
    Vertical,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = imageedit, no_json)]
pub enum ImageAdjust {
    RotateLeft,
    RotateRight,
    RotateHalf,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Action)]
#[action(namespace = imageedit, no_json)]
pub enum LayerAdjust {
    RotateLeft,
    RotateRight,
    RotateHalf,
    FlipHorizontal,
    FlipVertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Action)]
#[action(namespace = imageedit, no_json)]
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
        KeyBinding::new("r", SelectTool(Tool::Select), None),
        KeyBinding::new("f", SelectTool(Tool::FreeSelect), None),
        KeyBinding::new("u", SelectTool(Tool::MagicWand), None),
        KeyBinding::new("m", SelectTool(Tool::Move), None),
        KeyBinding::new("escape", CancelMove, Some("Move")),
        KeyBinding::new("escape", CancelFreeSelect, Some("FreeSelect")),
        KeyBinding::new("enter", CloseFreeSelect, Some("FreeSelect")),
        KeyBinding::new(&format!("{primary}-c"), CopyImage, None),
        KeyBinding::new(&format!("{primary}-x"), CutImage, None),
        KeyBinding::new(&format!("{primary}-v"), PasteImage, None),
        KeyBinding::new(&format!("{primary}-shift-v"), PasteAsLayer, None),
        KeyBinding::new(&format!("{primary}-a"), SelectAll, None),
        KeyBinding::new(&format!("{primary}-shift-a"), SelectNone, None),
        KeyBinding::new(&format!("{primary}-i"), InvertSelection, None),
        KeyBinding::new("shift-r", SelectTool(Tool::Rotate), None),
        KeyBinding::new("shift-f", SelectTool(Tool::Flip), None),
        KeyBinding::new("shift-p", SelectTool(Tool::Perspective), None),
        KeyBinding::new("shift-c", SelectTool(Tool::Crop), None),
        KeyBinding::new("enter", ApplyTransform, Some("Transform")),
        KeyBinding::new("escape", CancelTransform, Some("Transform")),
        KeyBinding::new("shift-b", SelectTool(Tool::BucketFill), None),
        KeyBinding::new("o", SelectTool(Tool::Eyedropper), None),
        KeyBinding::new("z", SelectTool(Tool::Zoom), None),
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
        ]);
    }

    // Window actions run while GPUI has the active window borrowed.
    cx.on_action(|_: &OpenImage, cx| cx.defer(crate::open_dialog));
    cx.on_action(|_: &Quit, cx| cx.defer(crate::quit));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

    cx.set_menus(vec![
        menu(
            "ImageEdit",
            vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide ImageEdit", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit ImageEdit", Quit),
            ],
        ),
        menu(
            "File",
            vec![
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
            ],
        ),
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
            ],
        ),
        menu(
            "Select",
            vec![
                MenuItem::action("Rectangle Select", SelectTool(Tool::Select)),
                MenuItem::separator(),
                MenuItem::action("Select All", SelectAll),
                MenuItem::action("Select None", SelectNone),
                MenuItem::action("Invert Selection", InvertSelection),
                MenuItem::separator(),
                MenuItem::action("Feather…", FeatherSelection),
                MenuItem::action("Grow…", GrowSelection),
                MenuItem::action("Shrink…", ShrinkSelection),
            ],
        ),
        menu(
            "View",
            vec![
                zoom_menu(),
                tool(Tool::Pan),
                MenuItem::separator(),
                MenuItem::action("Toggle Grid", ToggleGrid),
                MenuItem::action("Toggle Guides", ToggleGuides),
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
                        MenuItem::action("Center Horizontally in Image", AlignLayer::Horizontal),
                        MenuItem::action("Center Vertically in Image", AlignLayer::Vertical),
                        MenuItem::action("Center in Image", AlignLayer::Both),
                    ],
                )),
                MenuItem::action("Scale Layer…", ScaleLayer),
                MenuItem::action("Layer to Image Size", LayerToImageSize),
                MenuItem::action("Crop to Content", CropLayerToContent),
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
                planned("Brightness & Contrast…"),
                planned("Hue & Saturation…"),
                planned("Color Balance…"),
                planned("Levels…"),
                planned("Curves…"),
                MenuItem::separator(),
                planned("Desaturate…"),
                planned("Invert"),
            ],
        ),
        menu(
            "Filters",
            vec![
                planned("Drop Shadow…"),
                planned("Blur…"),
                planned("Sharpen…"),
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

fn planned(label: &'static str) -> MenuItem {
    MenuItem::action(label, Planned)
}
