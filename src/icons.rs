use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let (path, stroke) = path
            .strip_prefix("cursor-outline/")
            .map_or((path, "1.6"), |path| (path, "4.0"));
        let drawing = match path {
            "new" => {
                r#"<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9l-6-6Zm0 0v6h6M8 15h8m-4-4v8"/>"#
            }
            "save" => r#"<path d="M4 3h13l4 4v14H3V3h1Zm3 0v6h10V3M7 21v-8h10v8"/>"#,
            "export" => {
                r#"<path d="M14 3h7v7m0-7L10 14M10 5H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-5"/>"#
            }
            "undo" => r#"<path d="M3 5v6h6M3 11l4-4a8 8 0 1 1 0 11"/>"#,
            "redo" => r#"<path d="M21 5v6h-6m6 0-4-4a8 8 0 1 0 0 11"/>"#,
            "cut" => {
                r#"<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="m8 8 12 12M8 16 20 4"/>"#
            }
            "paste" => {
                r#"<path d="M9 5H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-3M9 3h6v4H9zM8 12h8m-8 4h8"/>"#
            }
            "image" => {
                r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="9" r="1.5"/><path d="m3 17 5-5 3 3 4-5 6 7"/>"#
            }
            "crop" => r#"<path d="M7 3v14h14M3 7h14v14"/>"#,
            "hand" => {
                r#"<path d="M8 12V6a1.5 1.5 0 0 1 3 0v4-6a1.5 1.5 0 0 1 3 0v6-5a1.5 1.5 0 0 1 3 0v6-3a1.5 1.5 0 0 1 3 0v7a6 6 0 0 1-6 6h-1a6 6 0 0 1-5-2.7l-4-5.8a1.6 1.6 0 0 1 2.5-2L8 12Z"/>"#
            }
            "zoom" => {
                r#"<circle cx="10" cy="10" r="6"/><path d="m14.5 14.5 5.5 5.5M7 10h6m-3-3v6"/>"#
            }
            "eyedropper" => {
                r#"<path d="M13 7l3.5-3.5a2.83 2.83 0 0 1 4 4L17 11l1.5 1.5-2 2-7-7 2-2L13 7Z"/><path d="m11.5 9.5-7 7L3 21l4.5-1.5 7-7"/>"#
            }
            "eye" => {
                r#"<path d="M2 12s4-6 10-6 10 6 10 6-4 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/>"#
            }
            "eye-off" => {
                r#"<path d="M4 4l16 16M10 6.2A9 9 0 0 1 12 6c6 0 10 6 10 6a17 17 0 0 1-3 3.4M6.5 7.6A17 17 0 0 0 2 12s4 6 10 6a9 9 0 0 0 4-1"/>"#
            }
            "lock-alpha" => {
                r#"<rect x="3" y="3" width="11" height="11" rx="1"/><path d="M3 8.5h11M8.5 3v11"/><path d="M14 15v-3a3 3 0 0 1 6 0v3"/><rect x="12" y="15" width="10" height="7" rx="1"/>"#
            }
            "lock-position" => {
                r#"<path d="M8 10V7a4 4 0 0 1 8 0v3"/><rect x="5" y="10" width="14" height="11" rx="2"/>"#
            }
            "unlock-position" => {
                r#"<path d="M8 10V7a4 4 0 0 1 8 0"/><rect x="5" y="10" width="14" height="11" rx="2"/>"#
            }
            "unlock-alpha" => {
                r#"<rect x="3" y="3" width="11" height="11" rx="1"/><path d="M3 8.5h11M8.5 3v11M14 15v-3a3 3 0 0 1 6 0"/><rect x="12" y="15" width="10" height="7" rx="1"/>"#
            }
            "chevron-right" => r#"<path d="m9 5 7 7-7 7"/>"#,
            "chevron-down" => r#"<path d="m5 9 7 7 7-7"/>"#,
            "duplicate" => {
                r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>"#
            }
            "arrow-up" => r#"<path d="M12 19V5m-6 6 6-6 6 6"/>"#,
            "arrow-down" => r#"<path d="M12 5v14m-6-6 6 6 6-6"/>"#,
            "align-left" => r#"<path d="M4 4v16M20 12H8m4-4-4 4 4 4"/>"#,
            "align-right" => r#"<path d="M20 4v16M4 12h12m-4-4 4 4-4 4"/>"#,
            "align-top" => r#"<path d="M4 4h16M12 20V8m-4 4 4-4 4 4"/>"#,
            "align-bottom" => r#"<path d="M4 20h16M12 4v12m-4-4 4 4 4-4"/>"#,
            "align-h-center" => r#"<path d="M12 4v16M4 8l4 4-4 4M20 8l-4 4 4 4"/>"#,
            "align-v-center" => r#"<path d="M4 12h16M8 4l4 4 4-4M8 20l4-4 4 4"/>"#,
            "merge" => r#"<path d="m12 4 8 4-8 4-8-4 8-4ZM12 15v6m-3-3 3 3 3-3"/>"#,
            "trash" => r#"<path d="M4 7h16M10 11v6m4-6v6M6 7l1 13h10l1-13M9 7V4h6v3"/>"#,
            "text" => r#"<path d="M4 7V4h16v3M12 4v16m-4 0h8"/>"#,
            "gradient" => {
                r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 8v8m4-8v8m4-8v8m3-8v8" opacity=".6"/>"#
            }
            "path" => {
                r#"<path d="M7 17c0-8 10-2 10-10"/><rect x="3" y="17" width="4" height="4" rx="1"/><rect x="17" y="3" width="4" height="4" rx="1"/><path d="M3 12h5m8 0h5"/>"#
            }
            "stroke" => r#"<path d="m4 19 4-1L20 6l-3-3L5 15l-1 4Zm10-13 3 3M4 22h16"/>"#,
            "move" => {
                r#"<path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3m12-6 3 3-3 3"/>"#
            }
            "rotate" => r#"<path d="M4 11a8 8 0 1 1 2 6M4 5v6h6"/>"#,
            "rotate-right" => r#"<path d="M20 11a8 8 0 1 0-2 6m2-12v6h-6"/>"#,
            "rotate-half" => r#"<path d="M4 12a8 8 0 0 1 16 0M4 6v6h6m4 0h6v6"/>"#,
            "flip" => r#"<path d="M12 3v18M9 6l-6 12h6V6Zm6 0 6 12h-6V6Z"/>"#,
            "flip-vertical" => r#"<path d="M3 12h18M6 9l12-6v6H6Zm0 6 12 6v-6H6Z"/>"#,
            "align-center" => {
                r#"<path d="M12 3v4m0 10v4M3 12h4m10 0h4"/><rect x="8" y="8" width="8" height="8" rx="1"/>"#
            }
            "perspective" => {
                r#"<path d="m5 7 13-3 2 16-14-2L5 7Z"/><circle cx="5" cy="7" r="1.4" fill="black"/><circle cx="18" cy="4" r="1.4" fill="black"/><circle cx="20" cy="20" r="1.4" fill="black"/><circle cx="6" cy="18" r="1.4" fill="black"/>"#
            }
            "scale" => r#"<path d="M4 20 20 4M13 4h7v7M11 20H4v-7"/>"#,
            "select" => {
                r#"<rect x="4" y="5" width="16" height="14" rx="1" stroke-dasharray="2.5 2.5"/>"#
            }
            "oval" => r#"<ellipse cx="12" cy="12" rx="8" ry="6" stroke-dasharray="2.5 2.5"/>"#,
            "lasso" => {
                r#"<path d="M8 16c-4 0-6-2-5-5 1-4 7-7 12-6 5 1 7 4 5 7-2 4-10 6-13 3-2-2 1-4 3-2 2 3-3 7-6 6"/>"#
            }
            "wand" => {
                r#"<path d="m4 20 12-12 3 3L7 23l-3-3Zm9-9 3 3M7 3v6M4 6h6m9-5v5m-2.5-2.5h5M19 18v4m-2-2h4"/>"#
            }
            "pencil" => r#"<path d="m4 20 1-5L16 4l4 4L9 19l-5 1ZM13 7l4 4M5 15l4 4"/>"#,
            "brush" => {
                r#"<path d="M12 16 21 7a2 2 0 0 0-3-3l-9 9m3 3-3-3m3 3c0 4-4 5-8 4 2-1 1-5 5-7"/>"#
            }
            "eraser" => {
                r#"<path d="m4 13 9-9a2 2 0 0 1 3 0l4 4a2 2 0 0 1 0 3l-8 9H8l-4-4a2 2 0 0 1 0-3Zm5-5 8 8m-5 4h9"/>"#
            }
            "blur" => {
                r#"<circle cx="12" cy="12" r="9" opacity=".35"/><circle cx="12" cy="12" r="6" opacity=".6"/><circle cx="12" cy="12" r="3"/>"#
            }
            "sharpen" => {
                r#"<path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z"/><path d="M12 8v8m-4-4h8" opacity=".55"/>"#
            }
            "clone" => r#"<path d="M9 3h6v3H9zM8 6h8l1 5H7l1-5ZM5 11h14v3H5zM4 14h16v5H4z"/>"#,
            "bucket" => {
                r#"<path d="m4 11 8-8 7 7-8 8-7-7Zm0 0h14M7 2l6 6m7 6c-1 2-2 3-2 4a2 2 0 0 0 4 0c0-1-1-2-2-4Z"/>"#
            }
            "folder" => {
                r#"<path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>"#
            }
            "close" => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
            "layers" => r#"<path d="m12 3 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 16l9 5 9-5"/>"#,
            "swap" => r#"<path d="M3 8h18m-4-4 4 4-4 4m4 4H3m4-4-4 4 4 4"/>"#,
            "sidebar-close" => {
                r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16m7-4-3-4 3-4"/>"#
            }
            "sidebar-open" => {
                r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16m4-4 3-4-3-4"/>"#
            }
            "plus" => r#"<path d="M12 5v14M5 12h14"/>"#,
            "info" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5m0-8h.01"/>"#,
            "help" => {
                r#"<circle cx="12" cy="12" r="9"/><path d="M9.5 9a2.5 2.5 0 1 1 4 2c-1 .7-1.5 1-1.5 2M12 16h.01"/>"#
            }
            "loading" => r#"<path d="M12 3a9 9 0 1 1-9 9"/>"#,
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{drawing}</svg>"#
        ).into_bytes())))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
