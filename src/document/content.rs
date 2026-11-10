use std::{collections::HashSet, sync::Arc};

use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage, imageops};

use super::{MAX_IMAGE_BYTES, composite};
use crate::preview::{Dirty, PixelRect};

pub const IMPORTED_LAYER_NAME: &str = "Image";
pub const MAX_LAYERS: usize = 128;
// Keeps a full stack's names within the project manifest limit.
pub const MAX_LAYER_NAME_CHARS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
}

#[derive(Clone)]
pub struct RasterLayer {
    pub id: LayerId,
    pub name: String,
    pub pixels: Arc<RgbaImage>,
    pub offset: (i32, i32),
    pub visible: bool,
    pub opacity: u8,
    pub blend_mode: BlendMode,
}

impl RasterLayer {
    pub(super) fn bounds(&self) -> PixelBounds {
        let (x, y) = self.offset;
        let (left, top) = (i64::from(x), i64::from(y));
        PixelBounds {
            left,
            top,
            right: left + i64::from(self.pixels.width()),
            bottom: top + i64::from(self.pixels.height()),
        }
    }
}

#[derive(Clone)]
pub struct DocumentContent {
    pub(super) width: u32,
    pub(super) height: u32,
    // Bottom to top. Constructors and edits keep at least one layer.
    pub(super) layers: Vec<RasterLayer>,
}

impl DocumentContent {
    pub fn single_layer(name: String, pixels: RgbaImage) -> Self {
        let (width, height) = pixels.dimensions();
        Self {
            width,
            height,
            layers: vec![RasterLayer {
                id: LayerId(0),
                name,
                pixels: Arc::new(pixels),
                offset: (0, 0),
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
            }],
        }
    }

    pub fn from_layers(width: u32, height: u32, layers: Vec<RasterLayer>) -> Result<Self> {
        ensure!(width > 0 && height > 0, "The canvas must not be empty");
        ensure!(!layers.is_empty(), "A document needs at least one layer");
        let mut ids = HashSet::with_capacity(layers.len());
        for layer in &layers {
            ensure!(
                layer.pixels.width() > 0 && layer.pixels.height() > 0,
                "Layer \"{}\" has no pixels",
                layer.name
            );
            ensure!(ids.insert(layer.id), "Layer ID {} is repeated", layer.id.0);
        }
        Ok(Self {
            width,
            height,
            layers,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn layers(&self) -> &[RasterLayer] {
        &self.layers
    }

    pub fn layer_dirty(&self, id: LayerId) -> Dirty {
        let Some(layer) = self.layer(id) else {
            return Dirty::Canvas;
        };
        Dirty::Region(PixelRect::new(
            i64::from(layer.offset.0),
            i64::from(layer.offset.1),
            layer.pixels.width(),
            layer.pixels.height(),
        ))
    }

    pub fn layer(&self, id: LayerId) -> Option<&RasterLayer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn top_layer_id(&self) -> LayerId {
        self.layers
            .last()
            .expect("documents have at least one layer")
            .id
    }

    // Prefer the top visible layer so tools edit something the user can see.
    pub fn preferred_active_layer(&self) -> LayerId {
        self.layers
            .iter()
            .rev()
            .find(|layer| layer.visible)
            .map(|layer| layer.id)
            .unwrap_or_else(|| self.top_layer_id())
    }

    pub(super) fn full_canvas_layer(&self) -> Option<&RasterLayer> {
        let [layer] = self.layers.as_slice() else {
            return None;
        };
        (layer.visible
            && layer.opacity == 255
            && layer.blend_mode == BlendMode::Normal
            && layer.offset == (0, 0)
            && layer.pixels.dimensions() == (self.width, self.height))
            .then_some(layer)
    }

    // One full-canvas layer with default properties and its import name.
    pub fn is_plain_raster(&self) -> bool {
        self.full_canvas_layer()
            .is_some_and(|layer| layer.name == IMPORTED_LAYER_NAME)
    }

    pub fn composite(&self) -> Arc<RgbaImage> {
        if let Some(layer) = self.full_canvas_layer() {
            return layer.pixels.clone();
        }
        Arc::new(composite::flatten(self))
    }

    // Composites a canvas-space rectangle. Coordinates may extend outside the canvas.
    pub(crate) fn composite_rect(&self, x: i64, y: i64, width: u32, height: u32) -> RgbaImage {
        let bounds = PixelBounds {
            left: x,
            top: y,
            right: x.saturating_add(i64::from(width)),
            bottom: y.saturating_add(i64::from(height)),
        };
        composite::flatten_region(&self.layers, bounds)
    }

    pub fn crop(&mut self, crop: CropRect) -> bool {
        let Some(bounds) = crop.bounds() else {
            return false;
        };
        if crop.width == 0
            || crop.height == 0
            || bounds.right > i64::from(self.width)
            || bounds.bottom > i64::from(self.height)
            || (crop.width, crop.height) == (self.width, self.height)
        {
            return false;
        }

        let mut fallback = self.layers[0].clone();
        self.layers.retain_mut(|layer| {
            let original = layer.bounds();
            let Some(clipped) = original.intersect(bounds) else {
                return false;
            };
            if clipped != original {
                let (x, y) = original.local_origin(clipped);
                let (width, height) = clipped.dimensions();
                layer.pixels =
                    Arc::new(imageops::crop_imm(&*layer.pixels, x, y, width, height).to_image());
            }
            let (x, y) = bounds.local_origin(clipped);
            // Rebasing a clipped layer keeps offsets within the bounded canvas.
            layer.offset = (x as i32, y as i32);
            true
        });
        if self.layers.is_empty() {
            fallback.offset = (0, 0);
            fallback.pixels = Arc::new(RgbaImage::new(crop.width, crop.height));
            self.layers.push(fallback);
        }
        (self.width, self.height) = (crop.width, crop.height);
        true
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id == id)
    }

    fn next_layer_id(&self) -> Option<LayerId> {
        let max = self.layers.iter().map(|layer| layer.id.0).max()?;
        max.checked_add(1).map(LayerId)
    }

    pub fn can_add_layer(&self) -> bool {
        self.layers.len() < MAX_LAYERS
    }

    // Inserts a canvas-sized layer above `above`, or at the top if it is missing.
    pub fn add_layer(&mut self, above: LayerId, name: String, fill: Rgba<u8>) -> Option<LayerId> {
        if !self.can_add_layer() {
            return None;
        }
        let id = self.next_layer_id()?;
        let index = self
            .layer_index(above)
            .map_or(self.layers.len(), |index| index + 1);
        self.layers.insert(
            index,
            RasterLayer {
                id,
                name,
                pixels: Arc::new(RgbaImage::from_pixel(self.width, self.height, fill)),
                offset: (0, 0),
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
            },
        );
        Some(id)
    }

    pub fn duplicate_layer(&mut self, id: LayerId) -> Option<LayerId> {
        if !self.can_add_layer() {
            return None;
        }
        let index = self.layer_index(id)?;
        let mut copy = self.layers[index].clone();
        copy.id = self.next_layer_id()?;
        copy.name = copy_name(&copy.name);
        let copy_id = copy.id;
        self.layers.insert(index + 1, copy);
        Some(copy_id)
    }

    pub fn rename_layer(&mut self, id: LayerId, name: String) -> bool {
        match self.layers.iter_mut().find(|layer| layer.id == id) {
            Some(layer) if layer.name != name => {
                layer.name = name;
                true
            }
            _ => false,
        }
    }

    // Returns the layer that takes over: the one below, or else the one above.
    pub fn delete_layer(&mut self, id: LayerId) -> Option<LayerId> {
        if self.layers.len() < 2 {
            return None;
        }
        let index = self.layer_index(id)?;
        self.layers.remove(index);
        Some(self.layers[index.saturating_sub(1)].id)
    }

    pub fn set_visible(&mut self, id: LayerId, visible: bool) -> bool {
        match self.layers.iter_mut().find(|layer| layer.id == id) {
            Some(layer) if layer.visible != visible => {
                layer.visible = visible;
                true
            }
            _ => false,
        }
    }

    pub fn set_opacity(&mut self, id: LayerId, opacity: u8) -> bool {
        match self.layers.iter_mut().find(|layer| layer.id == id) {
            Some(layer) if layer.opacity != opacity => {
                layer.opacity = opacity;
                true
            }
            _ => false,
        }
    }

    // Moves a layer to `index` in the bottom-to-top stack.
    pub fn move_layer(&mut self, id: LayerId, index: usize) -> bool {
        let Some(from) = self.layer_index(id) else {
            return false;
        };
        if from == index || index >= self.layers.len() {
            return false;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(index, layer);
        true
    }

    pub fn can_merge_down(&self, id: LayerId) -> bool {
        self.merge_bounds(id).is_some()
    }

    fn merge_bounds(&self, id: LayerId) -> Option<(usize, PixelBounds)> {
        let index = self.layer_index(id)?.checked_sub(1)?;
        let [lower, upper] = [&self.layers[index], &self.layers[index + 1]];
        if !lower.visible || !upper.visible {
            return None;
        }
        let union = lower.bounds().union(upper.bounds());
        let (width, height) = (union.right - union.left, union.bottom - union.top);
        let bytes = u64::try_from(width)
            .ok()?
            .checked_mul(u64::try_from(height).ok()?)?;
        (bytes.checked_mul(4)? <= MAX_IMAGE_BYTES).then_some((index, union))
    }

    // Replaces a layer and the one below it with their composite, keeping the lower ID and name.
    pub fn merge_down(&mut self, id: LayerId) -> Option<LayerId> {
        let (index, union) = self.merge_bounds(id)?;
        let pixels = composite::flatten_region(&self.layers[index..=index + 1], union);
        self.layers.remove(index + 1);
        let merged = &mut self.layers[index];
        merged.pixels = Arc::new(pixels);
        // Both bounds start at i32 offsets, so their minimum does too.
        merged.offset = (union.left as i32, union.top as i32);
        merged.visible = true;
        merged.opacity = 255;
        merged.blend_mode = BlendMode::Normal;
        Some(merged.id)
    }

    // The rectangle covers every pixel the fill changed, in canvas coordinates.
    pub fn fill(&mut self, id: LayerId, x: u32, y: u32, color: Rgba<u8>) -> Option<PixelRect> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let layer = self.layers.iter_mut().find(|layer| layer.id == id)?;
        let local_x = i64::from(x) - i64::from(layer.offset.0);
        let local_y = i64::from(y) - i64::from(layer.offset.1);
        if !(0..i64::from(layer.pixels.width())).contains(&local_x)
            || !(0..i64::from(layer.pixels.height())).contains(&local_y)
        {
            return None;
        }
        let (pixels, left, top, right, bottom) =
            bucket_fill(&layer.pixels, local_x as u32, local_y as u32, color)?;
        let origin_x = i64::from(layer.offset.0);
        let origin_y = i64::from(layer.offset.1);
        layer.pixels = Arc::new(pixels);
        Some(PixelRect::new(
            origin_x + i64::from(left),
            origin_y + i64::from(top),
            right - left + 1,
            bottom - top + 1,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PixelBounds {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl PixelBounds {
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let overlap = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (overlap.left < overlap.right && overlap.top < overlap.bottom).then_some(overlap)
    }

    pub fn local_origin(self, clipped: Self) -> (u32, u32) {
        (
            (clipped.left - self.left) as u32,
            (clipped.top - self.top) as u32,
        )
    }

    pub fn dimensions(self) -> (u32, u32) {
        (
            (self.right - self.left) as u32,
            (self.bottom - self.top) as u32,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub fn between(start: (u32, u32), end: (u32, u32)) -> Self {
        Self {
            x: start.0.min(end.0),
            y: start.1.min(end.1),
            width: start.0.abs_diff(end.0).saturating_add(1),
            height: start.1.abs_diff(end.1).saturating_add(1),
        }
    }

    fn bounds(self) -> Option<PixelBounds> {
        Some(PixelBounds {
            left: i64::from(self.x),
            top: i64::from(self.y),
            right: i64::from(self.x.checked_add(self.width)?),
            bottom: i64::from(self.y.checked_add(self.height)?),
        })
    }
}

fn copy_name(name: &str) -> String {
    const SUFFIX: &str = " copy";
    let keep = MAX_LAYER_NAME_CHARS - SUFFIX.chars().count();
    let mut copy: String = name.chars().take(keep).collect();
    copy.push_str(SUFFIX);
    copy
}

fn bucket_fill(
    source: &RgbaImage,
    x: u32,
    y: u32,
    color: Rgba<u8>,
) -> Option<(RgbaImage, u32, u32, u32, u32)> {
    let target = *source.get_pixel(x, y);
    if target == color {
        return None;
    }
    let mut image = source.clone();

    // Paint runs as they are queued, so a pixel is never queued twice.
    let height = image.height();
    let mut pending = vec![paint_run(&mut image, x, y, target, color)];
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (x, y, x, y);
    while let Some((left, right, y)) = pending.pop() {
        min_x = min_x.min(left);
        max_x = max_x.max(right);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        for row in [y.checked_sub(1), (y + 1 < height).then_some(y + 1)]
            .into_iter()
            .flatten()
        {
            let mut x = left;
            while x <= right {
                if *image.get_pixel(x, row) == target {
                    let run = paint_run(&mut image, x, row, target, color);
                    x = run.1 + 1;
                    pending.push(run);
                } else {
                    x += 1;
                }
            }
        }
    }
    Some((image, min_x, min_y, max_x, max_y))
}

fn paint_run(
    image: &mut RgbaImage,
    x: u32,
    y: u32,
    target: Rgba<u8>,
    color: Rgba<u8>,
) -> (u32, u32, u32) {
    let mut left = x;
    let mut right = x;
    while left > 0 && *image.get_pixel(left - 1, y) == target {
        left -= 1;
    }
    while right + 1 < image.width() && *image.get_pixel(right + 1, y) == target {
        right += 1;
    }
    for x in left..=right {
        image.put_pixel(x, y, color);
    }
    (left, right, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_clips_offsets_and_blends_in_stack_order() {
        let mut content = DocumentContent::single_layer(
            "Bottom".into(),
            RgbaImage::from_pixel(3, 2, Rgba([0, 0, 255, 128])),
        );
        let mut top = RasterLayer {
            id: LayerId(1),
            name: "Top".into(),
            pixels: Arc::new(RgbaImage::from_fn(2, 2, |x, _| {
                if x == 0 {
                    Rgba([0, 255, 0, 255])
                } else {
                    Rgba([255, 0, 0, 128])
                }
            })),
            offset: (-1, 1),
            visible: true,
            opacity: 128,
            blend_mode: BlendMode::Normal,
        };
        content.layers.push(top.clone());
        top.id = LayerId(2);
        top.offset = (1, 0);
        top.visible = false;
        content.layers.push(top.clone());
        top.id = LayerId(3);
        top.offset = (i32::MAX, i32::MIN);
        top.visible = true;
        content.layers.push(top);

        let pixels = content.composite();
        assert_eq!(pixels.get_pixel(0, 1).0, [102, 0, 153, 160]);
        for (x, y, pixel) in pixels.enumerate_pixels() {
            if (x, y) != (0, 1) {
                assert_eq!(pixel.0, [0, 0, 255, 128]);
            }
        }
    }

    #[test]
    fn from_layers_rejects_invalid_stacks() {
        let layer =
            DocumentContent::single_layer("Layer".into(), RgbaImage::new(2, 2)).layers[0].clone();
        let mut other = layer.clone();
        other.id = LayerId(1);
        assert!(DocumentContent::from_layers(2, 2, vec![layer.clone(), other.clone()]).is_ok());
        assert!(DocumentContent::from_layers(2, 2, vec![]).is_err());
        assert!(DocumentContent::from_layers(0, 2, vec![layer.clone()]).is_err());
        assert!(DocumentContent::from_layers(2, 2, vec![layer.clone(), layer.clone()]).is_err());
        other.pixels = Arc::new(RgbaImage::new(0, 3));
        assert!(DocumentContent::from_layers(2, 2, vec![layer, other]).is_err());
    }

    fn ids(content: &DocumentContent) -> Vec<u64> {
        content.layers.iter().map(|layer| layer.id.0).collect()
    }

    #[test]
    fn layer_commands_keep_ids_order_and_a_remaining_layer() {
        let mut content = DocumentContent::single_layer("Image".into(), RgbaImage::new(3, 2));
        let red = Rgba([255, 0, 0, 255]);
        let added = content.add_layer(LayerId(0), "Layer".into(), red).unwrap();
        assert_eq!(added, LayerId(1));
        assert_eq!(content.layers[1].pixels.dimensions(), (3, 2));
        assert_eq!(*content.layers[1].pixels.get_pixel(2, 1), red);
        let copy = content.duplicate_layer(LayerId(0)).unwrap();
        assert_eq!(ids(&content), [0, 2, 1]);
        assert_eq!(content.layer(copy).unwrap().name, "Image copy");
        assert!(Arc::ptr_eq(
            &content.layers[0].pixels,
            &content.layers[1].pixels
        ));

        assert!(content.move_layer(LayerId(0), 2));
        assert_eq!(ids(&content), [2, 1, 0]);
        assert!(!content.move_layer(LayerId(0), 2));
        assert!(!content.move_layer(LayerId(0), 3));
        assert!(content.rename_layer(LayerId(1), "Ink".into()));
        assert!(!content.rename_layer(LayerId(1), "Ink".into()));
        assert!(content.set_visible(LayerId(1), false));
        assert!(!content.set_visible(LayerId(1), false));
        assert!(content.set_opacity(LayerId(2), 10));
        assert!(!content.set_opacity(LayerId(2), 10));

        assert_eq!(content.delete_layer(LayerId(1)), Some(LayerId(2)));
        assert_eq!(content.delete_layer(LayerId(2)), Some(LayerId(0)));
        assert_eq!(content.delete_layer(LayerId(0)), None);
        assert_eq!(ids(&content), [0]);
        assert_eq!(
            content.add_layer(LayerId(0), "Next".into(), red),
            Some(LayerId(1))
        );

        let long = "x".repeat(MAX_LAYER_NAME_CHARS);
        assert!(content.rename_layer(LayerId(1), long));
        let copy = content.duplicate_layer(LayerId(1)).unwrap();
        let name = &content.layer(copy).unwrap().name;
        assert_eq!(name.chars().count(), MAX_LAYER_NAME_CHARS);
        assert!(name.ends_with(" copy"));

        while content.can_add_layer() {
            content.add_layer(LayerId(0), "Layer".into(), red).unwrap();
        }
        assert_eq!(content.layers.len(), MAX_LAYERS);
        assert!(content.add_layer(LayerId(0), "Layer".into(), red).is_none());
        assert!(content.duplicate_layer(LayerId(0)).is_none());
    }

    #[test]
    fn merge_down_preserves_the_composite_over_the_union() {
        let mut content = DocumentContent::single_layer(
            "Base".into(),
            RgbaImage::from_fn(5, 4, |x, y| Rgba([40 * x as u8, 60 * y as u8, 90, 200])),
        );
        let lower = content
            .add_layer(LayerId(0), "Lower".into(), Rgba([0; 4]))
            .unwrap();
        let upper = content
            .add_layer(lower, "Upper".into(), Rgba([0; 4]))
            .unwrap();
        content.layers[1].pixels = Arc::new(RgbaImage::from_fn(3, 3, |x, y| {
            Rgba([200, 30 * x as u8, 10, 60 + 60 * y as u8])
        }));
        content.layers[1].offset = (-1, 2);
        content.layers[1].opacity = 180;
        content.layers[2].pixels = Arc::new(RgbaImage::from_fn(2, 4, |x, _| {
            Rgba([10, 220, 30 * x as u8, 128])
        }));
        content.layers[2].offset = (4, -1);
        content.layers[2].opacity = 77;
        let before = content.composite();

        content.layers[1].visible = false;
        assert!(!content.can_merge_down(upper));
        content.layers[1].visible = true;
        assert!(!content.can_merge_down(LayerId(0)));
        assert_eq!(content.merge_down(upper), Some(lower));

        assert_eq!(ids(&content), [0, 1]);
        let merged = &content.layers[1];
        assert_eq!(merged.name, "Lower");
        assert_eq!(merged.offset, (-1, -1));
        assert_eq!(merged.pixels.dimensions(), (7, 6));
        assert_eq!((merged.opacity, merged.visible), (255, true));
        for (after, before) in content.composite().pixels().zip(before.pixels()) {
            for (after, before) in after.0.iter().zip(before.0) {
                assert!(after.abs_diff(before) <= 1, "{after} vs {before}");
            }
        }
    }

    #[test]
    fn crop_rebases_clipped_layers_and_keeps_their_ids() {
        let mut content = DocumentContent::single_layer("Bottom".into(), RgbaImage::new(4, 4));
        let mut layer = content.layers[0].clone();
        layer.id = LayerId(1);
        layer.offset = (-1, 2);
        layer.pixels = Arc::new(RgbaImage::from_fn(4, 2, |x, y| {
            Rgba([x as u8, y as u8, 0, 255])
        }));
        content.layers.push(layer);

        assert!(content.crop(CropRect::between((1, 1), (3, 2))));
        let layer = content.layer(LayerId(1)).unwrap();
        assert_eq!(layer.offset, (0, 1));
        assert_eq!(layer.pixels.dimensions(), (2, 1));
        assert_eq!(layer.pixels.get_pixel(0, 0).0, [2, 0, 0, 255]);
        assert_eq!(content.composite().get_pixel(1, 1).0, [3, 0, 0, 255]);

        assert!(content.crop(CropRect::between((2, 0), (2, 1))));
        assert!(content.layer(LayerId(1)).is_none());
        assert_eq!(content.top_layer_id(), LayerId(0));
    }

    #[test]
    fn crop_with_no_overlapping_layers_keeps_a_transparent_layer() {
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(3, 3, Rgba([255; 4])),
        );
        content.layers[0].offset = (3, 0);
        assert!(content.crop(CropRect::between((1, 1), (2, 2))));
        let layer = content.full_canvas_layer().unwrap();
        assert_eq!(layer.id, LayerId(0));
        assert_eq!(layer.pixels.dimensions(), (2, 2));
        assert!(layer.pixels.pixels().all(|pixel| pixel.0 == [0; 4]));
    }

    #[test]
    fn fill_maps_canvas_coordinates_to_the_chosen_layer() {
        let mut content = DocumentContent::single_layer("Bottom".into(), RgbaImage::new(4, 2));
        let mut layer = content.layers[0].clone();
        layer.id = LayerId(1);
        layer.offset = (1, -1);
        layer.pixels = Arc::new(RgbaImage::new(2, 2));
        content.layers.push(layer);
        let before = content.clone();
        let ink = Rgba([20, 30, 40, 255]);

        assert!(content.fill(LayerId(1), 0, 0, ink).is_none());
        assert_eq!(
            content.fill(LayerId(1), 1, 0, ink),
            Some(PixelRect::new(1, -1, 2, 2))
        );
        assert_eq!(*content.composite().get_pixel(1, 0), ink);
        assert_eq!(before.composite().get_pixel(1, 0).0, [0; 4]);
        assert!(Arc::ptr_eq(
            &before.layers[0].pixels,
            &content.layers[0].pixels
        ));
    }

    #[test]
    fn fill_does_not_cross_diagonal_or_different_alpha() {
        let blank = Rgba([0; 4]);
        let wall = Rgba([0, 0, 0, 255]);
        let ink = Rgba([255, 0, 255, 255]);
        let mut image = RgbaImage::from_pixel(2, 2, wall);
        image.put_pixel(0, 0, blank);
        image.put_pixel(1, 1, blank);
        let (filled, left, top, right, bottom) = bucket_fill(&image, 0, 0, ink).unwrap();
        assert_eq!((left, top, right, bottom), (0, 0, 0, 0));
        assert_eq!(filled.into_raw(), [ink.0, wall.0, wall.0, blank.0].concat());
    }
}
