//! Selective menu imagery shares the toolbox's SVG glyphs on both platforms.

use anyhow::{Context as _, Result};
use gpui::{Action, AssetSource};

use crate::{commands::*, icons::Icons};

pub(crate) fn for_action(action: &dyn Action) -> Option<&'static str> {
    let action = action.as_any();
    if let Some(SelectTool(tool)) = action.downcast_ref::<SelectTool>() {
        return Some(tool.info().icon);
    }
    if let Some(align) = action.downcast_ref::<AlignLayer>() {
        return Some(match align {
            AlignLayer::Top => "align-top",
            AlignLayer::Bottom => "align-bottom",
            AlignLayer::Left => "align-left",
            AlignLayer::Right => "align-right",
            AlignLayer::Horizontal => "align-h-center",
            AlignLayer::Both => "align-center",
            AlignLayer::Vertical => "align-v-center",
        });
    }
    if let Some(transform) = action.downcast_ref::<ImageAdjust>() {
        return Some(match transform {
            ImageAdjust::RotateLeft => "rotate",
            ImageAdjust::RotateRight => "rotate-right",
            ImageAdjust::RotateHalf => "rotate-half",
            ImageAdjust::FlipHorizontal => "flip",
            ImageAdjust::FlipVertical => "flip-vertical",
        });
    }
    if let Some(transform) = action.downcast_ref::<LayerAdjust>() {
        return Some(match transform {
            LayerAdjust::RotateLeft => "rotate",
            LayerAdjust::RotateRight => "rotate-right",
            LayerAdjust::RotateHalf => "rotate-half",
            LayerAdjust::FlipHorizontal => "flip",
            LayerAdjust::FlipVertical => "flip-vertical",
        });
    }
    macro_rules! icons {
        ($($kind:ty => $icon:literal),* $(,)?) => {
            $(if action.is::<$kind>() { return Some($icon); })*
        };
    }
    icons! {
        NewImage => "new", OpenImage => "folder", OpenAsLayers => "folder",
        SaveImage => "save", SaveImageAs => "save", SaveProjectAs => "save",
        ExportPng => "export", ExportJpeg => "export", ExportWebp => "export",
        Undo => "undo", Redo => "redo", CutImage => "cut", CopyImage => "duplicate",
        PasteImage => "paste", PasteAsLayer => "paste", NewFromClipboard => "paste",
        NewLayer => "plus", DuplicateLayer => "duplicate", DeleteLayer => "trash",
        MergeDown => "merge", RaiseLayer => "arrow-up", LowerLayer => "arrow-down",
        ToggleLayerVisibility => "eye", ToggleAlphaLock => "lock-alpha", TogglePositionLock => "lock-position", ScaleLayer => "scale", ScaleImage => "scale",
        ApplyCrop => "crop", CropToSelection => "crop", CropImageToContent => "crop",
        CropLayerToContent => "crop", StrokePath => "stroke", StrokeSelection => "stroke",
        SwapColors => "swap",
    }
    None
}

/// Straight-alpha RGBA for native icons; geometry stays in the shared SVG source.
pub(crate) fn rasterize(icon: &str, pixels: u32, color: u32) -> Result<Vec<u8>> {
    anyhow::ensure!((1..=256).contains(&pixels), "invalid menu icon size");
    let svg = Icons.load(icon)?.context("unknown menu icon")?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels).context("invalid icon size")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(pixels as f32 / 24., pixels as f32 / 24.),
        &mut pixmap.as_mut(),
    );
    let rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
    Ok(pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| [rgb[0], rgb[1], rgb[2], pixel.alpha()])
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::MenuItem;

    #[test]
    fn menu_icons_render_from_the_shared_assets() {
        fn visit(item: &MenuItem) {
            match item {
                MenuItem::Action { action, .. } => {
                    if let Some(icon) = for_action(action.as_ref()) {
                        for size in [16, 32] {
                            let rgba = rasterize(icon, size, 0xe9edf4).unwrap();
                            assert_eq!(rgba.len(), (size * size * 4) as usize);
                            assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
                        }
                    }
                }
                MenuItem::Submenu(menu) => menu.items.iter().for_each(visit),
                _ => {}
            }
        }
        for menu in application_menus(false) {
            menu.items.iter().for_each(visit);
        }
        assert_eq!(for_action(&Undo), Some("undo"));
        assert_eq!(for_action(&Levels), None);
        assert_eq!(for_action(&Quit), None);
    }
}
