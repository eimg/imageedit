use std::{collections::HashSet, sync::Arc};

use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage, imageops};

use super::{MAX_IMAGE_BYTES, composite};
use crate::preview::{Dirty, PixelRect};
use crate::selection::{Selection, within_tolerance};

pub const IMPORTED_LAYER_NAME: &str = "Image";
pub const MAX_LAYERS: usize = 128;
// Keeps a full stack's names within the project manifest limit.
pub const MAX_LAYER_NAME_CHARS: usize = 64;
// Matches the project reader's total decoded-pixel budget.
pub const MAX_STACK_BYTES: u64 = 512 * 1024 * 1024;

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
    mode: crate::transform::ColorMode,
    // Bottom to top. Constructors and edits keep at least one layer.
    pub(super) layers: Vec<RasterLayer>,
}

impl DocumentContent {
    pub fn single_layer(name: String, pixels: RgbaImage) -> Self {
        let (width, height) = pixels.dimensions();
        Self {
            width,
            height,
            mode: crate::transform::ColorMode::Rgb,
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
            mode: crate::transform::ColorMode::Rgb,
            layers,
        })
    }

    pub fn mode(&self) -> crate::transform::ColorMode {
        self.mode
    }

    pub(crate) fn set_stored_mode(&mut self, mode: crate::transform::ColorMode) {
        self.mode = mode;
    }

    pub fn set_color_mode(&mut self, mode: crate::transform::ColorMode) -> Dirty {
        if self.mode == mode {
            return Dirty::None;
        }
        self.mode = mode;
        if mode == crate::transform::ColorMode::Grayscale {
            for layer in &mut self.layers {
                layer.pixels = Arc::new(crate::transform::grayscale((*layer.pixels).clone()));
            }
            Dirty::Canvas
        } else {
            Dirty::Unpainted
        }
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

    pub fn can_create_layer(&self) -> bool {
        self.can_add_layer() && self.fits_stack(0, self.canvas_bytes())
    }

    pub fn can_duplicate_layer(&self, id: LayerId) -> bool {
        self.can_add_layer()
            && self
                .layer(id)
                .is_some_and(|layer| self.fits_stack(0, layer.pixels.as_raw().len() as u64))
    }

    // Inserts a canvas-sized layer above `above`, or at the top if it is missing.
    pub fn add_layer(&mut self, above: LayerId, name: String, fill: Rgba<u8>) -> Option<LayerId> {
        if !self.can_create_layer() {
            return None;
        }
        let id = self.next_layer_id()?;
        let index = self
            .layer_index(above)
            .map_or(self.layers.len(), |index| index + 1);
        let fill = self.mode.constrain(fill);
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

    // How many of `layers` fit above `above`, in the given order. Each one is centered.
    pub fn insert_centered_layers(
        &mut self,
        above: LayerId,
        layers: Vec<ImageLayer>,
    ) -> InsertedLayers {
        let mut above = above;
        let mut added = 0;
        let mut top = None;
        for layer in layers {
            if let Err(stop) = self.room_for(0, 0, &layer.pixels) {
                return InsertedLayers {
                    added,
                    top,
                    stopped: Some(stop),
                };
            }
            let Some(id) = self.insert_centered_layer(above, layer) else {
                return InsertedLayers {
                    added,
                    top,
                    stopped: Some(InsertStop::Full),
                };
            };
            above = id;
            top = Some(id);
            added += 1;
        }
        InsertedLayers {
            added,
            top,
            stopped: None,
        }
    }

    // `extra_layers` and `extra_bytes` are images accepted in this batch but not inserted yet.
    pub fn room_for(
        &self,
        extra_layers: usize,
        extra_bytes: u64,
        pixels: &RgbaImage,
    ) -> Result<(), InsertStop> {
        if self.layers.len().saturating_add(extra_layers) >= MAX_LAYERS {
            return Err(InsertStop::Full);
        }
        let Some(bytes) = image_bytes(pixels) else {
            return Err(InsertStop::TooLarge);
        };
        if bytes == 0 || bytes > MAX_IMAGE_BYTES {
            return Err(InsertStop::TooLarge);
        }
        let used = self.stack_bytes().saturating_add(extra_bytes);
        if used.saturating_add(bytes) > MAX_STACK_BYTES {
            Err(InsertStop::TooLarge)
        } else {
            Ok(())
        }
    }

    fn insert_centered_layer(&mut self, above: LayerId, layer: ImageLayer) -> Option<LayerId> {
        let id = self.next_layer_id()?;
        let offset = crate::geometry::centered_offset(
            (0, 0),
            layer.pixels.width(),
            layer.pixels.height(),
            self.width,
            self.height,
            crate::geometry::Axis::Both,
        );
        let index = self
            .layer_index(above)
            .map_or(self.layers.len(), |index| index + 1);
        let pixels = self.mode.constrain_image(layer.pixels);
        self.layers.insert(
            index,
            RasterLayer {
                id,
                name: limited_name(&layer.name),
                pixels: Arc::new(pixels),
                offset,
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
            },
        );
        Some(id)
    }

    fn stack_bytes(&self) -> u64 {
        self.layers.iter().fold(0, |total, layer| {
            total.saturating_add(image_bytes(&layer.pixels).unwrap_or(u64::MAX))
        })
    }

    fn canvas_bytes(&self) -> u64 {
        (u64::from(self.width) * u64::from(self.height)).saturating_mul(4)
    }

    fn fits_stack(&self, removed_bytes: u64, added_bytes: u64) -> bool {
        added_bytes <= MAX_IMAGE_BYTES
            && self
                .stack_bytes()
                .saturating_sub(removed_bytes)
                .saturating_add(added_bytes)
                <= MAX_STACK_BYTES
    }

    pub fn duplicate_layer(&mut self, id: LayerId) -> Option<LayerId> {
        if !self.can_duplicate_layer(id) {
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
        let removed = lower.pixels.as_raw().len() as u64 + upper.pixels.as_raw().len() as u64;
        self.fits_stack(removed, bytes.checked_mul(4)?)
            .then_some((index, union))
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
    pub fn fill(
        &mut self,
        id: LayerId,
        x: u32,
        y: u32,
        color: Rgba<u8>,
        selection: &Selection,
        tolerance: u8,
    ) -> Option<PixelRect> {
        if x >= self.width || y >= self.height {
            return None;
        }
        if !selection.covers(i64::from(x), i64::from(y), self.width, self.height) {
            return None;
        }
        let color = self.mode.constrain(color);
        let layer = self.layers.iter_mut().find(|layer| layer.id == id)?;
        let local_x = i64::from(x) - i64::from(layer.offset.0);
        let local_y = i64::from(y) - i64::from(layer.offset.1);
        if !(0..i64::from(layer.pixels.width())).contains(&local_x)
            || !(0..i64::from(layer.pixels.height())).contains(&local_y)
        {
            return None;
        }
        let origin_x = i64::from(layer.offset.0);
        let origin_y = i64::from(layer.offset.1);
        let canvas = (self.width, self.height);
        let coverage = |px: u32, py: u32| {
            selection.coverage(
                origin_x + i64::from(px),
                origin_y + i64::from(py),
                canvas.0,
                canvas.1,
            )
        };
        let (pixels, left, top, right, bottom) = bucket_fill(
            &layer.pixels,
            local_x as u32,
            local_y as u32,
            color,
            tolerance,
            coverage,
        )?;
        layer.pixels = Arc::new(pixels);
        Some(PixelRect::new(
            origin_x + i64::from(left),
            origin_y + i64::from(top),
            right - left + 1,
            bottom - top + 1,
        ))
    }

    // Sets covered pixels to transparent. Pixels that are already clear do not count as a change.
    pub fn clear(&mut self, id: LayerId, selection: &Selection) -> Option<PixelRect> {
        let canvas = (self.width, self.height);
        let layer = self.layers.iter_mut().find(|layer| layer.id == id)?;
        if !layer.visible {
            return None;
        }
        let origin_x = i64::from(layer.offset.0);
        let origin_y = i64::from(layer.offset.1);
        let (width, height) = layer.pixels.dimensions();
        let mut image = None;
        let (mut min_x, mut min_y) = (i64::MAX, i64::MAX);
        let (mut max_x, mut max_y) = (i64::MIN, i64::MIN);
        for y in 0..height {
            for x in 0..width {
                let canvas_x = origin_x + i64::from(x);
                let canvas_y = origin_y + i64::from(y);
                let coverage = selection.coverage(canvas_x, canvas_y, canvas.0, canvas.1);
                if coverage == 0 {
                    continue;
                }
                let before = *layer.pixels.get_pixel(x, y);
                let after = if coverage == 255 {
                    Rgba([0; 4])
                } else {
                    let mut pixel = before;
                    pixel[3] = crate::selection::apply_coverage(pixel[3], 255 - coverage);
                    pixel
                };
                if after == before {
                    continue;
                }
                let image = image.get_or_insert_with(|| layer.pixels.as_ref().clone());
                image.put_pixel(x, y, after);
                min_x = min_x.min(canvas_x);
                min_y = min_y.min(canvas_y);
                max_x = max_x.max(canvas_x);
                max_y = max_y.max(canvas_y);
            }
        }
        layer.pixels = Arc::new(image?);
        Some(PixelRect::new(
            min_x,
            min_y,
            (max_x - min_x + 1) as u32,
            (max_y - min_y + 1) as u32,
        ))
    }

    pub fn set_offset(&mut self, id: LayerId, offset: (i32, i32)) -> Dirty {
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return Dirty::None;
        };
        if layer.offset == offset {
            return Dirty::None;
        }
        let before = layer.bounds();
        layer.offset = offset;
        let after = layer.bounds();
        region_from_bounds(before.union(after))
    }

    pub fn accepts_replacement(&self, id: LayerId, pixels: &RgbaImage) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        let Some(added) = image_bytes(pixels) else {
            return false;
        };
        let removed = image_bytes(&self.layers[index].pixels).unwrap_or(0);
        self.fits_stack(removed, added)
    }

    // Replaces the layer's pixels and origin. The selection is left untouched.
    pub fn place_layer(&mut self, id: LayerId, pixels: RgbaImage, offset: (i32, i32)) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        let pixels = self.mode.constrain_image(pixels);
        if self.layers[index].offset == offset && self.layers[index].pixels.as_ref() == &pixels {
            return Dirty::None;
        }
        if !self.accepts_replacement(id, &pixels) {
            return Dirty::None;
        }
        self.layers[index].pixels = Arc::new(pixels);
        self.layers[index].offset = offset;
        Dirty::Canvas
    }

    pub fn align_layer(&mut self, id: LayerId, axis: crate::geometry::Axis) -> Dirty {
        let Some(layer) = self.layer(id) else {
            return Dirty::None;
        };
        let offset = crate::geometry::centered_offset(
            layer.offset,
            layer.pixels.width(),
            layer.pixels.height(),
            self.width,
            self.height,
            axis,
        );
        self.set_offset(id, offset)
    }

    pub fn can_fit_layer(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|layer| {
            self.fits_stack(layer.pixels.as_raw().len() as u64, self.canvas_bytes())
        })
    }

    // Resizes the layer to the canvas. Pixels keep their canvas position; the rest is transparent.
    pub fn layer_to_image_size(&mut self, id: LayerId) -> Dirty {
        if !self.can_fit_layer(id) {
            return Dirty::None;
        }
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return Dirty::None;
        };
        let Some(pixels) = crate::geometry::layer_to_image_size(
            layer.offset,
            &layer.pixels,
            self.width,
            self.height,
        ) else {
            return Dirty::None;
        };
        layer.pixels = Arc::new(pixels);
        layer.offset = (0, 0);
        Dirty::Canvas
    }

    pub fn crop_layer_to_content(&mut self, id: LayerId) -> LayerGeometry {
        let Some(index) = self.layer_index(id) else {
            return LayerGeometry::Unchanged;
        };
        let crop = match crate::geometry::layer_content_crop(&self.layers[index].pixels) {
            crate::geometry::ContentCrop::Empty => return LayerGeometry::Empty,
            crate::geometry::ContentCrop::Unchanged => return LayerGeometry::Unchanged,
            crate::geometry::ContentCrop::Crop(crop) => crop,
        };
        let layer = &mut self.layers[index];
        let before = layer.bounds();
        layer.pixels = Arc::new(
            imageops::crop_imm(&*layer.pixels, crop.x, crop.y, crop.width, crop.height).to_image(),
        );
        layer.offset = (
            layer.offset.0.saturating_add_unsigned(crop.x),
            layer.offset.1.saturating_add_unsigned(crop.y),
        );
        let after = layer.bounds();
        LayerGeometry::Done(region_from_bounds(before.union(after)))
    }

    pub fn image_content_rect(&self) -> crate::geometry::ContentCrop {
        let samples: Vec<_> = self
            .layers
            .iter()
            .map(|layer| crate::geometry::LayerSample {
                offset: layer.offset,
                pixels: layer.pixels.as_ref(),
                visible: layer.visible,
                opacity: layer.opacity,
            })
            .collect();
        crate::geometry::image_content_crop(&samples, self.width, self.height)
    }

    pub fn crop_image_to_content(&mut self) -> LayerGeometry {
        match self.image_content_rect() {
            crate::geometry::ContentCrop::Empty => LayerGeometry::Empty,
            crate::geometry::ContentCrop::Unchanged => LayerGeometry::Unchanged,
            crate::geometry::ContentCrop::Crop(crop) => {
                let cropped = self.crop(CropRect {
                    x: crop.x,
                    y: crop.y,
                    width: crop.width,
                    height: crop.height,
                });
                if cropped {
                    LayerGeometry::Done(Dirty::Canvas)
                } else {
                    LayerGeometry::Unchanged
                }
            }
        }
    }

    // Flatten the layers on each side of a moving layer so it can be painted in stack order.
    pub fn composite_around(&self, id: LayerId) -> (Option<RgbaImage>, Option<RgbaImage>) {
        let index = self.layer_index(id).expect("the moving layer exists");
        let bounds = PixelBounds {
            left: 0,
            top: 0,
            right: i64::from(self.width),
            bottom: i64::from(self.height),
        };
        let composite = |layers: &[RasterLayer]| {
            layers
                .iter()
                .any(|layer| {
                    layer.visible
                        && layer.opacity != 0
                        && layer.bounds().intersect(bounds).is_some()
                })
                .then(|| composite::flatten_region(layers, bounds))
        };
        (
            composite(&self.layers[..index]),
            composite(&self.layers[index + 1..]),
        )
    }

    pub fn turn_image(&mut self, turn: crate::transform::Turn) -> bool {
        let canvas = self.size();
        let mut planned = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            let Some(offset) = crate::transform::turned_offset(
                layer.offset,
                layer.pixels.dimensions(),
                canvas,
                turn,
            ) else {
                return false;
            };
            planned.push((crate::transform::turn_pixels(&layer.pixels, turn), offset));
        }
        for (layer, (pixels, offset)) in self.layers.iter_mut().zip(planned) {
            layer.pixels = Arc::new(pixels);
            layer.offset = offset;
        }
        (self.width, self.height) = crate::transform::turned_size(canvas.0, canvas.1, turn);
        true
    }

    pub fn mirror_image(&mut self, mirror: crate::transform::Mirror) -> bool {
        let canvas = self.size();
        let mut planned = Vec::with_capacity(self.layers.len());
        for layer in &self.layers {
            let Some(offset) = crate::transform::mirrored_offset(
                layer.offset,
                layer.pixels.dimensions(),
                canvas,
                mirror,
            ) else {
                return false;
            };
            planned.push((
                crate::transform::mirror_pixels(&layer.pixels, mirror),
                offset,
            ));
        }
        for (layer, (pixels, offset)) in self.layers.iter_mut().zip(planned) {
            layer.pixels = Arc::new(pixels);
            layer.offset = offset;
        }
        true
    }

    pub fn turn_layer(&mut self, id: LayerId, turn: crate::transform::Turn) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        let offset = self.layers[index].offset;
        let old = self.layers[index].pixels.dimensions();
        let pixels = crate::transform::turn_pixels(&self.layers[index].pixels, turn);
        self.layers[index].offset =
            crate::transform::centered_offset(offset, old, pixels.dimensions());
        self.layers[index].pixels = Arc::new(pixels);
        true
    }

    pub fn mirror_layer(&mut self, id: LayerId, mirror: crate::transform::Mirror) -> bool {
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        layer.pixels = Arc::new(crate::transform::mirror_pixels(&layer.pixels, mirror));
        true
    }

    pub fn scale_image(
        &mut self,
        width: u32,
        height: u32,
        resample: crate::transform::Resample,
    ) -> bool {
        let old = self.size();
        if (width, height) == old || !crate::transform::canvas_allowed(width, height) {
            return false;
        }
        let mut plan = Vec::with_capacity(self.layers.len());
        let mut total = 0u64;
        for layer in &self.layers {
            let Some((offset, size)) = crate::transform::scale_placement(
                layer.offset,
                layer.pixels.dimensions(),
                old,
                (width, height),
            ) else {
                return false;
            };
            let Some(bytes) = u64::from(size.0)
                .checked_mul(u64::from(size.1))
                .and_then(|pixels| pixels.checked_mul(4))
            else {
                return false;
            };
            if bytes > MAX_IMAGE_BYTES {
                return false;
            }
            total = total.saturating_add(bytes);
            plan.push((offset, size));
        }
        if !self.fits_stack(self.stack_bytes(), total) {
            return false;
        }
        for (layer, (offset, size)) in self.layers.iter_mut().zip(plan) {
            layer.pixels = Arc::new(crate::transform::scale_pixels(
                &layer.pixels,
                size.0,
                size.1,
                resample,
            ));
            layer.offset = offset;
        }
        self.width = width;
        self.height = height;
        true
    }

    pub fn scale_layer(
        &mut self,
        id: LayerId,
        width: u32,
        height: u32,
        resample: crate::transform::Resample,
    ) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        let old = self.layers[index].pixels.dimensions();
        if (width, height) == old {
            return false;
        }
        let Some(bytes) = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            return false;
        };
        let removed = image_bytes(&self.layers[index].pixels).unwrap_or(u64::MAX);
        if !self.fits_stack(removed, bytes) {
            return false;
        }
        let offset =
            crate::transform::centered_offset(self.layers[index].offset, old, (width, height));
        let pixels =
            crate::transform::scale_pixels(&self.layers[index].pixels, width, height, resample);
        self.layers[index].offset = offset;
        self.layers[index].pixels = Arc::new(pixels);
        true
    }

    pub fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        anchor: crate::transform::Anchor,
    ) -> Option<(i32, i32)> {
        if (width, height) == self.size() || !crate::transform::canvas_allowed(width, height) {
            return None;
        }
        let delta = crate::transform::anchor_delta(self.size(), (width, height), anchor);
        for layer in &self.layers {
            i32::try_from(i64::from(layer.offset.0) + i64::from(delta.0)).ok()?;
            i32::try_from(i64::from(layer.offset.1) + i64::from(delta.1)).ok()?;
        }
        for layer in &mut self.layers {
            layer.offset.0 += delta.0;
            layer.offset.1 += delta.1;
        }
        self.width = width;
        self.height = height;
        Some(delta)
    }
}

pub enum LayerGeometry {
    Unchanged,
    Empty,
    Done(Dirty),
}

fn region_from_bounds(bounds: PixelBounds) -> Dirty {
    let width = bounds.right.saturating_sub(bounds.left);
    let height = bounds.bottom.saturating_sub(bounds.top);
    match (u32::try_from(width), u32::try_from(height)) {
        (Ok(width), Ok(height)) if width > 0 && height > 0 => {
            Dirty::Region(PixelRect::new(bounds.left, bounds.top, width, height))
        }
        _ => Dirty::Canvas,
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

pub struct ImageLayer {
    pub name: String,
    pub pixels: RgbaImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertStop {
    Full,
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertedLayers {
    pub added: usize,
    pub top: Option<LayerId>,
    pub stopped: Option<InsertStop>,
}

pub(crate) fn image_bytes(pixels: &RgbaImage) -> Option<u64> {
    u64::from(pixels.width())
        .checked_mul(u64::from(pixels.height()))?
        .checked_mul(4)
}

fn limited_name(name: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() { "Layer" } else { name };
    name.chars().take(MAX_LAYER_NAME_CHARS).collect()
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
    tolerance: u8,
    coverage: impl Fn(u32, u32) -> u8,
) -> Option<(RgbaImage, u32, u32, u32, u32)> {
    let target = *source.get_pixel(x, y);
    if (tolerance == 0 && target == color) || coverage(x, y) == 0 {
        return None;
    }
    let (width, height) = source.dimensions();
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut visited = vec![false; width as usize * height as usize];
    let mut pending = vec![(x, y)];
    let mut image = None;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (x, y, x, y);
    let paintable = |visited: &[bool], x, y| {
        !visited[index(x, y)]
            && within_tolerance(*source.get_pixel(x, y), target, tolerance)
            && coverage(x, y) > 0
    };

    // Discover runs against the original pixels. A replacement can still match the target.
    while let Some((x, y)) = pending.pop() {
        if !paintable(&visited, x, y) {
            continue;
        }
        let mut left = x;
        let mut right = x;
        while left > 0 && paintable(&visited, left - 1, y) {
            left -= 1;
        }
        while right + 1 < width && paintable(&visited, right + 1, y) {
            right += 1;
        }
        for x in left..=right {
            visited[index(x, y)] = true;
            let before = *source.get_pixel(x, y);
            let after = mix_fill(before, color, coverage(x, y));
            if before != after {
                image
                    .get_or_insert_with(|| source.clone())
                    .put_pixel(x, y, after);
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
        for row in [y.checked_sub(1), (y + 1 < height).then_some(y + 1)]
            .into_iter()
            .flatten()
        {
            let mut in_run = false;
            for x in left..=right {
                let matches = paintable(&visited, x, row);
                if matches && !in_run {
                    pending.push((x, row));
                }
                in_run = matches;
            }
        }
    }
    Some((image?, min_x, min_y, max_x, max_y))
}

fn mix_fill(before: Rgba<u8>, color: Rgba<u8>, coverage: u8) -> Rgba<u8> {
    if coverage == 255 {
        return color;
    }
    if before == color {
        return before;
    }
    // Interpolate in premultiplied color so a transparent pixel adds no color to a soft edge.
    let old = u32::from(before[3]) * u32::from(255 - coverage);
    let new = u32::from(color[3]) * u32::from(coverage);
    let alpha = old + new;
    if alpha == 0 {
        return before;
    }
    let channel =
        |i| ((u32::from(before[i]) * old + u32::from(color[i]) * new + alpha / 2) / alpha) as u8;
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        ((alpha + 127) / 255) as u8,
    ])
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
    fn layer_operations_respect_the_project_pixel_budget() {
        // 85 shared 6 MiB images occupy 510 MiB in the project even though they share RAM.
        let pixels = Arc::new(RgbaImage::new(1536, 1024));
        let layers = (0..85)
            .map(|id| RasterLayer {
                id: LayerId(id),
                name: "Large layer".into(),
                pixels: pixels.clone(),
                offset: (0, 0),
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
            })
            .collect();
        let mut content = DocumentContent::from_layers(1536, 1024, layers).unwrap();
        assert!(content.can_add_layer());
        assert!(
            content
                .add_layer(LayerId(0), "Extra".into(), Rgba([0; 4]))
                .is_none()
        );
        assert!(content.duplicate_layer(LayerId(0)).is_none());

        let tiny = ImageLayer {
            name: "Tiny".into(),
            pixels: RgbaImage::new(1, 1),
        };
        let lower = content.insert_centered_layer(LayerId(84), tiny).unwrap();
        assert_eq!(content.layer_to_image_size(lower), Dirty::None);
        let upper = content.duplicate_layer(lower).unwrap();
        content.set_offset(upper, (1791, 1023));
        content.set_offset(lower, (0, 0));
        assert!(!content.can_merge_down(upper));
        assert!(content.merge_down(upper).is_none());
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

        let open = Selection::Unrestricted;
        assert!(content.fill(LayerId(1), 0, 0, ink, &open, 0).is_none());
        assert_eq!(
            content.fill(LayerId(1), 1, 0, ink, &open, 0),
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
    fn fill_terminates_when_the_replacement_is_within_tolerance() {
        let source = RgbaImage::from_pixel(3, 3, Rgba([0, 0, 0, 255]));
        let color = Rgba([1, 0, 0, 255]);
        let (filled, left, top, right, bottom) =
            bucket_fill(&source, 1, 1, color, 1, |_, _| 255).unwrap();
        assert_eq!((left, top, right, bottom), (0, 0, 2, 2));
        assert!(filled.pixels().all(|pixel| *pixel == color));
    }

    #[test]
    fn fill_and_clear_respect_feathered_coverage() {
        let mut content = DocumentContent::single_layer("Image".into(), RgbaImage::new(3, 3));
        let selection = Selection::rectangle((1, 1), (1, 1), 3, 3).feather(1, 3, 3);
        let id = content.top_layer_id();
        assert!(
            content
                .fill(id, 1, 1, Rgba([255, 0, 0, 255]), &selection, 0)
                .is_some()
        );
        assert_eq!(content.layers[0].pixels.get_pixel(1, 1).0, [255, 0, 0, 28]);
        assert!(content.clear(id, &selection).is_some());
        assert_eq!(content.layers[0].pixels.get_pixel(1, 1).0, [255, 0, 0, 25]);
    }

    #[test]
    fn fill_does_not_cross_diagonal_or_different_alpha() {
        let blank = Rgba([0; 4]);
        let wall = Rgba([0, 0, 0, 255]);
        let ink = Rgba([255, 0, 255, 255]);
        let mut image = RgbaImage::from_pixel(2, 2, wall);
        image.put_pixel(0, 0, blank);
        image.put_pixel(1, 1, blank);
        let (filled, left, top, right, bottom) =
            bucket_fill(&image, 0, 0, ink, 0, |_, _| 255).unwrap();
        assert_eq!((left, top, right, bottom), (0, 0, 0, 0));
        assert_eq!(filled.into_raw(), [ink.0, wall.0, wall.0, blank.0].concat());
    }

    #[test]
    fn fill_and_clear_follow_the_selection_on_an_offset_layer() {
        let ink = Rgba([9, 8, 7, 255]);
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0])),
        );
        let mut layer = content.layers[0].clone();
        layer.id = LayerId(1);
        layer.offset = (2, 3);
        layer.pixels = Arc::new(RgbaImage::from_pixel(4, 4, Rgba([1, 2, 3, 255])));
        content.layers.push(layer);
        let selection = Selection::Rect(PixelRect::new(3, 4, 2, 1));
        assert!(content.fill(LayerId(1), 2, 3, ink, &selection, 0).is_none());
        assert_eq!(
            content.fill(LayerId(1), 3, 4, ink, &selection, 0),
            Some(PixelRect::new(3, 4, 2, 1))
        );
        let pixels = content.layers[1].pixels.clone();
        assert_eq!(pixels.get_pixel(1, 1).0, ink.0);
        assert_eq!(pixels.get_pixel(2, 1).0, ink.0);
        assert_eq!(pixels.get_pixel(0, 0).0, [1, 2, 3, 255]);
        assert_eq!(pixels.get_pixel(1, 2).0, [1, 2, 3, 255]);

        let cleared = content.clear(LayerId(1), &selection).unwrap();
        assert_eq!(cleared, PixelRect::new(3, 4, 2, 1));
        assert_eq!(content.layers[1].pixels.get_pixel(1, 1).0, [0, 0, 0, 0]);
        assert_eq!(content.layers[1].pixels.get_pixel(0, 0).0, [1, 2, 3, 255]);
        assert!(content.clear(LayerId(1), &Selection::Empty).is_none());
    }

    #[test]
    fn a_move_preview_keeps_layers_below_and_above_the_sprite() {
        let red = Rgba([255, 0, 0, 255]);
        let green = Rgba([0, 255, 0, 255]);
        let blue = Rgba([0, 0, 255, 255]);
        let mut content =
            DocumentContent::single_layer("Lower".into(), RgbaImage::from_pixel(2, 2, red));
        let moving = content
            .add_layer(LayerId(0), "Moving".into(), green)
            .unwrap();
        let upper = content.add_layer(moving, "Upper".into(), blue).unwrap();

        let (below, above) = content.composite_around(moving);
        assert_eq!(*below.unwrap().get_pixel(0, 0), red);
        assert_eq!(*above.unwrap().get_pixel(0, 0), blue);
        assert!(content.composite_around(upper).1.is_none());
        content.set_visible(upper, false);
        assert!(content.composite_around(moving).1.is_none());
    }

    #[test]
    fn moving_and_aligning_cover_the_old_and_new_layer_bounds() {
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(4, 2, Rgba([1; 4])),
        );
        let id = content.top_layer_id();
        content.layers[0].offset = (-1, 3);
        assert_eq!(
            content.set_offset(id, (2, 3)),
            Dirty::Region(PixelRect::new(-1, 3, 7, 2))
        );
        assert_eq!(content.layers[0].offset, (2, 3));
        assert_eq!(
            content.align_layer(id, crate::geometry::Axis::Horizontal),
            Dirty::Region(PixelRect::new(0, 3, 6, 2))
        );
        assert_eq!(content.layers[0].offset, (0, 3));
        assert_eq!(content.set_offset(id, (0, 3)), Dirty::None);
    }

    #[test]
    fn cropping_to_content_keeps_visible_pixels_and_rejects_a_clear_layer() {
        let mut image = RgbaImage::new(4, 3);
        image.put_pixel(1, 1, Rgba([4, 5, 6, 255]));
        image.put_pixel(2, 1, Rgba([4, 5, 6, 128]));
        let mut content = DocumentContent::single_layer("Image".into(), image);
        let id = content.top_layer_id();
        content.layers[0].offset = (5, -2);
        assert!(matches!(
            content.crop_layer_to_content(id),
            LayerGeometry::Done(_)
        ));
        assert_eq!(content.layers[0].offset, (6, -1));
        assert_eq!(content.layers[0].pixels.dimensions(), (2, 1));
        assert_eq!(content.layers[0].pixels.get_pixel(0, 0).0, [4, 5, 6, 255]);

        content.layers[0].pixels = Arc::new(RgbaImage::new(3, 3));
        assert!(matches!(
            content.crop_layer_to_content(id),
            LayerGeometry::Empty
        ));
        assert_eq!(content.layers[0].pixels.dimensions(), (3, 3));
    }

    #[test]
    fn cropping_the_image_uses_visible_pixels_and_leaves_a_clear_canvas() {
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(6, 5, Rgba([0, 0, 0, 0])),
        );
        let id = content
            .add_layer(content.top_layer_id(), "Mark".into(), Rgba([0, 0, 0, 0]))
            .unwrap();
        let mut mark = RgbaImage::new(3, 2);
        mark.put_pixel(1, 0, Rgba([9, 0, 0, 255]));
        content.layers[1].pixels = Arc::new(mark);
        content.layers[1].offset = (2, 1);
        content.layers[0].visible = false;
        content.layers[0].pixels = Arc::new(RgbaImage::from_pixel(6, 5, Rgba([1, 1, 1, 255])));
        assert!(matches!(
            content.crop_image_to_content(),
            LayerGeometry::Done(Dirty::Canvas)
        ));
        assert_eq!(content.size(), (1, 1));
        assert_eq!(content.layer(id).unwrap().offset, (0, 0));
        assert_eq!(content.layer(id).unwrap().pixels.get_pixel(0, 0).0[0], 9);

        let clear = DocumentContent::single_layer("Image".into(), RgbaImage::new(4, 4));
        let mut clear = clear;
        assert!(matches!(
            clear.crop_image_to_content(),
            LayerGeometry::Empty
        ));
        assert_eq!(clear.size(), (4, 4));
    }

    #[test]
    fn inserted_layers_are_centered_above_the_active_layer_until_the_stack_is_full() {
        let mut content = DocumentContent::single_layer("Image".into(), RgbaImage::new(4, 4));
        let base = content.top_layer_id();
        let inserted = content.insert_centered_layers(
            base,
            vec![
                ImageLayer {
                    name: "  One  ".into(),
                    pixels: RgbaImage::from_pixel(2, 2, Rgba([9, 0, 0, 128])),
                },
                ImageLayer {
                    name: "N".repeat(MAX_LAYER_NAME_CHARS + 4),
                    pixels: RgbaImage::new(1, 1),
                },
            ],
        );
        assert_eq!(
            inserted,
            InsertedLayers {
                added: 2,
                top: content.top_layer_id().into(),
                stopped: None,
            }
        );
        assert_eq!(content.layers()[1].name, "One");
        assert_eq!(content.layers()[1].offset, (1, 1));
        assert_eq!(content.layers()[1].pixels.get_pixel(0, 0).0, [9, 0, 0, 128]);
        assert_eq!(
            content.layers()[2].name.chars().count(),
            MAX_LAYER_NAME_CHARS
        );
        assert_eq!(content.layers()[2].offset, (1, 1));
        assert_eq!(
            content.room_for(0, MAX_STACK_BYTES, &RgbaImage::new(1, 1)),
            Err(InsertStop::TooLarge)
        );

        while content.can_add_layer() {
            content
                .add_layer(content.top_layer_id(), "Fill".into(), Rgba([0; 4]))
                .unwrap();
        }
        let full = content.insert_centered_layers(
            content.top_layer_id(),
            vec![ImageLayer {
                name: "Extra".into(),
                pixels: RgbaImage::new(1, 1),
            }],
        );
        assert_eq!(full.added, 0);
        assert_eq!(full.stopped, Some(InsertStop::Full));
        assert!(
            content
                .room_for(0, MAX_STACK_BYTES, &RgbaImage::new(1, 1))
                .is_err()
        );
    }

    #[test]
    fn grayscale_constrains_later_color_and_keeps_alpha() {
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 200])),
        );
        assert!(matches!(
            content.set_color_mode(crate::transform::ColorMode::Grayscale),
            Dirty::Canvas
        ));
        let converted = content.layers()[0].pixels.get_pixel(0, 0).0;
        assert_eq!(converted[0], converted[1]);
        assert_eq!(converted[3], 200);
        content
            .fill(
                content.top_layer_id(),
                0,
                0,
                Rgba([255, 0, 0, 128]),
                &Selection::Unrestricted,
                0,
            )
            .unwrap();
        let filled = content.layers()[0].pixels.get_pixel(0, 0).0;
        assert_eq!(filled[0], filled[1]);
        assert_eq!(filled[1], filled[2]);
        assert_eq!(filled[3], 128);
        assert!(matches!(
            content.set_color_mode(crate::transform::ColorMode::Rgb),
            Dirty::Unpainted
        ));
        assert_eq!(content.layers()[0].pixels.get_pixel(0, 0).0, filled);
        assert_eq!(content.mode(), crate::transform::ColorMode::Rgb);
    }
}
