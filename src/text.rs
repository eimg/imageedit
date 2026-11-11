use std::sync::{Arc, Mutex};

use image::{Rgba, RgbaImage};

use crate::document::{MAX_IMAGE_BYTES, MAX_NEW_EDGE};

pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_FAMILY_CHARS: usize = 128;
pub const MIN_PIXEL_SIZE: f32 = 1.0;
pub const MAX_PIXEL_SIZE: f32 = 256.0;

// GPUI can lay out a line, but rasterize_glyph is crate-private. cosmic-text
// shapes with HarfRust and returns coverage through SwashCache.
static FONTS: Mutex<Option<(cosmic_text::FontSystem, cosmic_text::SwashCache)>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub source: String,
    pub family: String,
    pub size: f32,
    pub color: Rgba<u8>,
    pub resolved_family: String,
    // Layout origin inside the bitmap. The canvas anchor is layer offset plus this.
    pub origin: (i32, i32),
}

impl TextStyle {
    pub fn substituted(&self) -> bool {
        !is_generic(&self.family) && !self.family.eq_ignore_ascii_case(&self.resolved_family)
    }

    pub fn anchor(&self, offset: (i32, i32)) -> (i32, i32) {
        (
            offset.0.saturating_add(self.origin.0),
            offset.1.saturating_add(self.origin.1),
        )
    }

    pub fn offset_for(&self, anchor: (i32, i32)) -> (i32, i32) {
        (
            anchor.0.saturating_sub(self.origin.0),
            anchor.1.saturating_sub(self.origin.1),
        )
    }
}

#[derive(Clone, Debug)]
pub struct RenderedText {
    pub image: Arc<RgbaImage>,
    pub style: TextStyle,
}

impl RenderedText {
    pub fn matches(&self, source: &str, family: &str, size: f32, color: Rgba<u8>) -> bool {
        self.style.source == source
            && self.style.family == family
            && self.style.size == size
            && self.style.color == color
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextError {
    TooLong,
    Family,
    Size,
    TooLarge,
}

impl TextError {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooLong => "Text is limited to 4000 characters.",
            Self::Family => "The font name is too long.",
            Self::Size => "Text size must be between 1 and 256 pixels.",
            Self::TooLarge => "The text is too large to rasterize.",
        }
    }
}

pub fn is_generic(family: &str) -> bool {
    matches!(
        family.trim().to_ascii_lowercase().as_str(),
        "sans-serif" | "sans" | "serif" | "monospace" | "mono"
    )
}

pub fn normalize_family(family: &str) -> String {
    match family.trim().to_ascii_lowercase().as_str() {
        "sans-serif" | "sans" => "sans-serif".to_owned(),
        "serif" => "serif".to_owned(),
        "monospace" | "mono" => "monospace".to_owned(),
        _ => family.trim().to_owned(),
    }
}

pub fn fields_allowed(source: &str, family: &str, resolved: &str, size: f32) -> bool {
    source.chars().count() <= MAX_TEXT_CHARS
        && source
            .chars()
            .all(|ch| ch == '\n' || ch == '\t' || !ch.is_control())
        && family.chars().count() <= MAX_FAMILY_CHARS
        && resolved.chars().count() <= MAX_FAMILY_CHARS
        && family.chars().all(|ch| !ch.is_control())
        && resolved.chars().all(|ch| !ch.is_control())
        && size.is_finite()
        && (MIN_PIXEL_SIZE..=MAX_PIXEL_SIZE).contains(&size)
}

pub fn render(
    source: &str,
    family: &str,
    pixel_size: f32,
    color: Rgba<u8>,
) -> Result<RenderedText, TextError> {
    if !pixel_size.is_finite() || !(MIN_PIXEL_SIZE..=MAX_PIXEL_SIZE).contains(&pixel_size) {
        return Err(TextError::Size);
    }
    if source.chars().count() > MAX_TEXT_CHARS
        || source
            .chars()
            .any(|ch| ch != '\n' && ch != '\t' && ch.is_control())
    {
        return Err(TextError::TooLong);
    }
    let family = normalize_family(family);
    if family.chars().count() > MAX_FAMILY_CHARS || family.chars().any(|ch| ch.is_control()) {
        return Err(TextError::Family);
    }
    with_fonts(|font_system, cache| {
        if source.is_empty() {
            let (resolved, _) = resolve_family(font_system, &family, None);
            return Ok(rendered(
                RgbaImage::new(1, 1),
                source,
                family,
                pixel_size,
                color,
                resolved,
                (0, 0),
            ));
        }
        let metrics = cosmic_text::Metrics::new(pixel_size, (pixel_size * 1.2).max(pixel_size));
        let mut buffer = cosmic_text::Buffer::new(font_system, metrics);
        buffer.set_wrap(cosmic_text::Wrap::None);
        buffer.set_size(None, None);
        let named = family.clone();
        let attrs = cosmic_text::Attrs::new().family(family_attr(&named));
        buffer.set_text(source, &attrs, cosmic_text::Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);

        let mut line_right = 0i32;
        let mut line_bottom = 0i32;
        let mut used_font = None;
        for run in buffer.layout_runs() {
            if !run.line_w.is_finite()
                || !run.line_top.is_finite()
                || !run.line_height.is_finite()
                || run.line_w > MAX_NEW_EDGE as f32
                || run.line_top + run.line_height > MAX_NEW_EDGE as f32
            {
                return Err(TextError::TooLarge);
            }
            line_right = line_right.max(span_end(run.line_w));
            line_bottom = line_bottom.max(span_end(run.line_top + run.line_height));
            if used_font.is_none() {
                used_font = run
                    .glyphs
                    .iter()
                    .find(|glyph| glyph.glyph_id != 0)
                    .map(|glyph| glyph.font_id);
            }
        }

        let mut left = 0i32;
        let mut top = 0i32;
        let mut right = line_right;
        let mut bottom = line_bottom;
        let mut too_big = false;
        let ink = cosmic_text::Color::rgba(color.0[0], color.0[1], color.0[2], 255);
        buffer.draw(font_system, cache, ink, |x, y, w, h, pixel| {
            if too_big || pixel.a() == 0 || w == 0 || h == 0 {
                return;
            }
            let far_x = x.saturating_add(i32::try_from(w).unwrap_or(i32::MAX) - 1);
            let far_y = y.saturating_add(i32::try_from(h).unwrap_or(i32::MAX) - 1);
            left = left.min(x.min(0));
            top = top.min(y.min(0));
            right = right.max(far_x);
            bottom = bottom.max(far_y);
            if right.saturating_sub(left) >= MAX_NEW_EDGE as i32
                || bottom.saturating_sub(top) >= MAX_NEW_EDGE as i32
            {
                too_big = true;
            }
        });
        if too_big {
            return Err(TextError::TooLarge);
        }
        let width = u32::try_from(right - left + 1).unwrap_or(u32::MAX).max(1);
        let height = u32::try_from(bottom - top + 1).unwrap_or(u32::MAX).max(1);
        if !image_allowed(width, height) {
            return Err(TextError::TooLarge);
        }
        let mut image = RgbaImage::new(width, height);
        buffer.draw(font_system, cache, ink, |x, y, w, h, pixel| {
            let coverage = pixel.a();
            if coverage == 0 || w == 0 || h == 0 {
                return;
            }
            let rgb = [pixel.r(), pixel.g(), pixel.b()];
            let columns = i32::try_from(w).unwrap_or(i32::MAX);
            let rows = i32::try_from(h).unwrap_or(i32::MAX);
            for row in 0..rows {
                for column in 0..columns {
                    let px = x.saturating_add(column).saturating_sub(left);
                    let py = y.saturating_add(row).saturating_sub(top);
                    if px < 0 || py < 0 {
                        continue;
                    }
                    let (Ok(px), Ok(py)) = (u32::try_from(px), u32::try_from(py)) else {
                        continue;
                    };
                    if px >= width || py >= height {
                        continue;
                    }
                    cover(image.get_pixel_mut(px, py), rgb, coverage, color.0[3]);
                }
            }
        });
        let (resolved, _) = resolve_family(font_system, &family, used_font);
        if resolved.chars().count() > MAX_FAMILY_CHARS {
            return Err(TextError::Family);
        }
        Ok(rendered(
            image,
            source,
            family,
            pixel_size,
            color,
            resolved,
            (-left, -top),
        ))
    })
}

fn rendered(
    image: RgbaImage,
    source: &str,
    family: String,
    pixel_size: f32,
    color: Rgba<u8>,
    resolved: String,
    origin: (i32, i32),
) -> RenderedText {
    RenderedText {
        image: Arc::new(image),
        style: TextStyle {
            source: source.to_owned(),
            family,
            size: pixel_size,
            color,
            resolved_family: resolved,
            origin,
        },
    }
}

pub(crate) fn substitution(style: &TextStyle) -> Option<String> {
    if is_generic(&style.family) {
        return None;
    }
    with_fonts(|fonts, _| {
        if canonical_family(fonts, &style.family).is_some() {
            style
                .substituted()
                .then(|| format!("Cached text uses {}.", style.resolved_family))
        } else {
            Some(format!(
                "Using sans-serif instead of {} when edited.",
                style.family
            ))
        }
    })
}

fn with_fonts<T>(
    draw: impl FnOnce(&mut cosmic_text::FontSystem, &mut cosmic_text::SwashCache) -> T,
) -> T {
    let mut fonts = FONTS.lock().unwrap_or_else(|poison| poison.into_inner());
    if fonts.is_none() {
        *fonts = Some((
            cosmic_text::FontSystem::new(),
            cosmic_text::SwashCache::new(),
        ));
    }
    let (font_system, cache) = fonts.as_mut().expect("the font system was just installed");
    draw(font_system, cache)
}

fn family_attr(family: &str) -> cosmic_text::fontdb::Family<'_> {
    match family {
        "sans-serif" => cosmic_text::fontdb::Family::SansSerif,
        "serif" => cosmic_text::fontdb::Family::Serif,
        "monospace" => cosmic_text::fontdb::Family::Monospace,
        _ => cosmic_text::fontdb::Family::Name(family),
    }
}

fn resolve_family(
    font_system: &mut cosmic_text::FontSystem,
    family: &str,
    used: Option<cosmic_text::fontdb::ID>,
) -> (String, bool) {
    if is_generic(family) {
        let resolved = used
            .and_then(|id| face_family(font_system, id))
            .unwrap_or_else(|| family.to_owned());
        return (resolved, false);
    }
    if let Some(canonical) = canonical_family(font_system, family) {
        return (canonical, false);
    }
    let resolved = used
        .and_then(|id| face_family(font_system, id))
        .unwrap_or_else(|| "sans-serif".to_owned());
    (resolved, true)
}

fn canonical_family(font_system: &mut cosmic_text::FontSystem, requested: &str) -> Option<String> {
    font_system.db_mut().faces().find_map(|face| {
        face.families
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(requested))
            .map(|(name, _)| name.clone())
    })
}

fn face_family(
    font_system: &mut cosmic_text::FontSystem,
    id: cosmic_text::fontdb::ID,
) -> Option<String> {
    font_system
        .db_mut()
        .face(id)
        .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
}

fn span_end(length: f32) -> i32 {
    if !length.is_finite() || length <= 0.0 {
        return 0;
    }
    length.ceil() as i32 - 1
}

fn image_allowed(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_NEW_EDGE
        && height <= MAX_NEW_EDGE
        && u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4)
            <= MAX_IMAGE_BYTES
}

fn cover(pixel: &mut Rgba<u8>, rgb: [u8; 3], coverage: u8, opacity: u8) {
    let source_a = ((u32::from(coverage) * u32::from(opacity) + 127) / 255) as u8;
    if source_a == 0 {
        return;
    }
    let dest_a = pixel.0[3];
    let inverse = 255 - u32::from(source_a);
    let out_a = (u32::from(source_a) + (u32::from(dest_a) * inverse + 127) / 255).min(255) as u8;
    if out_a == 0 {
        *pixel = Rgba([0, 0, 0, 0]);
        return;
    }
    let channel = |source: u8, dest: u8| -> u8 {
        let blended = u32::from(source) * u32::from(source_a)
            + u32::from(dest) * u32::from(dest_a) * inverse / 255;
        ((blended + u32::from(out_a) / 2) / u32::from(out_a)) as u8
    };
    *pixel = Rgba([
        channel(rgb[0], pixel.0[0]),
        channel(rgb[1], pixel.0[1]),
        channel(rgb[2], pixel.0[2]),
        out_a,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(image: &RgbaImage) -> usize {
        image.pixels().filter(|pixel| pixel.0[3] > 0).count()
    }

    fn max_alpha(image: &RgbaImage) -> u8 {
        image.pixels().map(|pixel| pixel.0[3]).max().unwrap_or(0)
    }

    #[test]
    fn a_larger_size_rasterizes_more_pixels() {
        let color = Rgba([0, 0, 0, 255]);
        let small = render("Ag", "sans-serif", 16.0, color).unwrap();
        let large = render("Ag", "sans-serif", 64.0, color).unwrap();
        assert!(large.image.width() > small.image.width());
        assert!(large.image.height() > small.image.height());
        assert!(ink(&large.image) > ink(&small.image));
        assert!(max_alpha(&large.image) >= 200);
        assert!(!large.style.substituted());
    }

    #[test]
    fn a_newline_makes_the_bitmap_taller() {
        let color = Rgba([0, 0, 0, 255]);
        let one = render("A", "sans-serif", 32.0, color).unwrap();
        let two = render("A\nA", "sans-serif", 32.0, color).unwrap();
        assert!(two.image.height() > one.image.height());
        assert!(max_alpha(&two.image) > 0);
    }

    #[test]
    fn a_missing_family_is_substituted_and_still_paints() {
        let rendered = render(
            "Ag",
            "ImageEdit Missing Font",
            32.0,
            Rgba([20, 40, 60, 255]),
        )
        .unwrap();
        assert!(rendered.style.substituted());
        assert!(
            !rendered
                .style
                .resolved_family
                .eq_ignore_ascii_case("ImageEdit Missing Font")
        );
        assert!(max_alpha(&rendered.image) > 0);
        assert_eq!(rendered.style.family, "ImageEdit Missing Font");
    }

    #[test]
    fn non_ascii_scripts_shape_and_paint() {
        for sample in ["café", "مرحبا", "文字"] {
            let rendered = render(sample, "sans-serif", 36.0, Rgba([0, 0, 0, 255])).unwrap();
            assert!(
                max_alpha(&rendered.image) > 0,
                "{sample} produced no coverage"
            );
        }
    }

    #[test]
    fn empty_text_is_a_transparent_pixel_and_rejects_bad_input() {
        let empty = render("", "serif", 24.0, Rgba([0, 0, 0, 255])).unwrap();
        assert_eq!(empty.image.dimensions(), (1, 1));
        assert_eq!(empty.image.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert!(!empty.style.substituted());
        assert!(matches!(
            render("A", "sans-serif", 0.0, Rgba([0; 4])),
            Err(TextError::Size)
        ));
        assert!(matches!(
            render(
                &"A".repeat(MAX_TEXT_CHARS + 1),
                "sans-serif",
                16.0,
                Rgba([0; 4])
            ),
            Err(TextError::TooLong)
        ));
    }
}
