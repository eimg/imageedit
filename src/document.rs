use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, ensure};
use gpui::RenderImage;
use image::{
    DynamicImage, ExtendedColorType, Frame, ImageDecoder, ImageEncoder, ImageFormat, ImageReader,
    RgbImage, Rgba, RgbaImage, codecs, imageops,
};
use smallvec::smallvec;
use tempfile::NamedTempFile;

use crate::project;

const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_NEW_EDGE: u32 = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    Png,
    Jpeg,
    WebP,
    Project,
}

impl FileFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::WebP => "WebP",
            Self::Project => "ImageEdit project",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::WebP => "webp",
            Self::Project => project::EXTENSION,
        }
    }
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub name: String,
    pub format: FileFormat,
    pub layer_name: String,
    pub pixels: Arc<RgbaImage>,
    pub preview: Preview,
}

pub struct Preview {
    pub image: Arc<RenderImage>,
    pub has_transparency: bool,
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
            width: start.0.abs_diff(end.0) + 1,
            height: start.1.abs_diff(end.1) + 1,
        }
    }

    pub fn extract(self, pixels: &RgbaImage) -> RgbaImage {
        imageops::crop_imm(pixels, self.x, self.y, self.width, self.height).to_image()
    }
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().context("The file could not be found")?;
        ensure!(path.is_file(), "Choose an image file");
        let mut reader = BufReader::new(File::open(&path).context("The file could not be opened")?);
        let (format, pixels, layer_name) = if reader.fill_buf()?.starts_with(project::MAGIC) {
            let (pixels, layer_name) = project::read(reader)?;
            (FileFormat::Project, pixels, layer_name)
        } else {
            let reader = ImageReader::new(reader)
                .with_guessed_format()
                .context("The file type could not be read")?;
            let format = match reader.format() {
                Some(ImageFormat::Png) => FileFormat::Png,
                Some(ImageFormat::Jpeg) => FileFormat::Jpeg,
                Some(ImageFormat::WebP) => FileFormat::WebP,
                _ => anyhow::bail!("Choose an ImageEdit project or a PNG, JPEG, or WebP image"),
            };
            let mut decoder = reader.into_decoder().context("Invalid image data")?;
            let orientation = decoder.orientation().context("Invalid image metadata")?;
            let mut image = decode_image(decoder)?;
            image.apply_orientation(orientation);
            (format, image.into_rgba8(), "Image".to_owned())
        };
        let pixels = Arc::new(pixels);
        Ok(Self {
            name: file_name(&path),
            path: Some(path),
            format,
            layer_name,
            preview: render_preview(&pixels),
            pixels,
        })
    }

    pub fn validate_size(width: u32, height: u32) -> Result<()> {
        ensure!(
            (1..=MAX_NEW_EDGE).contains(&width) && (1..=MAX_NEW_EDGE).contains(&height),
            "Enter a width and height between 1 and {MAX_NEW_EDGE} pixels."
        );
        ensure!(
            u64::from(width) * u64::from(height) <= MAX_IMAGE_BYTES / 4,
            "The canvas can have at most {} pixels.",
            MAX_IMAGE_BYTES / 4
        );
        Ok(())
    }

    pub fn blank(name: String, width: u32, height: u32, background: Rgba<u8>) -> Self {
        let pixels = Arc::new(RgbaImage::from_pixel(width, height, background));
        Self {
            path: None,
            name,
            format: FileFormat::Project,
            layer_name: "Background".to_owned(),
            preview: render_preview(&pixels),
            pixels,
        }
    }

    pub fn width(&self) -> u32 {
        self.pixels.width()
    }

    pub fn height(&self) -> u32 {
        self.pixels.height()
    }
}

pub fn decode_image(decoder: impl ImageDecoder) -> Result<DynamicImage> {
    let (width, height) = decoder.dimensions();
    ensure!(
        width > 0
            && height > 0
            && u64::from(width) * u64::from(height) <= MAX_IMAGE_BYTES / 4
            && decoder.total_bytes() <= MAX_IMAGE_BYTES,
        "Invalid image dimensions or image is too large to open"
    );
    DynamicImage::from_decoder(decoder).context("Invalid image data")
}

pub fn render_preview(image: &RgbaImage) -> Preview {
    const MAX_PREVIEW_EDGE: u32 = 4096;
    let (width, height) = image.dimensions();
    let edge = width.max(height);
    let mut pixels = if edge > MAX_PREVIEW_EDGE {
        let fit = |length| {
            ((u64::from(length) * u64::from(MAX_PREVIEW_EDGE) / u64::from(edge)) as u32).max(1)
        };
        imageops::resize(
            image,
            fit(width),
            fit(height),
            imageops::FilterType::Triangle,
        )
    } else {
        image.clone()
    };
    // GPUI uses BGRA with straight alpha.
    let has_transparency = image.pixels().any(|pixel| pixel.0[3] < 255);
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Preview {
        image: Arc::new(RenderImage::new(smallvec![Frame::new(pixels)])),
        has_transparency,
    }
}

pub fn solid_color(color: u32) -> Rgba<u8> {
    let [_, red, green, blue] = color.to_be_bytes();
    Rgba([red, green, blue, 255])
}

pub fn bucket_fill(source: &RgbaImage, x: u32, y: u32, color: Rgba<u8>) -> Option<RgbaImage> {
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

pub fn save_format(path: &Path) -> Result<FileFormat> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "png" => Ok(FileFormat::Png),
        "jpg" | "jpeg" => Ok(FileFormat::Jpeg),
        "webp" => Ok(FileFormat::WebP),
        project::EXTENSION => Ok(FileFormat::Project),
        _ => anyhow::bail!("Use a .imageedit, .png, .jpg, .jpeg, or .webp filename."),
    }
}

pub fn save_image(
    pixels: &RgbaImage,
    layer_name: &str,
    path: &Path,
    format: FileFormat,
) -> Result<()> {
    let parent = path.parent().context("Choose a destination folder")?;
    let mut temporary =
        NamedTempFile::new_in(parent).context("Could not create the output file")?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let (width, height) = pixels.dimensions();
        let rgba = ExtendedColorType::Rgba8;
        match format {
            FileFormat::Project => project::write(&mut writer, pixels, layer_name)?,
            FileFormat::Png => codecs::png::PngEncoder::new(&mut writer).write_image(
                pixels.as_raw(),
                width,
                height,
                rgba,
            )?,
            FileFormat::WebP => codecs::webp::WebPEncoder::new_lossless(&mut writer).write_image(
                pixels.as_raw(),
                width,
                height,
                rgba,
            )?,
            FileFormat::Jpeg => {
                let rgb = RgbImage::from_fn(width, height, |x, y| {
                    let [r, g, b, a] = pixels.get_pixel(x, y).0;
                    let alpha = u16::from(a);
                    image::Rgb([r, g, b].map(|channel| {
                        ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                    }))
                });
                codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, 92).write_image(
                    rgb.as_raw(),
                    width,
                    height,
                    ExtendedColorType::Rgb8,
                )?;
            }
        }
        writer.flush()?;
    }
    if let Ok(existing) = path.metadata() {
        temporary
            .as_file()
            .set_permissions(existing.permissions())?;
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .context("Could not replace the destination file")?;
    Ok(())
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_and_fill_use_full_resolution_pixels() {
        let background = Rgba([0; 4]);
        let wall = solid_color(0xffffff);
        let ink = solid_color(0x135ace);
        let mut source = RgbaImage::from_pixel(5000, 3, background);
        for y in 0..3 {
            source.put_pixel(4998, y, wall);
        }
        let document = render_preview(&source);
        assert!(document.has_transparency);
        assert_eq!(u32::from(document.image.size(0).width), 4096);

        let crop = CropRect::between((4999, 2), (4997, 0)).extract(&source);
        let filled = bucket_fill(&crop, 0, 1, ink).unwrap();
        assert_eq!(filled.dimensions(), (3, 3));
        for y in 0..3 {
            assert_eq!(filled.get_pixel(0, y).0, ink.0);
            assert_eq!(filled.get_pixel(1, y).0, wall.0);
            assert_eq!(filled.get_pixel(2, y).0, background.0);
        }
        assert!(bucket_fill(&filled, 0, 0, ink).is_none());
    }

    #[test]
    fn fill_does_not_cross_diagonal_or_different_alpha() {
        let blank = Rgba([0; 4]);
        let wall = solid_color(0x000000);
        let ink = solid_color(0xff00ff);
        let mut image = RgbaImage::from_pixel(2, 2, wall);
        image.put_pixel(0, 0, blank);
        image.put_pixel(1, 1, blank);
        let filled = bucket_fill(&image, 0, 0, ink).unwrap();
        assert_eq!(filled.into_raw(), [ink.0, wall.0, wall.0, blank.0].concat());
    }

    #[test]
    fn lossless_formats_round_trip_pixels_and_project_layer_name() {
        let directory = tempfile::tempdir().unwrap();
        let mut pixels = RgbaImage::from_pixel(3, 2, Rgba([22, 31, 100, 128]));
        pixels.put_pixel(1, 0, Rgba([0; 4]));
        for format in [FileFormat::Project, FileFormat::Png, FileFormat::WebP] {
            let path = directory
                .path()
                .join(format!("image.{}", format.extension()));
            save_image(&pixels, "Original layer", &path, format).unwrap();
            let loaded = Document::open(&path).unwrap();
            assert_eq!(loaded.format, format);
            assert_eq!(*loaded.pixels, pixels);
            if format == FileFormat::Project {
                assert_eq!(loaded.layer_name, "Original layer");
            }
        }
    }

    #[test]
    fn failed_project_save_preserves_the_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.imageedit");
        let original = RgbaImage::from_pixel(2, 1, solid_color(0xff5500));
        save_image(&original, "Original", &path, FileFormat::Project).unwrap();
        let previous_bytes = std::fs::read(&path).unwrap();
        let replacement = RgbaImage::from_pixel(1, 1, solid_color(0x000000));
        assert!(
            save_image(
                &replacement,
                &"x".repeat(65_536),
                &path,
                FileFormat::Project
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), previous_bytes);
    }

    #[test]
    fn jpeg_save_flattens_transparency_to_white() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.jpg");
        let pixels = RgbaImage::from_pixel(8, 8, Rgba([200, 0, 120, 0]));
        save_image(&pixels, "Image", &path, FileFormat::Jpeg).unwrap();
        assert!(
            Document::open(&path)
                .unwrap()
                .pixels
                .pixels()
                .all(|pixel| pixel.0 == [255; 4])
        );
    }
}
