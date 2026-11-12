use std::sync::{Arc, Mutex};

use image::{Rgba, RgbaImage};

use crate::document::{MAX_IMAGE_BYTES, MAX_NEW_EDGE};

pub const MAX_TEXT_CHARS: usize = 4_000;
pub const MAX_FAMILY_CHARS: usize = 128;
pub const MIN_PIXEL_SIZE: f32 = 1.0;
pub const MAX_PIXEL_SIZE: f32 = 256.0;
// Letter spacing is extra advance in canvas pixels after each glyph.
pub const MIN_LETTER_SPACING: f32 = -64.0;
pub const MAX_LETTER_SPACING: f32 = 256.0;
// Line spacing is the distance between baselines as a multiple of the size.
pub const MIN_LINE_SPACING: f32 = 0.5;
pub const MAX_LINE_SPACING: f32 = 4.0;
pub const DEFAULT_LINE_SPACING: f32 = 1.2;

// GPUI can lay out a line, but rasterize_glyph is crate-private. cosmic-text
// shapes with HarfRust and returns coverage through SwashCache.
static FONTS: Mutex<Option<(cosmic_text::FontSystem, cosmic_text::SwashCache)>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub letter: f32,
    pub line: f32,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            letter: 0.0,
            line: DEFAULT_LINE_SPACING,
        }
    }
}

impl Spacing {
    pub fn allowed(self) -> bool {
        self.letter.is_finite()
            && self.line.is_finite()
            && (MIN_LETTER_SPACING..=MAX_LETTER_SPACING).contains(&self.letter)
            && (MIN_LINE_SPACING..=MAX_LINE_SPACING).contains(&self.line)
    }
}

/// Face choices shared by rasterization, caret layout, and cached text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontFace {
    pub bold: bool,
    pub italic: bool,
}

/// Everything that decides how a text layer rasterizes.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpec {
    pub source: String,
    pub family: String,
    pub face: FontFace,
    pub size: f32,
    pub color: Rgba<u8>,
    pub spacing: Spacing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub source: String,
    pub family: String,
    pub face: FontFace,
    pub size: f32,
    pub color: Rgba<u8>,
    pub spacing: Spacing,
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
    pub fn matches(&self, spec: &TextSpec) -> bool {
        self.style.source == spec.source
            && self.style.family == spec.family
            && self.style.face == spec.face
            && self.style.size == spec.size
            && self.style.color == spec.color
            && self.style.spacing == spec.spacing
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextError {
    TooLong,
    Family,
    Size,
    Spacing,
    TooLarge,
}

impl TextError {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooLong => "Text is limited to 4000 characters.",
            Self::Family => "The font name is too long.",
            Self::Size => "Text size must be between 1 and 256 pixels.",
            Self::Spacing => "Text spacing is out of range.",
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

pub fn fields_allowed(
    source: &str,
    family: &str,
    resolved: &str,
    size: f32,
    spacing: Spacing,
) -> bool {
    spacing.allowed()
        && source.chars().count() <= MAX_TEXT_CHARS
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

#[cfg(test)]
pub fn render(
    source: &str,
    family: &str,
    pixel_size: f32,
    color: Rgba<u8>,
) -> Result<RenderedText, TextError> {
    render_spec(&TextSpec {
        source: source.to_owned(),
        family: family.to_owned(),
        face: FontFace::default(),
        size: pixel_size,
        color,
        spacing: Spacing::default(),
    })
}

pub fn render_spec(spec: &TextSpec) -> Result<RenderedText, TextError> {
    let (source, pixel_size, color, spacing) =
        (spec.source.as_str(), spec.size, spec.color, spec.spacing);
    if !spacing.allowed() {
        return Err(TextError::Spacing);
    }
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
    let family = normalize_family(&spec.family);
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
                spacing,
                spec.face,
                resolved,
                (0, 0),
            ));
        }
        let mut buffer = layout(font_system, source, &family, pixel_size, spacing, spec.face);

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
            spacing,
            spec.face,
            resolved,
            (-left, -top),
        ))
    })
}

fn layout(
    font_system: &mut cosmic_text::FontSystem,
    source: &str,
    family: &str,
    pixel_size: f32,
    spacing: Spacing,
    face: FontFace,
) -> cosmic_text::Buffer {
    let metrics = cosmic_text::Metrics::new(pixel_size, line_height(pixel_size, spacing));
    let mut buffer = cosmic_text::Buffer::new(font_system, metrics);
    buffer.set_wrap(cosmic_text::Wrap::None);
    buffer.set_size(None, None);
    let attrs = cosmic_text::Attrs::new()
        .family(family_attr(family))
        .weight(if face.bold {
            cosmic_text::Weight::BOLD
        } else {
            cosmic_text::Weight::NORMAL
        })
        .style(if face.italic {
            cosmic_text::Style::Italic
        } else {
            cosmic_text::Style::Normal
        })
        .letter_spacing(spacing.letter / pixel_size);
    buffer.set_text(source, &attrs, cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn line_height(pixel_size: f32, spacing: Spacing) -> f32 {
    (pixel_size * spacing.line).max(1.0)
}

/// A caret position in layout coordinates, where the text anchor is the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStop {
    pub byte: usize,
    pub line: usize,
    pub x: f32,
    pub top: f32,
}

/// Caret positions for every character boundary of one layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaretMap {
    pub spec: Option<TextSpec>,
    pub stops: Vec<CaretStop>,
    pub line_height: f32,
}

impl CaretMap {
    pub fn matches(&self, spec: &TextSpec) -> bool {
        self.spec.as_ref().is_some_and(|own| {
            own.source == spec.source
                && own.family == spec.family
                && own.face == spec.face
                && own.size == spec.size
                && own.spacing == spec.spacing
        })
    }

    /// The stop at a byte offset, or the nearest earlier one.
    pub fn at(&self, byte: usize) -> Option<CaretStop> {
        let index = self.stops.partition_point(|stop| stop.byte <= byte);
        index.checked_sub(1).map(|index| self.stops[index])
    }

    /// The byte offset nearest to a layout point.
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let last = self.stops.last()?;
        let line = if self.line_height > 0.0 && y.is_finite() {
            ((y / self.line_height).floor().max(0.0) as usize).min(last.line)
        } else {
            0
        };
        self.stops
            .iter()
            .filter(|stop| stop.line == line)
            .min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs()))
            .map(|stop| stop.byte)
    }

    /// The byte offset on the line above or below at the same x.
    pub fn vertical(&self, byte: usize, down: bool) -> Option<usize> {
        let current = self.at(byte)?;
        let line = if down {
            current.line.checked_add(1)?
        } else {
            current.line.checked_sub(1)?
        };
        self.stops
            .iter()
            .filter(|stop| stop.line == line)
            .min_by(|a, b| (a.x - current.x).abs().total_cmp(&(b.x - current.x).abs()))
            .map(|stop| stop.byte)
    }
}

pub fn caret_map(spec: &TextSpec) -> Result<CaretMap, TextError> {
    if !spec.size.is_finite() || !(MIN_PIXEL_SIZE..=MAX_PIXEL_SIZE).contains(&spec.size) {
        return Err(TextError::Size);
    }
    if !spec.spacing.allowed() {
        return Err(TextError::Spacing);
    }
    let family = normalize_family(&spec.family);
    let line_height = line_height(spec.size, spec.spacing);
    let mut stops = Vec::with_capacity(spec.source.len() + 1);
    with_fonts(|font_system, _| {
        let buffer = layout(
            font_system,
            &spec.source,
            &family,
            spec.size,
            spec.spacing,
            spec.face,
        );
        let mut start = 0;
        for (line, text) in spec.source.split('\n').enumerate() {
            let run = buffer.layout_runs().find(|run| run.line_i == line);
            let top = run
                .as_ref()
                .map_or(line as f32 * line_height, |run| run.line_top);
            for (index, _) in text
                .char_indices()
                .chain(std::iter::once((text.len(), ' ')))
            {
                let cursor = cosmic_text::Cursor::new(line, index);
                let x = run
                    .as_ref()
                    .and_then(|run| run.cursor_position(&cursor))
                    .unwrap_or(0.0);
                stops.push(CaretStop {
                    byte: start + index,
                    line,
                    x: if x.is_finite() { x } else { 0.0 },
                    top: if top.is_finite() { top } else { 0.0 },
                });
            }
            start += text.len() + 1;
        }
    });
    Ok(CaretMap {
        spec: Some(spec.clone()),
        stops,
        line_height,
    })
}

/// Installed font families, sorted for a picker.
pub struct FontFamilies {
    pub names: Vec<String>,
    pub generic_previews: [String; 3],
}

pub fn families() -> FontFamilies {
    let (mut names, generic_previews) = with_fonts(|font_system, _| {
        let names = font_system
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .filter(|name| {
                !name.starts_with('.')
                    && name.chars().count() <= MAX_FAMILY_CHARS
                    && !name.chars().any(char::is_control)
            })
            .collect::<Vec<_>>();
        // GPUI expects an installed family name rather than CSS generic names.
        let previews = ["sans-serif", "serif", "monospace"].map(|family| {
            let id = font_system.db().query(&cosmic_text::fontdb::Query {
                families: &[family_attr(family)],
                ..Default::default()
            });
            id.and_then(|id| face_family(font_system, id))
                .unwrap_or_else(|| family.to_owned())
        });
        (names, previews)
    });
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    FontFamilies {
        names,
        generic_previews,
    }
}

#[allow(clippy::too_many_arguments)]
fn rendered(
    image: RgbaImage,
    source: &str,
    family: String,
    pixel_size: f32,
    color: Rgba<u8>,
    spacing: Spacing,
    face: FontFace,
    resolved: String,
    origin: (i32, i32),
) -> RenderedText {
    RenderedText {
        image: Arc::new(image),
        style: TextStyle {
            source: source.to_owned(),
            family,
            face,
            size: pixel_size,
            color,
            spacing,
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

    fn spec(source: &str, spacing: Spacing) -> TextSpec {
        TextSpec {
            source: source.into(),
            family: "sans-serif".into(),
            face: FontFace::default(),
            size: 32.0,
            color: Rgba([0, 0, 0, 255]),
            spacing,
        }
    }

    #[test]
    fn letter_and_line_spacing_widen_and_heighten_the_bitmap() {
        let plain = render_spec(&spec("AA\nAA", Spacing::default())).unwrap();
        let tracked = render_spec(&spec(
            "AA\nAA",
            Spacing {
                letter: 20.0,
                ..Spacing::default()
            },
        ))
        .unwrap();
        let loose = render_spec(&spec(
            "AA\nAA",
            Spacing {
                line: 3.0,
                ..Spacing::default()
            },
        ))
        .unwrap();
        assert!(tracked.image.width() >= plain.image.width() + 15);
        assert!(loose.image.height() >= plain.image.height() + 40);
        assert_eq!(tracked.style.spacing.letter, 20.0);
        assert!(matches!(
            render_spec(&spec(
                "A",
                Spacing {
                    letter: MAX_LETTER_SPACING + 1.0,
                    ..Spacing::default()
                }
            )),
            Err(TextError::Spacing)
        ));
    }

    #[test]
    fn bold_and_italic_change_pixels_and_invalidate_both_preview_caches() {
        let plain_spec = spec("Aa Hello", Spacing::default());
        let plain = render_spec(&plain_spec).unwrap();
        let carets = caret_map(&plain_spec).unwrap();
        for face in [
            FontFace {
                bold: true,
                italic: false,
            },
            FontFace {
                bold: false,
                italic: true,
            },
            FontFace {
                bold: true,
                italic: true,
            },
        ] {
            let styled_spec = TextSpec {
                face,
                ..plain_spec.clone()
            };
            assert!(!plain.matches(&styled_spec));
            assert!(!carets.matches(&styled_spec));
            let styled = render_spec(&styled_spec).unwrap();
            assert_eq!(styled.style.face, face);
            assert_ne!(styled.image, plain.image);
            assert!(styled.matches(&styled_spec));
            let styled_carets = caret_map(&styled_spec).unwrap();
            assert!(styled_carets.matches(&styled_spec));
            assert_eq!(
                styled_carets.stops.last().unwrap().byte,
                styled_spec.source.len()
            );
        }
    }

    #[test]
    fn caret_stops_cover_every_boundary_and_follow_lines() {
        let map = caret_map(&spec("ab\nc上", Spacing::default())).unwrap();
        let bytes: Vec<usize> = map.stops.iter().map(|stop| stop.byte).collect();
        assert_eq!(bytes, vec![0, 1, 2, 3, 4, 7]);
        assert_eq!(map.stops[0].x, 0.0);
        assert!(map.stops[1].x > 0.0 && map.stops[2].x > map.stops[1].x);
        assert_eq!(map.stops[3].line, 1);
        assert!(map.stops[3].top > map.stops[0].top);
        assert_eq!(map.at(5).unwrap().byte, 4);
        assert_eq!(map.vertical(0, true), Some(3));
        assert_eq!(map.hit(-10.0, 1.0), Some(0));
        assert_eq!(map.hit(1000.0, map.line_height * 1.5), Some(7));
        assert_eq!(map.vertical(0, false), None);
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
        let rendered = render("Ag", "Lime Missing Font", 32.0, Rgba([20, 40, 60, 255])).unwrap();
        assert!(rendered.style.substituted());
        assert!(
            !rendered
                .style
                .resolved_family
                .eq_ignore_ascii_case("Lime Missing Font")
        );
        assert!(max_alpha(&rendered.image) > 0);
        assert_eq!(rendered.style.family, "Lime Missing Font");
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
