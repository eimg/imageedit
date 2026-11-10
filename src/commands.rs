use gpui::{Action, App, KeyBinding, Menu, MenuItem, SystemMenuType, actions};

actions!(
    imageedit,
    [
        OpenImage,
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
                "Reposition a layer or selection on the canvas.",
            ),
            Self::Pan => ("Pan", "hand", "Drag the canvas to navigate a zoomed image."),
            Self::Eyedropper => (
                "Eyedropper",
                "eyedropper",
                "Click the image preview to set the foreground color.",
            ),
            Self::Crop => (
                "Crop",
                "crop",
                "Trim the edges of an image or choose a new aspect ratio.",
            ),
            Self::Rotate => ("Rotate", "rotate", "Turn a layer or selection by an angle."),
            Self::Flip => (
                "Flip",
                "flip",
                "Mirror a layer or selection horizontally or vertically.",
            ),
            Self::Perspective => (
                "Perspective",
                "perspective",
                "Drag the corners to correct or change perspective.",
            ),
            Self::Select => (
                "Select",
                "select",
                "Drag a rectangle to select part of the image.",
            ),
            Self::FreeSelect => (
                "Free Select",
                "lasso",
                "Trace a freehand or polygonal selection.",
            ),
            Self::MagicWand => (
                "Magic Wand",
                "wand",
                "Select a connected region of similar colors.",
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
                "Fill a selection or a region of similar color.",
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
    ("Utilities", &[Tool::Pan, Tool::Eyedropper]),
];

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = imageedit, no_json)]
pub struct SelectTool(pub Tool);

#[derive(Clone, Copy, Debug, Default, PartialEq, Action)]
#[action(namespace = imageedit, no_json)]
pub enum Zoom {
    #[default]
    Fit,
    Percent(u16),
}

pub const EXPORT_PNG: &str = "Export as PNG…";
pub const EXPORT_JPEG: &str = "Export as JPEG…";
pub const ADD_LAYER: &str = "New Layer…";

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
        KeyBinding::new(&format!("{primary}-o"), OpenImage, None),
        KeyBinding::new(&format!("{primary}-w"), CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, None),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
        ]);
    }

    // Window actions run while GPUI has the active window borrowed.
    cx.on_action(|_: &OpenImage, cx| cx.defer(crate::open_dialog));
    cx.on_action(|_: &Quit, cx| cx.quit());
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
                planned("New Image…"),
                planned("New from Clipboard"),
                MenuItem::separator(),
                MenuItem::action("Open Image…", OpenImage),
                planned("Open as Layers…"),
                MenuItem::action("Close Tab", CloseTab),
                MenuItem::separator(),
                planned("Save Project"),
                planned("Save Project As…"),
                MenuItem::separator(),
                planned(EXPORT_PNG),
                planned(EXPORT_JPEG),
            ],
        ),
        menu(
            "Edit",
            vec![
                planned("Undo"),
                planned("Redo"),
                MenuItem::separator(),
                planned("Cut"),
                planned("Copy"),
                planned("Paste"),
                planned("Paste as New Layer"),
                planned("Clear to Transparency"),
            ],
        ),
        menu(
            "Select",
            vec![
                planned("Select All"),
                planned("Select None"),
                planned("Invert Selection"),
                MenuItem::separator(),
                planned("Feather…"),
                planned("Grow…"),
                planned("Shrink…"),
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
                MenuItem::submenu(menu("Mode", vec![planned("RGB"), planned("Grayscale")])),
                transform_menu(),
                MenuItem::separator(),
                planned("Scale Image…"),
                planned("Canvas Size…"),
                tool(Tool::Crop),
                planned("Crop to Content"),
            ],
        ),
        menu(
            "Layer",
            vec![
                planned(ADD_LAYER),
                planned("Duplicate Layer"),
                planned("Rename Layer…"),
                planned("Merge Down"),
                planned("Delete Layer"),
                MenuItem::separator(),
                planned("Toggle Layer Visibility"),
                planned("Layer Opacity…"),
                planned("Raise Layer"),
                planned("Lower Layer"),
                MenuItem::separator(),
                transform_menu(),
                MenuItem::submenu(menu(
                    "Align",
                    vec![
                        planned("Center Horizontally in Image"),
                        planned("Center Vertically in Image"),
                        planned("Center in Image"),
                    ],
                )),
                planned("Scale Layer…"),
                planned("Layer to Image Size"),
                planned("Crop to Content"),
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

fn transform_menu() -> MenuItem {
    MenuItem::submenu(menu(
        "Transform",
        vec![
            planned("Rotate 90° Left"),
            planned("Rotate 90° Right"),
            planned("Rotate 180°"),
            MenuItem::separator(),
            planned("Flip Horizontally"),
            planned("Flip Vertically"),
            MenuItem::separator(),
            tool(Tool::Perspective),
        ],
    ))
}

fn zoom_menu() -> MenuItem {
    let mut items = vec![
        MenuItem::action("Fit Image in Window", Zoom::Fit),
        MenuItem::separator(),
    ];
    items.extend(
        [25, 50, 100, 200, 400]
            .map(|percent| MenuItem::action(format!("{percent}%"), Zoom::Percent(percent))),
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
