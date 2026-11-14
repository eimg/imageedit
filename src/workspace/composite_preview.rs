//! Background compositing for previews whose blend modes depend on the backdrop.
//! The normal-only stack retains its existing immediate GPU sprite path.
use super::{Workspace, layers::MovePaint};
use crate::document::{self, BlendMode, DocumentContent, LayerId};
use gpui::{Context, Window};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    tab: usize,
    generation: u64,
    sprite: gpui::ImageId,
    offset: (i32, i32),
    size: (u32, u32),
    target: Option<LayerId>,
    linked_move: bool,
}

#[derive(Default)]
pub(super) struct CompositePreview {
    requested: Option<Key>,
    published: Option<(Key, MovePaint)>,
    busy: bool,
}

impl Workspace {
    fn linked_move_preview(&self, raw: &MovePaint) -> bool {
        self.move_overlay
            .as_ref()
            .is_some_and(|paint| Arc::ptr_eq(&paint.sprite, &raw.sprite))
            && self.active_tab().is_some_and(|tab| {
                !tab.document
                    .content
                    .linked_layers(tab.active_layer)
                    .is_empty()
            })
    }

    pub(super) fn move_paint(&self) -> Option<MovePaint> {
        let raw = self.raw_move_paint()?;
        if self
            .active_tab()
            .is_some_and(|tab| tab.document.content.needs_composite_preview())
            || self.linked_move_preview(&raw)
        {
            self.composite_preview
                .published
                .as_ref()
                .map(|(_, paint)| paint.clone())
        } else {
            Some(raw)
        }
    }

    pub(super) fn refresh_composite_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.raw_move_paint();
        let input = self
            .active_tab()
            .zip(raw)
            .filter(|(tab, raw)| {
                tab.document.content.needs_composite_preview() || self.linked_move_preview(raw)
            })
            .map(|(tab, raw)| {
                let target = if self
                    .text_overlay
                    .as_ref()
                    .is_some_and(|text| Arc::ptr_eq(&text.sprite, &raw.sprite))
                {
                    self.text_preview_layer().unwrap_or(Some(tab.active_layer))
                } else {
                    Some(tab.active_layer)
                };
                let key = Key {
                    tab: tab.id,
                    generation: tab.history.generation(),
                    sprite: raw.sprite.id,
                    offset: raw.offset,
                    size: raw.size,
                    target,
                    linked_move: self.linked_move_preview(&raw),
                };
                (key, raw, tab.document.content.clone(), tab.active_layer)
            });
        let Some((key, raw, content, above)) = input else {
            self.composite_preview.requested = None;
            if let Some((_, paint)) = self.composite_preview.published.take() {
                super::drop_images(paint.into_images(), window, cx);
            }
            return;
        };
        self.composite_preview.requested = Some(key);
        // Never display a previous tab's preview or a result from older content.
        if self
            .composite_preview
            .published
            .as_ref()
            .is_some_and(|(old, _)| old.tab != key.tab || old.generation != key.generation)
        {
            let (_, paint) = self.composite_preview.published.take().unwrap();
            super::drop_images(paint.into_images(), window, cx);
        }
        if self.composite_preview.busy
            || self
                .composite_preview
                .published
                .as_ref()
                .is_some_and(|(old, _)| *old == key)
        {
            return;
        }
        self.composite_preview.busy = true;
        let mask_baked = self
            .warp_overlay
            .as_ref()
            .is_some_and(|warp| Arc::ptr_eq(&warp.sprite, &raw.sprite));
        let job = cx.background_executor().spawn(async move {
            let image = composite(
                &content,
                &raw,
                key.target,
                Some(above),
                1024,
                mask_baked,
                key.linked_move,
            );
            MovePaint {
                below: None,
                above: None,
                sprite: document::preview_render(&image),
                offset: (0, 0),
                size: content.size(),
            }
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let paint = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.composite_preview.busy = false;
                if workspace.composite_preview.requested == Some(key) {
                    if let Some((_, old)) =
                        workspace.composite_preview.published.replace((key, paint))
                    {
                        super::drop_images(old.into_images(), window, cx);
                    }
                } else {
                    super::drop_images(paint.into_images(), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn composite(
    content: &DocumentContent,
    paint: &MovePaint,
    mut target: Option<LayerId>,
    mut above: Option<LayerId>,
    edge: u32,
    mask_baked: bool,
    linked_move: bool,
) -> RgbaImage {
    // Translate the immutable snapshot, preserving stack order, masks, opacity,
    // and backdrop-dependent blending even across intervening unlinked layers.
    let mut moved;
    let content = if linked_move {
        moved = content.clone();
        if let Some(id) = target {
            moved.move_linked_layers(id, paint.offset);
        }
        target = None;
        above = None;
        &moved
    } else {
        content
    };
    let (width, height) = content.size();
    let scale = |value: u32| {
        ((u64::from(value) * u64::from(edge.min(width.max(height))) / u64::from(width.max(height)))
            as u32)
            .max(1)
    };
    let mut output = RgbaImage::new(scale(width), scale(height));
    let size = paint.sprite.size(0);
    let sprite_size = (u32::from(size.width), u32::from(size.height));
    let bytes = paint.sprite.as_bytes(0).unwrap_or_default();
    let sprite_at = |cx: i64, cy: i64| -> Option<Rgba<u8>> {
        let (x, y) = (
            cx - i64::from(paint.offset.0),
            cy - i64::from(paint.offset.1),
        );
        if x < 0 || y < 0 || x >= i64::from(paint.size.0) || y >= i64::from(paint.size.1) {
            return None;
        }
        let sx =
            ((x as u64 * 2 + 1) * u64::from(sprite_size.0) / (2 * u64::from(paint.size.0))) as u32;
        let sy =
            ((y as u64 * 2 + 1) * u64::from(sprite_size.1) / (2 * u64::from(paint.size.1))) as u32;
        let index = ((u64::from(sy) * u64::from(sprite_size.0) + u64::from(sx)) * 4) as usize;
        bytes
            .get(index..index + 4)
            .map(|pixel| Rgba([pixel[2], pixel[1], pixel[0], pixel[3]]))
    };
    let (ow, oh) = output.dimensions();
    for (x, y, out) in output.enumerate_pixels_mut() {
        let cx = ((u64::from(x) * 2 + 1) * u64::from(width) / (u64::from(ow) * 2))
            .min(u64::from(width - 1)) as i64;
        let cy = ((u64::from(y) * 2 + 1) * u64::from(height) / (u64::from(oh) * 2))
            .min(u64::from(height - 1)) as i64;
        for layer in content.layers() {
            if layer.visible && layer.opacity != 0 {
                if target == Some(layer.id) {
                    if let Some(mut pixel) = sprite_at(cx, cy) {
                        if !mask_baked {
                            let sx = (((cx - i64::from(paint.offset.0)) as u64 * 2 + 1)
                                * u64::from(layer.pixels.width())
                                / (2 * u64::from(paint.size.0)))
                                as u32;
                            let sy = (((cy - i64::from(paint.offset.1)) as u64 * 2 + 1)
                                * u64::from(layer.pixels.height())
                                / (2 * u64::from(paint.size.1)))
                                as u32;
                            pixel = layer.masked(pixel, sx, sy);
                        }
                        *out = document::blend(*out, pixel, 255, layer.blend_mode);
                    }
                } else {
                    let (sx, sy) = (
                        cx - i64::from(layer.offset.0),
                        cy - i64::from(layer.offset.1),
                    );
                    if sx >= 0
                        && sy >= 0
                        && sx < i64::from(layer.pixels.width())
                        && sy < i64::from(layer.pixels.height())
                    {
                        *out = document::blend(
                            *out,
                            layer.masked(
                                *layer.pixels.get_pixel(sx as u32, sy as u32),
                                sx as u32,
                                sy as u32,
                            ),
                            layer.opacity,
                            layer.blend_mode,
                        );
                    }
                }
            }
            if target.is_none()
                && Some(layer.id) == above
                && let Some(pixel) = sprite_at(cx, cy)
            {
                *out = document::blend(*out, pixel, 255, BlendMode::Normal);
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linked_move_preview_preserves_interleaved_stack_masks_and_blend_modes() {
        for mode in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
        ] {
            let mut content = DocumentContent::single_layer(
                "Back".into(),
                RgbaImage::from_pixel(7, 5, Rgba([50, 100, 180, 120])),
            );
            let back = content.top_layer_id();
            let middle = content
                .add_layer(back, "Stationary".into(), Rgba([90, 30, 60, 190]))
                .unwrap();
            let top = content
                .add_layer(middle, "Source".into(), Rgba([210, 60, 20, 180]))
                .unwrap();
            content.mask_from_selection(
                top,
                &crate::selection::Selection::from_coverage(7, 5, |x, _| (x * 40) as u8),
            );
            content.change_blend_mode(top, mode);
            content.set_opacity(top, 160);
            content.link_layers(back, top);
            let paint = MovePaint {
                below: None,
                above: None,
                sprite: document::preview_render(&RgbaImage::new(1, 1)),
                offset: (-2, 1),
                size: (7, 5),
            };
            let preview = composite(&content, &paint, Some(top), Some(back), 1024, false, true);
            let mut committed = content.clone();
            committed.move_linked_layers(top, paint.offset);
            assert_eq!(preview, *committed.composite());
            assert_eq!(content.layer(top).unwrap().offset, (0, 0));
            assert_eq!(committed.layer(middle).unwrap().offset, (0, 0));
            content.set_visible(back, false);
            committed.set_visible(back, false);
            assert_eq!(
                composite(&content, &paint, Some(top), Some(back), 1024, false, true),
                *committed.composite()
            );
        }
    }
    #[test]
    fn backdrop_dependent_move_and_scale_previews_match_full_composition() {
        for mode in [BlendMode::Multiply, BlendMode::Screen, BlendMode::Overlay] {
            let mut content = DocumentContent::single_layer(
                "Base".into(),
                RgbaImage::from_pixel(6, 4, Rgba([80, 120, 180, 200])),
            );
            let base = content.top_layer_id();
            let id = content
                .add_layer(base, "Source".into(), Rgba([210, 80, 40, 180]))
                .unwrap();
            content.mask_from_selection(
                id,
                &crate::selection::Selection::from_coverage(6, 4, |x, _| (x * 40) as u8),
            );
            content.change_blend_mode(id, mode);
            let top = content
                .add_layer(id, "Top".into(), Rgba([30, 180, 70, 70]))
                .unwrap();
            content.change_blend_mode(top, BlendMode::Screen);
            let sprite = document::preview_render(&content.layer(id).unwrap().pixels);
            let paint = MovePaint {
                below: None,
                above: None,
                sprite,
                offset: (-2, 1),
                size: (6, 4),
            };
            let preview = composite(&content, &paint, Some(id), Some(base), 1024, false, false);
            content.set_offset(id, paint.offset);
            assert_eq!(preview, *content.composite());
            content.scale_layer(id, 3, 2, crate::transform::Resample::Nearest);
            content.set_offset(id, (1, 1));
            let paint = MovePaint {
                offset: (1, 1),
                size: (3, 2),
                ..paint
            };
            // Reconstruct from the unscaled input, matching the GPU scale frame.
            let mut original = content.clone();
            let layers = original.layers().to_vec();
            let mut restored = layers;
            restored
                .iter_mut()
                .find(|layer| layer.id == id)
                .unwrap()
                .pixels = Arc::new(RgbaImage::from_pixel(6, 4, Rgba([210, 80, 40, 180])));
            restored
                .iter_mut()
                .find(|layer| layer.id == id)
                .unwrap()
                .mask
                .as_mut()
                .unwrap()
                .pixels = Arc::new(RgbaImage::from_fn(6, 4, |x, _| {
                Rgba([255, 255, 255, (x * 40) as u8])
            }));
            original = DocumentContent::from_layers(6, 4, restored).unwrap();
            assert_eq!(
                composite(&original, &paint, Some(id), Some(base), 1024, false, false),
                *content.composite()
            );
        }
    }
}
