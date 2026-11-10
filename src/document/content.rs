use std::sync::Arc;

use image::{Rgba, RgbaImage, imageops};

use super::composite;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq)]
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

    pub fn layer(&self, id: LayerId) -> Option<&RasterLayer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn top_layer_id(&self) -> LayerId {
        self.layers
            .last()
            .expect("documents have at least one layer")
            .id
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

    pub fn composite(&self) -> Arc<RgbaImage> {
        if let Some(layer) = self.full_canvas_layer() {
            return layer.pixels.clone();
        }
        Arc::new(composite::flatten(self))
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

    pub fn fill(&mut self, id: LayerId, x: u32, y: u32, color: Rgba<u8>) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let Some(layer) = self.layers.iter_mut().find(|layer| layer.id == id) else {
            return false;
        };
        let x = i64::from(x) - i64::from(layer.offset.0);
        let y = i64::from(y) - i64::from(layer.offset.1);
        if !(0..i64::from(layer.pixels.width())).contains(&x)
            || !(0..i64::from(layer.pixels.height())).contains(&y)
        {
            return false;
        }
        let Some(pixels) = bucket_fill(&layer.pixels, x as u32, y as u32, color) else {
            return false;
        };
        layer.pixels = Arc::new(pixels);
        true
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

fn bucket_fill(source: &RgbaImage, x: u32, y: u32, color: Rgba<u8>) -> Option<RgbaImage> {
    let target = *source.get_pixel(x, y);
    if target == color {
        return None;
    }
    let mut image = source.clone();

    // Paint runs as they are queued, so a pixel is never queued twice.
    let height = image.height();
    let mut pending = vec![paint_run(&mut image, x, y, target, color)];
    while let Some((left, right, y)) = pending.pop() {
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
    Some(image)
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

        assert!(!content.fill(LayerId(1), 0, 0, ink));
        assert!(content.fill(LayerId(1), 1, 0, ink));
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
        let filled = bucket_fill(&image, 0, 0, ink).unwrap();
        assert_eq!(filled.into_raw(), [ink.0, wall.0, wall.0, blank.0].concat());
    }
}
