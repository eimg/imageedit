use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let (path, stroke) = path
            .strip_prefix("cursor-outline/")
            .map_or((path, "1.6"), |path| (path, "4.0"));
        let drawing = match path {
            "image" => {
                r#"<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="8.5" cy="9" r="1.5"/><path d="m3 17 5-5 3 3 4-5 6 7"/>"#
            }
            "crop" => r#"<path d="M7 3v14h14M3 7h14v14M17 7l4-4"/>"#,
            "hand" => {
                r#"<path d="M8 12V6a1.5 1.5 0 0 1 3 0v4-6a1.5 1.5 0 0 1 3 0v6-5a1.5 1.5 0 0 1 3 0v6-3a1.5 1.5 0 0 1 3 0v7a6 6 0 0 1-6 6h-1a6 6 0 0 1-5-2.7l-4-5.8a1.6 1.6 0 0 1 2.5-2L8 12Z"/>"#
            }
            "zoom" => {
                r#"<circle cx="10" cy="10" r="6"/><path d="m14.5 14.5 5.5 5.5M7 10h6m-3-3v6"/>"#
            }
            "eyedropper" => {
                r#"<path d="m13 7 4 4m-2-6 2-2a2 2 0 0 1 3 3l-2 2 1 1-3 3-2-2-8 8-4 1 1-4 8-8-2-2 3-3 3 3Z"/>"#
            }
            "eye" => {
                r#"<path d="M2 12s4-6 10-6 10 6 10 6-4 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/>"#
            }
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
            "flip" => r#"<path d="M12 3v18M9 6l-6 12h6V6Zm6 0 6 12h-6V6Z"/>"#,
            "perspective" => {
                r#"<path d="m5 7 13-3 2 16-14-2L5 7Z"/><circle cx="5" cy="7" r="1.4" fill="black"/><circle cx="18" cy="4" r="1.4" fill="black"/><circle cx="20" cy="20" r="1.4" fill="black"/><circle cx="6" cy="18" r="1.4" fill="black"/>"#
            }
            "select" => {
                r#"<rect x="4" y="5" width="16" height="14" rx="1" stroke-dasharray="2.5 2.5"/>"#
            }
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
            "bucket" => {
                r#"<path d="m4 11 8-8 7 7-8 8-7-7Zm0 0h14M7 2l6 6m7 6c-1 2-2 3-2 4a2 2 0 0 0 4 0c0-1-1-2-2-4Z"/>"#
            }
            "folder" => {
                r#"<path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>"#
            }
            "close" => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
            "layers" => r#"<path d="m12 3 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 16l9 5 9-5"/>"#,
            "swap" => r#"<path d="M3 8h18m-4-4 4 4-4 4m4 4H3m4-4-4 4 4 4"/>"#,
            "plus" => r#"<path d="M12 5v14M5 12h14"/>"#,
            "info" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5m0-8h.01"/>"#,
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
