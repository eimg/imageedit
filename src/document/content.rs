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
    pub text: Option<crate::text::TextStyle>,
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
    paths: Vec<crate::paths::VectorPath>,
    active_path: Option<crate::paths::PathId>,
    guides: Vec<crate::guides::Guide>,
}

struct ScaledLayer {
    offset: (i32, i32),
    size: (u32, u32),
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
                text: None,
            }],
            paths: Vec::new(),
            active_path: None,
            guides: Vec::new(),
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
            paths: Vec::new(),
            active_path: None,
            guides: Vec::new(),
        })
    }

    pub fn guides(&self) -> &[crate::guides::Guide] {
        &self.guides
    }

    pub fn set_guides(&mut self, guides: Vec<crate::guides::Guide>) -> Result<()> {
        ensure!(
            guides.len() <= crate::guides::MAX_GUIDES
                && guides.iter().all(|guide| guide.valid(self.size())),
            "Invalid or excessive image guides"
        );
        self.guides = guides;
        Ok(())
    }

    pub fn paths(&self) -> &[crate::paths::VectorPath] {
        &self.paths
    }

    pub fn active_path_id(&self) -> Option<crate::paths::PathId> {
        self.active_path
    }

    pub fn active_path(&self) -> Option<&crate::paths::VectorPath> {
        self.active_path
            .and_then(|id| self.paths.iter().find(|path| path.id == id))
    }

    pub fn path_selection(&self) -> Selection {
        crate::paths::selection(&self.paths, self.width, self.height)
    }

    pub fn set_paths(
        &mut self,
        paths: Vec<crate::paths::VectorPath>,
        active: Option<crate::paths::PathId>,
    ) -> Result<()> {
        ensure!(
            paths.len() <= crate::paths::MAX_PATHS,
            "A project has too many paths"
        );
        let mut path_ids = HashSet::new();
        let mut anchor_ids = HashSet::new();
        for path in &paths {
            ensure!(
                path_ids.insert(path.id),
                "Path ID {} is repeated",
                path.id.0
            );
            ensure!(
                (1..=crate::paths::MAX_ANCHORS).contains(&path.anchors.len()),
                "Path {} has no anchors or too many",
                path.id.0
            );
            for anchor in &path.anchors {
                ensure!(
                    anchor.finite(),
                    "Path {} has a coordinate that is not finite",
                    path.id.0
                );
                ensure!(
                    anchor_ids.insert(anchor.id),
                    "Anchor ID {} is repeated",
                    anchor.id.0
                );
            }
        }
        if let Some(active) = active {
            ensure!(
                paths.iter().any(|path| path.id == active),
                "The active path is missing"
            );
        }
        self.paths = paths;
        self.active_path = active;
        Ok(())
    }

    pub fn upsert_path(&mut self, path: crate::paths::VectorPath) -> bool {
        if path.anchors.is_empty()
            || path.anchors.len() > crate::paths::MAX_ANCHORS
            || !path.anchors.iter().all(|anchor| anchor.finite())
        {
            return false;
        }
        let mut anchors = HashSet::new();
        for anchor in &path.anchors {
            if !anchors.insert(anchor.id) {
                return false;
            }
        }
        if let Some(index) = self.paths.iter().position(|stored| stored.id == path.id) {
            if self.paths[index] == path && self.active_path == Some(path.id) {
                return false;
            }
            if self.paths.iter().enumerate().any(|(stored_index, stored)| {
                stored_index != index
                    && stored
                        .anchors
                        .iter()
                        .any(|anchor| anchors.contains(&anchor.id))
            }) {
                return false;
            }
            self.paths[index] = path;
            self.active_path = Some(self.paths[index].id);
            return true;
        }
        if self.paths.len() >= crate::paths::MAX_PATHS {
            return false;
        }
        if self.paths.iter().any(|stored| {
            stored
                .anchors
                .iter()
                .any(|anchor| anchors.contains(&anchor.id))
        }) {
            return false;
        }
        self.active_path = Some(path.id);
        self.paths.push(path);
        true
    }

    pub fn clear_active_path(&mut self) -> bool {
        self.active_path.take().is_some()
    }

    pub fn select_path(&mut self, id: crate::paths::PathId) -> bool {
        if self.paths.iter().any(|path| path.id == id) {
            self.active_path = Some(id);
            true
        } else {
            false
        }
    }

    pub fn set_path_visible(&mut self, id: crate::paths::PathId, visible: bool) -> bool {
        let Some(path) = self.paths.iter_mut().find(|path| path.id == id) else {
            return false;
        };
        if path.visible == visible {
            return false;
        }
        path.visible = visible;
        true
    }

    pub fn delete_path(&mut self, id: crate::paths::PathId) -> bool {
        let Some(index) = self.paths.iter().position(|path| path.id == id) else {
            return false;
        };
        self.paths.remove(index);
        if self.active_path == Some(id) {
            self.active_path = self
                .paths
                .get(index)
                .or_else(|| self.paths.last())
                .map(|path| path.id);
        }
        true
    }

    pub fn close_path(&mut self, id: crate::paths::PathId) -> bool {
        let Some(path) = self.paths.iter_mut().find(|path| path.id == id) else {
            return false;
        };
        if path.closed || path.anchors.len() < 3 {
            return false;
        }
        path.closed = true;
        self.active_path = Some(id);
        true
    }

    pub fn delete_anchor(
        &mut self,
        path_id: crate::paths::PathId,
        anchor_id: crate::paths::AnchorId,
    ) -> bool {
        let Some(index) = self.paths.iter().position(|path| path.id == path_id) else {
            return false;
        };
        let Some(anchor_index) = self.paths[index]
            .anchors
            .iter()
            .position(|anchor| anchor.id == anchor_id)
        else {
            return false;
        };
        if self.paths[index].anchors.len() == 1 {
            self.paths.remove(index);
            if self.active_path == Some(path_id) {
                self.active_path = self.paths.last().map(|path| path.id);
            }
            return true;
        }
        self.paths[index].anchors.remove(anchor_index);
        if self.paths[index].anchors.len() < 3 {
            self.paths[index].closed = false;
        }
        self.active_path = Some(path_id);
        true
    }

    pub fn next_path_id(&self) -> Option<crate::paths::PathId> {
        let max = self.paths.iter().map(|path| path.id.0).max().unwrap_or(0);
        max.checked_add(1).map(crate::paths::PathId)
    }

    pub fn next_anchor_id(&self) -> Option<crate::paths::AnchorId> {
        let max = self
            .paths
            .iter()
            .flat_map(|path| path.anchors.iter().map(|anchor| anchor.id.0))
            .max()
            .unwrap_or(0);
        max.checked_add(1).map(crate::paths::AnchorId)
    }

    pub fn paths_from_selection(&self, selection: &Selection) -> Vec<crate::paths::VectorPath> {
        let (width, height) = self.size();
        let outlines = crate::paths::outlines_from_selection(selection, width, height);
        let mut next_path = self.next_path_id();
        let mut next_anchor = self.next_anchor_id();
        let mut paths = Vec::new();
        for mut anchors in outlines {
            if self.paths.len() + paths.len() >= crate::paths::MAX_PATHS {
                break;
            }
            let Some(id) = next_path else {
                break;
            };
            next_path = id.0.checked_add(1).map(crate::paths::PathId);
            let mut complete = true;
            for anchor in &mut anchors {
                let Some(anchor_id) = next_anchor else {
                    complete = false;
                    break;
                };
                next_anchor = anchor_id.0.checked_add(1).map(crate::paths::AnchorId);
                anchor.id = anchor_id;
            }
            if !complete {
                break;
            }
            paths.push(crate::paths::VectorPath {
                id,
                visible: true,
                closed: true,
                anchors,
            });
        }
        paths
    }

    fn map_paths(&mut self, mut map: impl FnMut((f64, f64)) -> (f64, f64)) {
        for path in &mut self.paths {
            for anchor in &mut path.anchors {
                anchor.point = map(anchor.point);
                anchor.handle_in = map(anchor.handle_in);
                anchor.handle_out = map(anchor.handle_out);
            }
        }
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
                if let Some(style) = layer.text.as_mut() {
                    style.color = mode.constrain(style.color);
                }
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

    pub fn layer_composite_dirty(&self, id: LayerId) -> Dirty {
        let Some(layer) = self.layer(id) else {
            return Dirty::None;
        };
        let canvas = PixelBounds {
            left: 0,
            top: 0,
            right: i64::from(self.width),
            bottom: i64::from(self.height),
        };
        let Some(bounds) = layer.bounds().intersect(canvas) else {
            return Dirty::Unpainted;
        };
        let (width, height) = bounds.dimensions();
        Dirty::Composite(PixelRect::new(bounds.left, bounds.top, width, height))
    }

    pub fn layer(&self, id: LayerId) -> Option<&RasterLayer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn layer_is_text(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|layer| layer.text.is_some())
    }

    pub fn has_text_layer(&self) -> bool {
        self.layers.iter().any(|layer| layer.text.is_some())
    }

    // Replaces a layer's pixels with a new rasterization and keeps the layout anchor.
    pub fn set_text(
        &mut self,
        id: LayerId,
        anchor: (i32, i32),
        rendered: crate::text::RenderedText,
    ) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        let pixels = if self.mode == crate::transform::ColorMode::Rgb {
            rendered.image
        } else {
            Arc::new(
                self.mode
                    .constrain_image(Arc::unwrap_or_clone(rendered.image)),
            )
        };
        let mut style = rendered.style;
        style.color = self.mode.constrain(style.color);
        let offset = style.offset_for(anchor);
        if self.layers[index].text.as_ref() == Some(&style)
            && self.layers[index].offset == offset
            && self.layers[index].pixels.as_ref() == pixels.as_ref()
        {
            return Dirty::None;
        }
        if !self.accepts_replacement(id, pixels.dimensions()) {
            return Dirty::None;
        }
        self.layers[index].pixels = pixels;
        self.layers[index].offset = offset;
        self.layers[index].text = Some(style);
        Dirty::Canvas
    }

    // Drops the text source and keeps the cached pixels. Callers record history
    // without rebuilding tiles, because the bitmap did not change.
    pub fn rasterize_text(&mut self, id: LayerId) -> bool {
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        if layer.text.is_none() {
            return false;
        }
        layer.text = None;
        true
    }

    pub fn insert_text_layer(
        &mut self,
        above: LayerId,
        anchor: (i32, i32),
        rendered: crate::text::RenderedText,
    ) -> Option<LayerId> {
        if !self.can_add_layer() {
            return None;
        }
        let pixels = if self.mode == crate::transform::ColorMode::Rgb {
            rendered.image
        } else {
            Arc::new(
                self.mode
                    .constrain_image(Arc::unwrap_or_clone(rendered.image)),
            )
        };
        let bytes = image_bytes(&pixels)?;
        if !self.fits_stack(0, bytes) {
            return None;
        }
        let id = self.next_layer_id()?;
        let mut style = rendered.style;
        style.color = self.mode.constrain(style.color);
        let offset = style.offset_for(anchor);
        let index = self
            .layer_index(above)
            .map_or(self.layers.len(), |index| index + 1);
        self.layers.insert(
            index,
            RasterLayer {
                id,
                name: "Text".into(),
                pixels,
                offset,
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
                text: Some(style),
            },
        );
        Some(id)
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
        self.paths.is_empty()
            && self.guides.is_empty()
            && self
                .full_canvas_layer()
                .is_some_and(|layer| layer.name == IMPORTED_LAYER_NAME && layer.text.is_none())
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
        if self.has_text_layer() {
            return false;
        }
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
        let origin = (f64::from(crop.x), f64::from(crop.y));
        self.map_paths(|point| (point.0 - origin.0, point.1 - origin.1));
        self.guides = self
            .guides
            .iter()
            .filter_map(|guide| {
                guide.translated(
                    (-i64::from(crop.x), -i64::from(crop.y)),
                    (crop.width, crop.height),
                )
            })
            .collect();
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
                text: None,
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
        let offset = crate::geometry::aligned_offset(
            (0, 0),
            layer.pixels.width(),
            layer.pixels.height(),
            self.width,
            self.height,
            crate::geometry::Alignment::Both,
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
                text: None,
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

    pub fn change_opacity(&mut self, id: LayerId, opacity: u8) -> Dirty {
        if !self.set_opacity(id, opacity) {
            return Dirty::None;
        }
        if self.layer(id).is_some_and(|layer| !layer.visible) {
            return Dirty::Unpainted;
        }
        self.layer_composite_dirty(id)
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
        if !lower.visible || !upper.visible || lower.text.is_some() || upper.text.is_some() {
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
        if self.layers[index].text.is_some() || self.layers[index + 1].text.is_some() {
            return None;
        }
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
    #[cfg(test)]
    pub fn fill(
        &mut self,
        id: LayerId,
        x: u32,
        y: u32,
        color: Rgba<u8>,
        selection: &Selection,
        tolerance: u8,
    ) -> Option<PixelRect> {
        self.fill_bucket(
            id,
            x,
            y,
            color,
            selection,
            FillRequest {
                mode: FillMode::Connected,
                sample_all: false,
                tolerance,
            },
        )
    }

    // Connected fill compares the active layer unless Sample All Layers is set, in which case
    // the composite decides the region and only the active layer is written. Selection mode
    // paints every covered pixel of that layer.
    pub fn fill_bucket(
        &mut self,
        id: LayerId,
        x: u32,
        y: u32,
        color: Rgba<u8>,
        selection: &Selection,
        request: FillRequest,
    ) -> Option<PixelRect> {
        if self.layer_is_text(id) || x >= self.width || y >= self.height {
            return None;
        }
        let canvas = (self.width, self.height);
        if !selection.covers(i64::from(x), i64::from(y), canvas.0, canvas.1) {
            return None;
        }
        let color = self.mode.constrain(color);
        let connected = (request.sample_all && request.mode == FillMode::Connected).then(|| {
            Selection::flood_within(&self.composite(), x, y, request.tolerance, selection)
        });
        let layer = self.layers.iter_mut().find(|layer| layer.id == id)?;
        let origin_x = i64::from(layer.offset.0);
        let origin_y = i64::from(layer.offset.1);
        let coverage = |px: u32, py: u32| {
            selection.coverage(
                origin_x + i64::from(px),
                origin_y + i64::from(py),
                canvas.0,
                canvas.1,
            )
        };
        let pixels = layer.pixels.clone();
        let painted = match request.mode {
            FillMode::Selection => paint_covered(&pixels, color, coverage)?,
            FillMode::Connected => {
                if let Some(connected) = &connected {
                    paint_covered(&pixels, color, |px, py| {
                        let cx = origin_x + i64::from(px);
                        let cy = origin_y + i64::from(py);
                        if connected.covers(cx, cy, canvas.0, canvas.1) {
                            coverage(px, py)
                        } else {
                            0
                        }
                    })?
                } else {
                    let local_x = i64::from(x) - origin_x;
                    let local_y = i64::from(y) - origin_y;
                    if !(0..i64::from(pixels.width())).contains(&local_x)
                        || !(0..i64::from(pixels.height())).contains(&local_y)
                    {
                        return None;
                    }
                    let local_x = local_x as u32;
                    let local_y = local_y as u32;
                    let target = *pixels.get_pixel(local_x, local_y);
                    if request.tolerance == 0 && target == color {
                        return None;
                    }
                    let included = |px, py| {
                        within_tolerance(*pixels.get_pixel(px, py), target, request.tolerance)
                    };
                    bucket_fill(&pixels, local_x, local_y, color, coverage, included)?
                }
            }
        };
        let (pixels, left, top, right, bottom) = painted;
        layer.pixels = Arc::new(pixels);
        Some(PixelRect::new(
            origin_x + i64::from(left),
            origin_y + i64::from(top),
            right - left + 1,
            bottom - top + 1,
        ))
    }

    pub fn apply_gradient(
        &mut self,
        id: LayerId,
        mut gradient: crate::paint::Gradient,
        selection: &Selection,
    ) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        if self.layers[index].text.is_some() {
            return Dirty::None;
        }
        gradient.foreground = self.mode.constrain(gradient.foreground);
        gradient.background = self.mode.constrain(gradient.background);
        let offset = self.layers[index].offset;
        let Some((pixels, left, top, right, bottom)) = render_gradient(
            &self.layers[index].pixels,
            offset,
            gradient,
            selection,
            (self.width, self.height),
        ) else {
            return Dirty::None;
        };
        self.layers[index].pixels = Arc::new(pixels);
        Dirty::Region(PixelRect::new(
            i64::from(offset.0) + i64::from(left),
            i64::from(offset.1) + i64::from(top),
            right - left + 1,
            bottom - top + 1,
        ))
    }

    pub fn apply_adjustment(
        &mut self,
        id: LayerId,
        adjustment: crate::adjustments::Adjustment,
        selection: &Selection,
    ) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        if self.layers[index].text.is_some() {
            return Dirty::None;
        }
        let offset = self.layers[index].offset;
        let canvas = (self.width, self.height);
        let mode = self.mode;
        let Some(adjusted) = crate::adjustments::apply_pixels(
            &self.layers[index].pixels,
            adjustment,
            |x, y| {
                selection.coverage(
                    i64::from(offset.0) + i64::from(x),
                    i64::from(offset.1) + i64::from(y),
                    canvas.0,
                    canvas.1,
                )
            },
            |pixel| mode.constrain(pixel),
        ) else {
            return Dirty::None;
        };
        self.layers[index].pixels = Arc::new(adjusted.pixels);
        Dirty::Region(PixelRect::new(
            i64::from(offset.0) + i64::from(adjusted.left),
            i64::from(offset.1) + i64::from(adjusted.top),
            adjusted.right - adjusted.left + 1,
            adjusted.bottom - adjusted.top + 1,
        ))
    }

    pub fn apply_filter(
        &mut self,
        id: LayerId,
        spec: crate::filters::FilterSpec,
        selection: &Selection,
    ) -> Dirty {
        if self.layer_is_text(id) || selection == &Selection::Empty {
            return Dirty::None;
        }
        let (radius, filtered) = match spec {
            crate::filters::FilterSpec::Blur { radius } => (radius, None),
            crate::filters::FilterSpec::Sharpen { radius, amount } => {
                if radius == 0 || amount == 0 {
                    return Dirty::None;
                }
                (radius, Some(amount))
            }
            crate::filters::FilterSpec::Shadow { .. } => return Dirty::None,
        };
        if radius == 0 {
            return Dirty::None;
        }
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        let offset = self.layers[index].offset;
        let canvas = (self.width, self.height);
        let mode = self.mode;
        let source = Arc::clone(&self.layers[index].pixels);
        let filtered = match filtered {
            None => crate::filters::gaussian_blur(&source, radius),
            Some(amount) => crate::filters::sharpen(&source, radius, amount),
        };
        let Some(adjusted) = crate::filters::apply_covered(
            &source,
            &filtered,
            |x, y| {
                selection.coverage(
                    i64::from(offset.0) + i64::from(x),
                    i64::from(offset.1) + i64::from(y),
                    canvas.0,
                    canvas.1,
                )
            },
            |pixel| mode.constrain(pixel),
        ) else {
            return Dirty::None;
        };
        self.layers[index].pixels = Arc::new(adjusted.pixels);
        Dirty::Region(PixelRect::new(
            i64::from(offset.0) + i64::from(adjusted.left),
            i64::from(offset.1) + i64::from(adjusted.top),
            adjusted.right - adjusted.left + 1,
            adjusted.bottom - adjusted.top + 1,
        ))
    }

    pub fn place_drop_shadow(
        &mut self,
        id: LayerId,
        spec: crate::filters::FilterSpec,
    ) -> Result<PixelRect, crate::filters::FilterStop> {
        if self.layer_is_text(id) {
            return Err(crate::filters::FilterStop::Unchanged);
        }
        let crate::filters::FilterSpec::Shadow {
            radius,
            offset,
            opacity,
            color,
        } = spec
        else {
            return Err(crate::filters::FilterStop::Unchanged);
        };
        if opacity == 0 {
            return Err(crate::filters::FilterStop::Unchanged);
        }
        let Some(index) = self.layer_index(id) else {
            return Err(crate::filters::FilterStop::Unchanged);
        };
        let rect = self
            .drop_shadow_bounds(id, radius, offset)
            .ok_or(crate::filters::FilterStop::TooLarge)?;
        let color = self.mode.constrain(color);
        let Some(shadow) =
            crate::filters::render_shadow(&self.layers[index].pixels, radius, opacity, color)
        else {
            return Err(crate::filters::FilterStop::Unchanged);
        };
        let pixels = self.mode.constrain_image(shadow.pixels);
        let layer_offset = (rect.x as i32, rect.y as i32);
        let Some(shadow_id) = self.next_layer_id() else {
            return Err(crate::filters::FilterStop::TooLarge);
        };
        let rect = PixelRect::new(
            i64::from(layer_offset.0),
            i64::from(layer_offset.1),
            pixels.width(),
            pixels.height(),
        );
        self.layers.insert(
            index,
            RasterLayer {
                id: shadow_id,
                name: "Drop Shadow".into(),
                pixels: Arc::new(pixels),
                offset: layer_offset,
                visible: true,
                opacity: 255,
                blend_mode: BlendMode::Normal,
                text: None,
            },
        );
        Ok(rect)
    }

    pub fn drop_shadow_fits(&self, id: LayerId, radius: u32, offset: (i32, i32)) -> bool {
        self.drop_shadow_bounds(id, radius, offset).is_some()
    }

    fn drop_shadow_bounds(
        &self,
        id: LayerId,
        radius: u32,
        offset: (i32, i32),
    ) -> Option<PixelRect> {
        let layer = self.layer(id)?;
        let (width, height) =
            crate::filters::shadow_dimensions(layer.pixels.width(), layer.pixels.height(), radius)?;
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(4)?;
        if !self.can_add_layer() || !self.fits_stack(0, bytes) {
            return None;
        }
        let pad = i64::from(crate::filters::kernel_support(radius));
        let x = i64::from(layer.offset.0) - pad + i64::from(offset.0);
        let y = i64::from(layer.offset.1) - pad + i64::from(offset.1);
        i32::try_from(x).ok()?;
        i32::try_from(y).ok()?;
        Some(PixelRect::new(x, y, width, height))
    }

    pub fn composite_stroke_tile(
        &self,
        id: LayerId,
        stroke: &crate::paint::Stroke,
        rect: PixelRect,
    ) -> RgbaImage {
        let bounds = PixelBounds {
            left: rect.x,
            top: rect.y,
            right: rect.right(),
            bottom: rect.bottom(),
        };
        composite::flatten_with(&self.layers, bounds, |layer, x, y| {
            if layer.id == id
                && let Some(pixel) = stroke.pixel(x, y)
            {
                return pixel;
            }
            *layer.pixels.get_pixel(x, y)
        })
    }

    pub fn apply_stroke(&mut self, id: LayerId, stroke: &crate::paint::Stroke) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        if self.layers[index].text.is_some() {
            return Dirty::None;
        }
        let Some((pixels, mut regions)) =
            crate::paint::apply_stroke(&self.layers[index].pixels, stroke)
        else {
            return Dirty::None;
        };
        let offset = self.layers[index].offset;
        self.layers[index].pixels = Arc::new(pixels);
        for rect in &mut regions {
            rect.x += i64::from(offset.0);
            rect.y += i64::from(offset.1);
        }
        match regions.as_slice() {
            [rect] => Dirty::Region(*rect),
            _ => Dirty::Regions(regions),
        }
    }

    // Paints canvas-space polylines onto one layer. Points move by the layer offset.
    // The brush is not limited by a selection: the stroke is the contour itself.
    #[cfg(test)]
    pub fn stroke_lines(
        &mut self,
        id: LayerId,
        lines: &[Vec<(f64, f64)>],
        color: Rgba<u8>,
        brush: crate::paint::Brush,
    ) -> Dirty {
        self.stroke_lines_kind(id, lines, color, crate::paint::StrokeKind::Paint, brush)
    }

    pub fn stroke_lines_kind(
        &mut self,
        id: LayerId,
        lines: &[Vec<(f64, f64)>],
        color: Rgba<u8>,
        kind: crate::paint::StrokeKind,
        brush: crate::paint::Brush,
    ) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        if self.layers[index].text.is_some() {
            return Dirty::None;
        }
        let brush = brush.clamped();
        if lines.is_empty() || brush.opacity == 0 {
            return Dirty::None;
        }
        let offset = self.layers[index].offset;
        let pixels = Arc::clone(&self.layers[index].pixels);
        let mut stroke = crate::paint::Stroke::new(kind, self.mode.constrain(color), brush);
        for line in lines {
            let local = line
                .iter()
                .map(|(x, y)| (*x - f64::from(offset.0), *y - f64::from(offset.1)))
                .collect::<Vec<_>>();
            stroke.trace(&local, &pixels);
        }
        self.apply_stroke(id, &stroke)
    }

    // Sets covered pixels to transparent. Pixels that are already clear do not count as a change.
    pub fn clear(&mut self, id: LayerId, selection: &Selection) -> Option<PixelRect> {
        if self.layer_is_text(id) {
            return None;
        }
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
        if !layer.visible || layer.opacity == 0 {
            return Dirty::Unpainted;
        }
        let canvas = PixelBounds {
            left: 0,
            top: 0,
            right: i64::from(self.width),
            bottom: i64::from(self.height),
        };
        let bounds = match (before.intersect(canvas), after.intersect(canvas)) {
            (Some(before), Some(after)) => before.union(after),
            (Some(bounds), None) | (None, Some(bounds)) => bounds,
            (None, None) => return Dirty::Unpainted,
        };
        let (width, height) = bounds.dimensions();
        Dirty::Composite(PixelRect::new(bounds.left, bounds.top, width, height))
    }

    pub fn accepts_replacement(&self, id: LayerId, size: (u32, u32)) -> bool {
        let Some(index) = self.layer_index(id) else {
            return false;
        };
        let Some(added) = (u64::from(size.0) * u64::from(size.1)).checked_mul(4) else {
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
        if self.layers[index].text.is_some() {
            return Dirty::None;
        }
        let pixels = self.mode.constrain_image(pixels);
        if self.layers[index].offset == offset && self.layers[index].pixels.as_ref() == &pixels {
            return Dirty::None;
        }
        if !self.accepts_replacement(id, pixels.dimensions()) {
            return Dirty::None;
        }
        self.layers[index].pixels = Arc::new(pixels);
        self.layers[index].offset = offset;
        Dirty::Canvas
    }

    pub fn align_layer(&mut self, id: LayerId, alignment: crate::geometry::Alignment) -> Dirty {
        let Some(layer) = self.layer(id) else {
            return Dirty::None;
        };
        let offset = crate::geometry::aligned_offset(
            layer.offset,
            layer.pixels.width(),
            layer.pixels.height(),
            self.width,
            self.height,
            alignment,
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
        if self.layer_is_text(id) || !self.can_fit_layer(id) {
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
        if self.layer_is_text(id) {
            return LayerGeometry::Unchanged;
        }
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
        self.composite_partition(index, index + 1)
    }

    pub fn composite_at_insertion(&self, above: LayerId) -> (Option<RgbaImage>, Option<RgbaImage>) {
        let index = self
            .layer_index(above)
            .map_or(self.layers.len(), |index| index + 1);
        self.composite_partition(index, index)
    }

    fn composite_partition(
        &self,
        below: usize,
        above: usize,
    ) -> (Option<RgbaImage>, Option<RgbaImage>) {
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
            composite(&self.layers[..below]),
            composite(&self.layers[above..]),
        )
    }

    pub fn turn_image(&mut self, turn: crate::transform::Turn) -> bool {
        if self.has_text_layer() {
            return false;
        }
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
        self.map_paths(|point| crate::paths::turn_point(point, canvas, turn));
        self.guides
            .iter_mut()
            .for_each(|guide| *guide = guide.turned(canvas, turn));
        (self.width, self.height) = crate::transform::turned_size(canvas.0, canvas.1, turn);
        true
    }

    pub fn mirror_image(&mut self, mirror: crate::transform::Mirror) -> bool {
        if self.has_text_layer() {
            return false;
        }
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
        self.map_paths(|point| crate::paths::mirror_point(point, canvas, mirror));
        self.guides
            .iter_mut()
            .for_each(|guide| *guide = guide.mirrored(canvas, mirror));
        true
    }

    pub fn turn_layer(&mut self, id: LayerId, turn: crate::transform::Turn) -> bool {
        if self.layer_is_text(id) {
            return false;
        }
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
        if self.layer_is_text(id) {
            return false;
        }
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
        let Some(plan) = self.image_scale_plan(width, height) else {
            return false;
        };
        for (layer, placement) in self.layers.iter_mut().zip(plan) {
            layer.pixels = Arc::new(crate::transform::scale_pixels(
                &layer.pixels,
                placement.size.0,
                placement.size.1,
                resample,
            ));
            layer.offset = placement.offset;
        }
        self.map_paths(|point| crate::paths::scale_point(point, old, (width, height)));
        self.guides
            .iter_mut()
            .for_each(|guide| *guide = guide.scaled(old, (width, height)));
        self.width = width;
        self.height = height;
        true
    }

    // Validate every layer and the full stack before allocating any resized pixels.
    fn image_scale_plan(&self, width: u32, height: u32) -> Option<Vec<ScaledLayer>> {
        let old = self.size();
        if self.has_text_layer()
            || (width, height) == old
            || !crate::transform::canvas_allowed(width, height)
        {
            return None;
        }
        if self
            .paths
            .iter()
            .flat_map(|path| &path.anchors)
            .any(|anchor| {
                [anchor.point, anchor.handle_in, anchor.handle_out]
                    .into_iter()
                    .any(|point| {
                        !crate::paths::finite(crate::paths::scale_point(
                            point,
                            old,
                            (width, height),
                        ))
                    })
            })
        {
            return None;
        }
        let mut plan = Vec::with_capacity(self.layers.len());
        let mut total = 0u64;
        for layer in &self.layers {
            let (offset, size) = crate::transform::scale_placement(
                layer.offset,
                layer.pixels.dimensions(),
                old,
                (width, height),
            )?;
            let bytes = u64::from(size.0)
                .checked_mul(u64::from(size.1))
                .and_then(|pixels| pixels.checked_mul(4))?;
            if bytes > MAX_IMAGE_BYTES {
                return None;
            }
            total = total.checked_add(bytes)?;
            if total > MAX_STACK_BYTES {
                return None;
            }
            plan.push(ScaledLayer { offset, size });
        }
        Some(plan)
    }

    pub fn scale_layer(
        &mut self,
        id: LayerId,
        width: u32,
        height: u32,
        resample: crate::transform::Resample,
    ) -> bool {
        if self.layer_is_text(id) || width == 0 || height == 0 {
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
        let shift = (f64::from(delta.0), f64::from(delta.1));
        self.map_paths(|point| crate::paths::translate_point(point, shift));
        self.guides = self
            .guides
            .iter()
            .filter_map(|guide| {
                guide.translated((i64::from(delta.0), i64::from(delta.1)), (width, height))
            })
            .collect();
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    Connected,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillRequest {
    pub mode: FillMode,
    pub sample_all: bool,
    pub tolerance: u8,
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

fn paint_covered(
    source: &RgbaImage,
    color: Rgba<u8>,
    coverage: impl Fn(u32, u32) -> u8,
) -> Option<(RgbaImage, u32, u32, u32, u32)> {
    let (width, height) = source.dimensions();
    let mut image = None;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let amount = coverage(x, y);
            if amount == 0 {
                continue;
            }
            let before = *source.get_pixel(x, y);
            let after = mix_fill(before, color, amount);
            if before == after {
                continue;
            }
            image
                .get_or_insert_with(|| source.clone())
                .put_pixel(x, y, after);
            bounds = Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    let (min_x, min_y, max_x, max_y) = bounds?;
    Some((image?, min_x, min_y, max_x, max_y))
}

pub(crate) fn gradient_preview(
    source: &RgbaImage,
    offset: (i32, i32),
    gradient: crate::paint::Gradient,
    selection: &Selection,
    canvas: (u32, u32),
    max_edge: u32,
) -> RgbaImage {
    let (width, height) = source.dimensions();
    let (preview_width, preview_height) = crate::adjustments::preview_size(width, height, max_edge);
    RgbaImage::from_fn(preview_width, preview_height, |x, y| {
        let sx = ((u64::from(x) * u64::from(width)) / u64::from(preview_width)) as u32;
        let sy = ((u64::from(y) * u64::from(height)) / u64::from(preview_height)) as u32;
        let cx = i64::from(offset.0) + i64::from(sx);
        let cy = i64::from(offset.1) + i64::from(sy);
        let before = *source.get_pixel(sx, sy);
        let amount = selection.coverage(cx, cy, canvas.0, canvas.1);
        if amount == 0 {
            return before;
        }
        let color = crate::paint::gradient_color(&gradient, cx as f64 + 0.5, cy as f64 + 0.5);
        mix_fill(before, color, amount)
    })
}

pub(crate) fn render_gradient(
    source: &RgbaImage,
    offset: (i32, i32),
    gradient: crate::paint::Gradient,
    selection: &Selection,
    canvas: (u32, u32),
) -> Option<(RgbaImage, u32, u32, u32, u32)> {
    let (width, height) = source.dimensions();
    let mut image = None;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let canvas_x = i64::from(offset.0) + i64::from(x);
            let canvas_y = i64::from(offset.1) + i64::from(y);
            let amount = selection.coverage(canvas_x, canvas_y, canvas.0, canvas.1);
            if amount == 0 {
                continue;
            }
            let color = crate::paint::gradient_color(
                &gradient,
                canvas_x as f64 + 0.5,
                canvas_y as f64 + 0.5,
            );
            let before = *source.get_pixel(x, y);
            let after = mix_fill(before, color, amount);
            if before == after {
                continue;
            }
            image
                .get_or_insert_with(|| source.clone())
                .put_pixel(x, y, after);
            bounds = Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    let (min_x, min_y, max_x, max_y) = bounds?;
    Some((image?, min_x, min_y, max_x, max_y))
}

fn bucket_fill(
    source: &RgbaImage,
    x: u32,
    y: u32,
    color: Rgba<u8>,
    coverage: impl Fn(u32, u32) -> u8,
    included: impl Fn(u32, u32) -> bool,
) -> Option<(RgbaImage, u32, u32, u32, u32)> {
    if coverage(x, y) == 0 || !included(x, y) {
        return None;
    }
    let (width, height) = source.dimensions();
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut visited = vec![false; width as usize * height as usize];
    let mut pending = vec![(x, y)];
    let mut image = None;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (x, y, x, y);
    let paintable =
        |visited: &[bool], x, y| !visited[index(x, y)] && included(x, y) && coverage(x, y) > 0;

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
    fn antialiased_selection_coverage_is_shared_by_fill_and_clear() {
        let selection = Selection::ellipse_rect_quality(PixelRect::new(0, 0, 8, 8), 8, 8, true);
        let mut content = DocumentContent::single_layer("Soft oval".into(), RgbaImage::new(8, 8));
        let id = content.top_layer_id();
        let ink = Rgba([220, 40, 10, 255]);
        assert!(
            content
                .fill_bucket(
                    id,
                    4,
                    4,
                    ink,
                    &selection,
                    FillRequest {
                        mode: FillMode::Selection,
                        sample_all: false,
                        tolerance: 0
                    }
                )
                .is_some()
        );
        let coverage = selection.coverage(1, 0, 8, 8);
        assert!((1..255).contains(&coverage));
        assert_eq!(
            *content.layer(id).unwrap().pixels.get_pixel(1, 0),
            Rgba([220, 40, 10, coverage])
        );
        assert_eq!(*content.layer(id).unwrap().pixels.get_pixel(4, 4), ink);
        assert!(content.clear(id, &selection).is_some());
        assert_eq!(content.layer(id).unwrap().pixels.get_pixel(4, 4).0[3], 0);
        let expected = ((u16::from(coverage) * u16::from(255 - coverage) + 127) / 255) as u8;
        assert_eq!(
            content.layer(id).unwrap().pixels.get_pixel(1, 0).0[3],
            expected
        );
    }

    #[test]
    fn image_operations_transform_guides_and_guides_require_project_saving() {
        use crate::guides::{Axis, Guide, MAX_GUIDES};
        use crate::transform::{Anchor, Mirror, Resample, Turn};
        let mut content =
            DocumentContent::single_layer(IMPORTED_LAYER_NAME.into(), RgbaImage::new(10, 6));
        assert!(content.is_plain_raster());
        let guides = vec![
            Guide {
                axis: Axis::Vertical,
                position: 3,
            },
            Guide {
                axis: Axis::Horizontal,
                position: 2,
            },
        ];
        content.set_guides(guides.clone()).unwrap();
        assert!(!content.is_plain_raster());
        assert!(
            content
                .set_guides(vec![Guide {
                    axis: Axis::Vertical,
                    position: 11
                }])
                .is_err()
        );
        assert!(content.set_guides(vec![guides[0]; MAX_GUIDES + 1]).is_err());
        assert_eq!(content.guides(), guides);
        assert!(content.turn_image(Turn::Right));
        assert_eq!(
            content.guides(),
            &[
                Guide {
                    axis: Axis::Horizontal,
                    position: 3
                },
                Guide {
                    axis: Axis::Vertical,
                    position: 4
                }
            ]
        );
        assert!(content.mirror_image(Mirror::Vertical));
        assert_eq!(content.guides()[0].position, 7);
        assert!(content.scale_image(12, 20, Resample::Nearest));
        assert_eq!(content.guides()[0].position, 14);
        assert_eq!(content.guides()[1].position, 8);
        content.resize_canvas(16, 24, Anchor::Center).unwrap();
        assert_eq!(content.guides()[0].position, 16);
        assert_eq!(content.guides()[1].position, 10);
        assert!(content.crop(CropRect {
            x: 2,
            y: 10,
            width: 10,
            height: 10
        }));
        assert_eq!(
            content.guides(),
            &[
                Guide {
                    axis: Axis::Horizontal,
                    position: 6
                },
                Guide {
                    axis: Axis::Vertical,
                    position: 8
                }
            ]
        );
        assert!(content.crop(CropRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }));
        assert!(content.guides().is_empty());
    }

    #[test]
    fn bucket_fill_crosses_hidden_rgb_on_clear_pixels_and_respects_a_soft_selection() {
        let pixels = RgbaImage::from_fn(4, 1, |x, _| match x {
            0 => Rgba([255, 0, 0, 0]),
            1 => Rgba([0, 255, 0, 0]),
            _ => Rgba([0, 0, 255, 255]),
        });
        let blue = Rgba([0, 0, 255, 255]);
        let ink = Rgba([255, 255, 0, 255]);
        for sample_all in [false, true] {
            let mut content = DocumentContent::single_layer("Transparent".into(), pixels.clone());
            let selection = Selection::Full.feather(1, 4, 1);
            assert!(
                content
                    .fill_bucket(
                        LayerId(0),
                        0,
                        0,
                        ink,
                        &selection,
                        FillRequest {
                            mode: FillMode::Connected,
                            sample_all,
                            tolerance: 0
                        }
                    )
                    .is_some()
            );
            let image = &content.layer(LayerId(0)).unwrap().pixels;
            for x in 0..2 {
                assert_eq!(&image.get_pixel(x, 0).0[..3], &[255, 255, 0]);
                assert!(image.get_pixel(x, 0).0[3] > 0 && image.get_pixel(x, 0).0[3] < 255);
            }
            assert_eq!(*image.get_pixel(2, 0), blue);
            assert_eq!(*image.get_pixel(3, 0), blue);
        }
    }

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
            text: None,
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
                text: None,
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
        let (filled, left, top, right, bottom) = bucket_fill(
            &source,
            1,
            1,
            color,
            |_, _| 255,
            |x, y| within_tolerance(*source.get_pixel(x, y), *source.get_pixel(1, 1), 1),
        )
        .unwrap();
        assert_eq!((left, top, right, bottom), (0, 0, 2, 2));
        assert!(filled.pixels().all(|pixel| *pixel == color));
    }

    #[test]
    fn a_live_eraser_tile_matches_the_committed_layer_stack() {
        use crate::paint::{Brush, Stroke, StrokeKind};

        let mut content = DocumentContent::single_layer(
            "Base".into(),
            RgbaImage::from_pixel(12, 12, Rgba([30, 80, 120, 150])),
        );
        let active = content
            .add_layer(LayerId(0), "Paint".into(), Rgba([250, 10, 20, 210]))
            .unwrap();
        let top = content
            .add_layer(active, "Top".into(), Rgba([10, 200, 0, 90]))
            .unwrap();
        content.set_offset(active, (-2, 1));
        content.set_opacity(active, 140);
        content.set_offset(top, (3, -1));
        let mut stroke = Stroke::new(
            StrokeKind::Erase,
            Rgba([0; 4]),
            Brush {
                diameter: 4,
                ..Brush::default()
            },
        );
        stroke.sample(5.5, 4.5, &content.layer(active).unwrap().pixels, |_, _| 128);
        let bounds = PixelRect::new(2, 1, 7, 7);
        let live = content.composite_stroke_tile(active, &stroke, bounds);
        assert_ne!(
            live,
            content.composite_rect(bounds.x, bounds.y, bounds.width, bounds.height)
        );
        assert_ne!(content.apply_stroke(active, &stroke), Dirty::None);
        assert_eq!(
            live,
            content.composite_rect(bounds.x, bounds.y, bounds.width, bounds.height)
        );
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
        let (filled, left, top, right, bottom) = bucket_fill(
            &image,
            0,
            0,
            ink,
            |_, _| 255,
            |x, y| *image.get_pixel(x, y) == blank,
        )
        .unwrap();
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
        assert_eq!(content.set_offset(id, (2, 3)), Dirty::Unpainted);
        assert_eq!(content.layers[0].offset, (2, 3));
        assert_eq!(
            content.align_layer(id, crate::geometry::Alignment::HorizontalCenter),
            Dirty::Unpainted
        );
        assert_eq!(content.layers[0].offset, (0, 3));
        assert_eq!(content.set_offset(id, (0, 3)), Dirty::None);
        assert_eq!(
            content.set_offset(id, (0, 0)),
            Dirty::Composite(PixelRect::new(0, 0, 4, 2))
        );
        content.set_offset(id, (2, 1));
        assert_eq!(
            content.set_offset(id, (3, 1)),
            Dirty::Composite(PixelRect::new(2, 1, 2, 1))
        );
        content.set_visible(id, false);
        assert_eq!(content.set_offset(id, (-1, 0)), Dirty::Unpainted);
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

    fn red_blue_gradient(shape: crate::paint::GradientShape) -> crate::paint::Gradient {
        crate::paint::Gradient {
            shape,
            blend: crate::paint::GradientBlend::ForegroundToBackground,
            start: (0.5, 0.5),
            end: (3.5, 0.5),
            foreground: Rgba([255, 0, 0, 255]),
            background: Rgba([0, 0, 255, 255]),
        }
    }

    #[test]
    fn a_gradient_honors_its_ends_a_zero_length_drag_and_an_offset_mask() {
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(4, 1));
        let id = content.top_layer_id();
        let open = Selection::Unrestricted;
        assert!(matches!(
            content.apply_gradient(
                id,
                red_blue_gradient(crate::paint::GradientShape::Linear),
                &open
            ),
            Dirty::Region(_)
        ));
        let pixels = content.layers[0].pixels.clone();
        assert_eq!(pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(3, 0).0, [0, 0, 255, 255]);

        let mut collapsed = red_blue_gradient(crate::paint::GradientShape::Radial);
        collapsed.end = collapsed.start;
        collapsed.blend = crate::paint::GradientBlend::ForegroundToTransparent;
        content.layers[0].pixels = Arc::new(RgbaImage::from_pixel(4, 1, Rgba([0, 255, 0, 255])));
        assert!(matches!(
            content.apply_gradient(id, collapsed, &open),
            Dirty::Region(_)
        ));
        assert!(
            content.layers[0]
                .pixels
                .pixels()
                .all(|pixel| pixel.0 == [255, 0, 0, 255])
        );

        content.layers[0].pixels = Arc::new(RgbaImage::new(4, 1));
        content.layers[0].offset = (2, 0);
        content.width = 8;
        let selection = Selection::rectangle((3, 0), (3, 0), 8, 1).feather(1, 8, 1);
        assert!(matches!(
            content.apply_gradient(
                id,
                red_blue_gradient(crate::paint::GradientShape::Linear),
                &selection
            ),
            Dirty::Region(_)
        ));
        let painted = content.layers[0].pixels.get_pixel(1, 0).0;
        assert!(painted[3] > 0 && painted[3] < 255);
        assert_eq!(content.layers[0].pixels.get_pixel(3, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn sample_all_connects_through_canvas_pixels_outside_the_active_layer() {
        let red = Rgba([255, 0, 0, 255]);
        let blue = Rgba([0, 0, 255, 255]);
        let mut content =
            DocumentContent::single_layer("Base".into(), RgbaImage::from_pixel(3, 3, red));
        let active = content.add_layer(LayerId(0), "Strip".into(), red).unwrap();
        content.layers[1].pixels =
            Arc::new(RgbaImage::from_fn(
                3,
                1,
                |x, _| {
                    if x == 1 { blue } else { red }
                },
            ));
        let original = content.clone();
        let base = content.layers[0].pixels.clone();
        let request = FillRequest {
            mode: FillMode::Connected,
            sample_all: true,
            tolerance: 0,
        };
        content
            .fill_bucket(active, 0, 2, blue, &Selection::Unrestricted, request)
            .unwrap();
        assert_eq!(*content.layers[1].pixels.get_pixel(2, 0), blue);
        assert!(Arc::ptr_eq(&base, &content.layers[0].pixels));
        let mut content = original;
        let selection = Selection::Rect(PixelRect::new(0, 0, 3, 1));
        content
            .fill_bucket(active, 0, 0, blue, &selection, request)
            .unwrap();
        assert_eq!(*content.layers[1].pixels.get_pixel(2, 0), red);
    }

    #[test]
    fn shadow_offsets_are_checked_and_clear_sources_create_no_layer() {
        let mut content = DocumentContent::single_layer("Clear".into(), RgbaImage::new(2, 2));
        let id = content.top_layer_id();
        let spec = crate::filters::FilterSpec::Shadow {
            radius: 1,
            offset: (20, 0),
            opacity: 75,
            color: Rgba([0, 0, 0, 255]),
        };
        assert!(content.place_drop_shadow(id, spec).is_err());
        assert_eq!(content.layers.len(), 1);
        content.layers[0].offset = (i32::MAX, 0);
        assert!(!content.drop_shadow_fits(id, 1, (20, 0)));
        assert!(content.place_drop_shadow(id, spec).is_err());
        assert_eq!(content.layers.len(), 1);
    }

    #[test]
    fn gradient_previews_are_bounded_and_match_full_resolution_sample_positions() {
        let source = RgbaImage::from_pixel(1024, 4, Rgba([20, 30, 40, 255]));
        let mut gradient = red_blue_gradient(crate::paint::GradientShape::Linear);
        gradient.end = (1023.5, 0.5);
        let selection = Selection::Rect(PixelRect::new(256, 0, 256, 4));
        let preview = gradient_preview(&source, (0, 0), gradient, &selection, (1024, 4), 64);
        let rendered = render_gradient(&source, (0, 0), gradient, &selection, (1024, 4))
            .unwrap()
            .0;
        assert_eq!(preview.dimensions(), (64, 1));
        for x in 0..64 {
            assert_eq!(preview.get_pixel(x, 0), rendered.get_pixel(x * 16, 0));
        }
    }

    #[test]
    fn sample_all_layers_changes_the_connected_region_without_editing_the_other_layer() {
        let mut content = DocumentContent::single_layer(
            "Bottom".into(),
            RgbaImage::from_pixel(3, 1, Rgba([255, 0, 0, 255])),
        );
        let top = content
            .add_layer(LayerId(0), "Top".into(), Rgba([255, 0, 0, 255]))
            .unwrap();
        content.layers[1].pixels = Arc::new(RgbaImage::from_fn(3, 1, |x, _| {
            if x == 1 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([255, 0, 0, 255])
            }
        }));
        let bottom = content.layers[0].pixels.clone();
        let ink = Rgba([0, 0, 255, 255]);
        let open = Selection::Unrestricted;
        let connected = FillRequest {
            mode: FillMode::Connected,
            sample_all: false,
            tolerance: 0,
        };
        assert_eq!(
            content.fill_bucket(top, 0, 0, ink, &open, connected),
            Some(PixelRect::new(0, 0, 1, 1))
        );
        assert_eq!(content.layers[1].pixels.get_pixel(2, 0).0, [255, 0, 0, 255]);

        content.layers[1].pixels = Arc::new(RgbaImage::from_fn(3, 1, |x, _| {
            if x == 1 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([255, 0, 0, 255])
            }
        }));
        let sampled = FillRequest {
            sample_all: true,
            ..connected
        };
        assert_eq!(
            content.fill_bucket(top, 0, 0, ink, &open, sampled),
            Some(PixelRect::new(0, 0, 3, 1))
        );
        assert!(content.layers[1].pixels.pixels().all(|pixel| *pixel == ink));
        assert!(Arc::ptr_eq(&bottom, &content.layers[0].pixels));

        content.layers[1].pixels =
            Arc::new(RgbaImage::from_fn(4, 1, |x, _| Rgba([x as u8, 0, 0, 255])));
        content.width = 4;
        content.layers[0].pixels = Arc::new(RgbaImage::from_pixel(4, 1, Rgba([255, 0, 0, 255])));
        let bottom = content.layers[0].pixels.clone();
        let selection = Selection::Rect(PixelRect::new(0, 0, 3, 1));
        assert_eq!(
            content.fill_bucket(
                top,
                0,
                0,
                ink,
                &selection,
                FillRequest {
                    mode: FillMode::Selection,
                    sample_all: true,
                    tolerance: 0,
                },
            ),
            Some(PixelRect::new(0, 0, 3, 1))
        );
        assert_eq!(content.layers[1].pixels.get_pixel(3, 0).0[0], 3);
        assert!(Arc::ptr_eq(&bottom, &content.layers[0].pixels));
    }

    #[test]
    fn an_adjustment_follows_an_offset_mask_and_leaves_an_identity_unrecorded() {
        let mut content = DocumentContent::single_layer(
            "Layer".into(),
            RgbaImage::from_pixel(3, 1, Rgba([10, 20, 30, 255])),
        );
        content.layers[0].offset = (2, 1);
        content.width = 6;
        content.height = 3;
        let id = content.top_layer_id();
        let selection = Selection::Rect(PixelRect::new(3, 1, 1, 1));
        assert!(matches!(
            content.apply_adjustment(
                id,
                crate::adjustments::Adjustment::BrightnessContrast {
                    brightness: 0,
                    contrast: 0,
                },
                &selection,
            ),
            Dirty::None
        ));
        assert!(matches!(
            content.apply_adjustment(id, crate::adjustments::Adjustment::Invert, &selection),
            Dirty::Region(PixelRect {
                x: 3,
                y: 1,
                width: 1,
                height: 1
            })
        ));
        let pixels = &content.layers[0].pixels;
        assert_eq!(pixels.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert_eq!(pixels.get_pixel(1, 0).0, [245, 235, 225, 255]);
        assert_eq!(pixels.get_pixel(2, 0).0, [10, 20, 30, 255]);
        content.mode = crate::transform::ColorMode::Grayscale;
        assert!(matches!(
            content.apply_adjustment(id, crate::adjustments::Adjustment::Invert, &selection),
            Dirty::Region(PixelRect {
                x: 3,
                y: 1,
                width: 1,
                height: 1
            })
        ));
        assert_eq!(
            content.layers[0].pixels.get_pixel(1, 0).0,
            [18, 18, 18, 255]
        );
        assert_eq!(
            content.layers[0].pixels.get_pixel(0, 0).0,
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn a_levels_lut_follows_an_offset_mask_and_stays_gray() {
        use crate::adjustments::{LevelStops, Levels, MapChannel};

        let mut content = DocumentContent::single_layer(
            "Layer".into(),
            RgbaImage::from_pixel(3, 1, Rgba([10, 20, 30, 255])),
        );
        content.layers[0].offset = (2, 1);
        content.width = 6;
        content.height = 3;
        let id = content.top_layer_id();
        let selection = Selection::Rect(PixelRect::new(3, 1, 1, 1));
        let invert = Levels::default().set_stops(
            MapChannel::Value,
            LevelStops::IDENTITY
                .set_output_black(255)
                .set_output_white(0),
        );
        assert!(matches!(
            content.apply_adjustment(
                id,
                crate::adjustments::Adjustment::Levels(Levels::default()),
                &selection
            ),
            Dirty::None
        ));
        assert!(matches!(
            content.apply_adjustment(
                id,
                crate::adjustments::Adjustment::Levels(invert),
                &selection
            ),
            Dirty::Region(PixelRect {
                x: 3,
                y: 1,
                width: 1,
                height: 1
            })
        ));
        assert_eq!(
            content.layers[0].pixels.get_pixel(1, 0).0,
            [245, 235, 225, 255]
        );
        assert_eq!(
            content.layers[0].pixels.get_pixel(0, 0).0,
            [10, 20, 30, 255]
        );
        content.mode = crate::transform::ColorMode::Grayscale;
        content.apply_adjustment(
            id,
            crate::adjustments::Adjustment::Levels(invert),
            &selection,
        );
        assert_eq!(
            content.layers[0].pixels.get_pixel(1, 0).0,
            [18, 18, 18, 255]
        );
        assert_eq!(
            content.layers[0].pixels.get_pixel(2, 0).0,
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn a_blur_follows_an_offset_mask_and_grayscale_stays_gray() {
        use crate::filters::FilterSpec;

        let mut content = DocumentContent::single_layer(
            "Layer".into(),
            RgbaImage::from_fn(3, 1, |x, _| {
                if x == 1 {
                    Rgba([255, 0, 0, 255])
                } else {
                    Rgba([0, 0, 0, 255])
                }
            }),
        );
        content.layers[0].offset = (2, 1);
        content.width = 8;
        content.height = 4;
        let id = content.top_layer_id();
        let selection = Selection::Rect(PixelRect::new(3, 1, 1, 1));
        assert!(matches!(
            content.apply_filter(id, FilterSpec::Blur { radius: 0 }, &selection),
            Dirty::None
        ));
        assert!(matches!(
            content.apply_filter(id, FilterSpec::Blur { radius: 1 }, &selection),
            Dirty::Region(_)
        ));
        assert_eq!(content.layers[0].pixels.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_ne!(content.layers[0].pixels.get_pixel(1, 0).0, [255, 0, 0, 255]);
        content.mode = crate::transform::ColorMode::Grayscale;
        content.apply_filter(id, FilterSpec::Blur { radius: 1 }, &Selection::Unrestricted);
        let pixel = content.layers[0].pixels.get_pixel(1, 0).0;
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
    }

    #[test]
    fn a_drop_shadow_lands_below_its_layer_and_round_trips() {
        use std::io::Cursor;

        use crate::filters::{FilterSpec, FilterStop};

        let mut pixels = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 0]));
        pixels.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let mut content = DocumentContent::single_layer("Layer".into(), pixels);
        content.layers[0].offset = (2, 1);
        let id = content.top_layer_id();
        let spec = FilterSpec::Shadow {
            radius: 0,
            offset: (1, 0),
            opacity: 100,
            color: Rgba([0, 0, 40, 255]),
        };
        assert_eq!(
            content.place_drop_shadow(id, spec).unwrap(),
            PixelRect::new(3, 1, 2, 1)
        );
        assert_eq!(content.layers[0].name, "Drop Shadow");
        assert_eq!(content.layers[0].offset, (3, 1));
        assert_eq!(content.layers[0].pixels.get_pixel(0, 0).0, [0, 0, 40, 255]);
        assert_eq!(content.layers[1].id, id);
        assert_eq!(content.layers[1].pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
        let mut bytes = Vec::new();
        crate::project::write(&mut bytes, &content).unwrap();
        let read = crate::project::read(Cursor::new(bytes)).unwrap();
        assert_eq!(read.layers[0].name, "Drop Shadow");
        assert_eq!(read.layers[0].offset, (3, 1));
        assert_eq!(read.layers[0].pixels.get_pixel(0, 0).0, [0, 0, 40, 255]);
        assert_eq!(read.layers[1].pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);

        content.mode = crate::transform::ColorMode::Grayscale;
        let gray = content
            .place_drop_shadow(
                id,
                FilterSpec::Shadow {
                    radius: 0,
                    offset: (0, 0),
                    opacity: 100,
                    color: Rgba([255, 0, 0, 255]),
                },
            )
            .unwrap();
        assert_eq!(gray, PixelRect::new(2, 1, 2, 1));
        let pixel = content
            .layers
            .iter()
            .find(|layer| layer.offset == (2, 1))
            .unwrap()
            .pixels
            .get_pixel(0, 0)
            .0;
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);

        while content.can_add_layer() {
            content
                .add_layer(id, "Fill".into(), Rgba([0, 0, 0, 0]))
                .unwrap();
        }
        assert_eq!(
            content.place_drop_shadow(id, spec),
            Err(FilterStop::TooLarge)
        );
    }

    #[test]
    fn selection_paths_use_fresh_ids_and_become_active() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};

        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(12, 10));
        assert!(content.upsert_path(VectorPath {
            id: PathId(4),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor::corner(AnchorId(7), (0.0, 0.0)),
                Anchor::corner(AnchorId(8), (1.0, 1.0)),
            ],
        }));
        let selection = Selection::rectangle((2, 2), (8, 7), 12, 10);
        let paths = content.paths_from_selection(&selection);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].id, PathId(5));
        assert!(paths[0].anchors.iter().all(|anchor| anchor.id.0 > 8));
        for path in paths {
            assert!(content.upsert_path(path));
        }
        assert_eq!(content.paths().len(), 2);
        assert_eq!(content.active_path_id(), Some(PathId(5)));
        assert!(content.paths_from_selection(&Selection::Empty).is_empty());
        assert!(
            content
                .paths_from_selection(&Selection::Unrestricted)
                .is_empty()
        );
    }

    #[test]
    fn canvas_transforms_move_path_points_with_the_image() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};
        use crate::transform::{Anchor as CanvasAnchor, Mirror, Turn};

        let mut content = DocumentContent::single_layer(
            "Layer".into(),
            RgbaImage::from_pixel(4, 6, Rgba([0, 0, 0, 255])),
        );
        let path = VectorPath {
            id: PathId(1),
            visible: true,
            closed: false,
            anchors: vec![Anchor {
                id: AnchorId(1),
                point: (1.0, 2.0),
                handle_in: (0.0, 2.0),
                handle_out: (1.0, 3.0),
            }],
        };
        assert!(content.upsert_path(path));
        assert!(content.turn_image(Turn::Left));
        let turned = &content.paths()[0].anchors[0];
        assert_eq!(turned.point, (2.0, 3.0));
        assert_eq!(turned.handle_in, (2.0, 4.0));
        assert_eq!(turned.handle_out, (3.0, 3.0));
        assert_eq!(content.size(), (6, 4));

        assert!(content.mirror_image(Mirror::Horizontal));
        assert_eq!(content.paths()[0].anchors[0].point, (4.0, 3.0));

        assert!(content.scale_image(12, 8, crate::transform::Resample::Nearest));
        assert_eq!(content.paths()[0].anchors[0].point, (8.0, 6.0));

        content
            .resize_canvas(12, 10, CanvasAnchor::NorthWest)
            .unwrap();
        assert_eq!(content.paths()[0].anchors[0].point, (8.0, 6.0));
        content
            .resize_canvas(10, 10, CanvasAnchor::SouthEast)
            .unwrap();
        assert_eq!(content.paths()[0].anchors[0].point, (6.0, 6.0));

        assert!(content.crop(CropRect {
            x: 1,
            y: 2,
            width: 4,
            height: 4,
        }));
        assert_eq!(content.paths()[0].anchors[0].point, (5.0, 4.0));
    }

    fn hairline() -> crate::paint::Brush {
        crate::paint::Brush {
            diameter: 1,
            spacing: 100,
            hardness: 100,
            opacity: 100,
        }
    }

    #[test]
    fn an_open_path_omits_the_return_and_a_closed_path_paints_it() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};

        let color = Rgba([255, 0, 0, 255]);
        let open = VectorPath {
            id: PathId(1),
            visible: true,
            closed: false,
            anchors: vec![
                Anchor::corner(AnchorId(1), (2.0, 2.0)),
                Anchor::corner(AnchorId(2), (8.0, 2.0)),
                Anchor::corner(AnchorId(3), (8.0, 8.0)),
            ],
        };
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(12, 12));
        assert!(content.upsert_path(open));
        let line = crate::paths::stroke(content.active_path().unwrap());
        assert_ne!(
            content.stroke_lines(LayerId(0), &[line], color, hairline()),
            Dirty::None
        );
        let pixels = &content.layers[0].pixels;
        assert_eq!(pixels.get_pixel(5, 2).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(8, 5).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(2, 5).0[3], 0);
        assert_eq!(pixels.get_pixel(5, 8).0[3], 0);

        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(12, 12));
        assert!(content.upsert_path(VectorPath {
            id: PathId(2),
            visible: true,
            closed: true,
            anchors: vec![
                Anchor::corner(AnchorId(4), (2.0, 2.0)),
                Anchor::corner(AnchorId(5), (8.0, 2.0)),
                Anchor::corner(AnchorId(6), (8.0, 8.0)),
                Anchor::corner(AnchorId(7), (2.0, 8.0)),
            ],
        }));
        let line = crate::paths::stroke(content.active_path().unwrap());
        assert_ne!(
            content.stroke_lines(LayerId(0), &[line], color, hairline()),
            Dirty::None
        );
        let pixels = &content.layers[0].pixels;
        assert_eq!(pixels.get_pixel(2, 5).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(5, 8).0, [255, 0, 0, 255]);
    }

    #[test]
    fn a_layer_offset_moves_the_stroke_into_layer_pixels() {
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(12, 12));
        content.layers[0].offset = (3, 1);
        let dirty = content.stroke_lines(
            LayerId(0),
            &[vec![(5.0, 2.0), (5.0, 8.0)]],
            Rgba([255, 0, 0, 255]),
            hairline(),
        );
        assert_ne!(dirty, Dirty::None);
        let pixels = &content.layers[0].pixels;
        assert_eq!(pixels.get_pixel(2, 4).0, [255, 0, 0, 255]);
        assert_eq!(pixels.get_pixel(5, 4).0[3], 0);
    }

    #[test]
    fn a_hole_adds_a_second_contour_and_the_brush_paints_outside_the_selection() {
        use crate::selection::Selection;

        let outer = [(1.0, 1.0), (15.0, 1.0), (15.0, 15.0), (1.0, 15.0)];
        let hole = [(5.0, 11.0), (11.0, 11.0), (11.0, 5.0), (5.0, 5.0)];
        let donut = Selection::polygons(&[&outer, &hole], 16, 16);
        assert!(donut.covers(3, 8, 16, 16));
        assert!(!donut.covers(8, 8, 16, 16));
        let lines = donut.stroke_polylines(16, 16);
        assert_eq!(lines.len(), 2);
        let color = Rgba([255, 0, 0, 255]);
        let paint = |line: &[Vec<(f64, f64)>]| {
            let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(16, 16));
            assert_ne!(
                content.stroke_lines(LayerId(0), line, color, hairline()),
                Dirty::None
            );
            std::sync::Arc::clone(&content.layers[0].pixels)
        };
        let both = paint(&lines);
        let first = paint(std::slice::from_ref(&lines[0]));
        let second = paint(std::slice::from_ref(&lines[1]));
        assert_eq!(both.get_pixel(8, 1).0, [255, 0, 0, 255]);
        assert_eq!(both.get_pixel(8, 5).0, [255, 0, 0, 255]);
        assert!(!donut.covers(8, 5, 16, 16));
        assert_eq!(both.get_pixel(8, 8).0[3], 0);
        assert_eq!(both.get_pixel(8, 3).0[3], 0);
        assert_ne!(first.as_raw(), second.as_raw());

        let selection = Selection::rectangle((4, 4), (10, 10), 16, 16);
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(16, 16));
        let wide = crate::paint::Brush {
            diameter: 3,
            ..hairline()
        };
        assert_ne!(
            content.stroke_lines(LayerId(0), &selection.stroke_polylines(16, 16), color, wide,),
            Dirty::None
        );
        assert!(
            (0..16).any(|x| content.layers[0].pixels.get_pixel(x, 3).0[3] > 0),
            "the brush should cross the selection edge"
        );
        assert!(!selection.covers(0, 3, 16, 16));
    }

    #[test]
    fn zero_opacity_does_not_paint_and_grayscale_constrains_the_stroke() {
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(8, 8));
        let line = vec![(1.0, 1.0), (5.0, 1.0)];
        let color = Rgba([255, 0, 0, 255]);
        let clear = crate::paint::Brush {
            opacity: 0,
            ..hairline()
        };
        assert_eq!(
            content.stroke_lines(LayerId(0), std::slice::from_ref(&line), color, clear),
            Dirty::None
        );
        assert_eq!(
            content.stroke_lines(LayerId(9), std::slice::from_ref(&line), color, hairline()),
            Dirty::None
        );
        assert_eq!(content.layers[0].pixels.get_pixel(2, 1).0[3], 0);

        content.mode = crate::transform::ColorMode::Grayscale;
        assert_ne!(
            content.stroke_lines(LayerId(0), &[line], color, hairline()),
            Dirty::None
        );
        assert_eq!(
            content.layers[0].pixels.get_pixel(2, 1).0,
            [77, 77, 77, 255]
        );
    }

    #[test]
    fn text_pixels_match_the_composite_and_pixel_tools_leave_them_alone() {
        let rendered =
            crate::text::render("Hi", "sans-serif", 28.0, Rgba([255, 0, 0, 255])).unwrap();
        let (width, height) = rendered.image.dimensions();
        let mut content =
            DocumentContent::single_layer("Text".into(), RgbaImage::new(width, height));
        let id = content.top_layer_id();
        let anchor = rendered.style.origin;
        assert_ne!(content.set_text(id, anchor, rendered.clone()), Dirty::None);
        assert_eq!(content.layer(id).unwrap().offset, (0, 0));
        assert_eq!(content.composite().as_ref(), rendered.image.as_ref());
        let before = content.layer(id).unwrap().pixels.clone();
        assert!(
            content
                .fill(
                    id,
                    0,
                    0,
                    Rgba([0, 255, 0, 255]),
                    &Selection::Unrestricted,
                    0
                )
                .is_none()
        );
        assert!(!content.turn_image(crate::transform::Turn::Right));
        assert_eq!(content.layer(id).unwrap().pixels.as_ref(), before.as_ref());
        assert!(content.layer_is_text(id));
        content.set_color_mode(crate::transform::ColorMode::Grayscale);
        assert_eq!(
            content.layer(id).unwrap().text.as_ref().unwrap().color.0,
            [77, 77, 77, 255]
        );
    }

    #[test]
    fn raster_save_cannot_drop_paths_or_cached_text() {
        use crate::paths::{Anchor, AnchorId, PathId, VectorPath};
        let mut content = DocumentContent::single_layer("Image".into(), RgbaImage::new(2, 2));
        assert!(content.is_plain_raster());
        let mut with_path = content.clone();
        assert!(with_path.upsert_path(VectorPath {
            id: PathId(0),
            visible: true,
            closed: false,
            anchors: vec![Anchor::corner(AnchorId(0), (0.0, 0.0))]
        }));
        assert!(!with_path.is_plain_raster());
        content.layers[0].text = Some(crate::text::TextStyle {
            source: "Cached text".into(),
            family: "Missing Family".into(),
            resolved_family: "Fallback".into(),
            size: 24.0,
            color: Rgba([0, 0, 0, 255]),
            spacing: crate::text::Spacing::default(),
            face: crate::text::FontFace::default(),
            origin: (0, 0),
        });
        assert!(!content.is_plain_raster());
        let layer = &content.layers[0];
        let cached = crate::text::RenderedText {
            image: layer.pixels.clone(),
            style: layer.text.clone().unwrap(),
        };
        assert!(cached.matches(&crate::text::TextSpec {
            source: "Cached text".into(),
            family: "Missing Family".into(),
            size: 24.0,
            color: Rgba([0, 0, 0, 255]),
            spacing: crate::text::Spacing::default(),
            face: crate::text::FontFace::default(),
        }));
        assert_eq!(content.set_text(LayerId(0), (0, 0), cached), Dirty::None);
        assert!(content.rasterize_text(LayerId(0)));
        assert!(content.is_plain_raster());
    }

    #[test]
    fn insertion_preview_uses_the_same_layer_order_as_apply() {
        let mut content = DocumentContent::single_layer(
            "Base".into(),
            RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])),
        );
        let middle = content
            .add_layer(LayerId(0), "Middle".into(), Rgba([0, 255, 0, 255]))
            .unwrap();
        let upper = content
            .add_layer(middle, "Upper".into(), Rgba([0, 0, 255, 255]))
            .unwrap();
        let (below, above) = content.composite_at_insertion(middle);
        assert_eq!(below.unwrap().get_pixel(0, 0)[1], 255);
        assert_eq!(above.unwrap().get_pixel(0, 0)[2], 255);
        assert!(content.composite_at_insertion(upper).1.is_none());
        content.layers[1].text = Some(crate::text::TextStyle {
            source: "Hi".into(),
            family: "sans-serif".into(),
            resolved_family: "sans-serif".into(),
            size: 24.0,
            color: Rgba([0, 0, 0, 255]),
            spacing: crate::text::Spacing::default(),
            face: crate::text::FontFace::default(),
            origin: (0, 0),
        });
        assert!(!content.can_merge_down(middle));
        assert!(!content.can_merge_down(upper));
    }

    #[test]
    fn image_scaling_distinguishes_the_layer_and_stack_budgets() {
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(1, 1));
        let layer = content.layers[0].clone();
        content.layers = (0..MAX_LAYERS)
            .map(|index| {
                let mut layer = layer.clone();
                layer.id = LayerId(index as u64);
                layer
            })
            .collect();
        // This plans exactly 512 MiB across 128 small layers without allocating
        // the output bitmaps. Each layer is well below the 256 MiB image limit.
        let plan = content.image_scale_plan(1024, 1024).unwrap();
        assert_eq!(plan.len(), MAX_LAYERS);
        assert!(plan.iter().all(|placement| placement.size == (1024, 1024)));
        assert!(content.image_scale_plan(1025, 1025).is_none());

        let mut content = DocumentContent::single_layer("Wide layer".into(), RgbaImage::new(2, 1));
        content.width = 1;
        // A legal canvas must still reject an individual scaled layer over 256 MiB.
        assert!(content.image_scale_plan(8192, 8192).is_none());
    }

    #[test]
    fn image_scaling_cannot_leave_nonfinite_paths_in_a_saveable_project() {
        let mut content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(1, 1));
        content
            .set_paths(
                vec![crate::paths::VectorPath {
                    id: crate::paths::PathId(1),
                    visible: true,
                    closed: false,
                    anchors: vec![crate::paths::Anchor::corner(
                        crate::paths::AnchorId(1),
                        (f64::MAX, 0.),
                    )],
                }],
                Some(crate::paths::PathId(1)),
            )
            .unwrap();
        let pixels = content.layers[0].pixels.clone();
        assert!(!content.scale_image(2, 1, crate::transform::Resample::Nearest));
        assert_eq!(content.size(), (1, 1));
        assert!(Arc::ptr_eq(&pixels, &content.layers[0].pixels));
        assert!(content.paths[0].anchors[0].finite());

        content.width = 4;
        assert!(content.scale_image(2, 1, crate::transform::Resample::Nearest));
        assert_eq!(content.paths[0].anchors[0].point.0, f64::MAX / 2.);
        let mut bytes = Vec::new();
        crate::project::write(&mut bytes, &content).unwrap();
        assert!(crate::project::read(std::io::Cursor::new(bytes)).is_ok());
    }
}
