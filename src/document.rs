mod composite;
mod content;

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

pub use content::{
    BlendMode, CropRect, DocumentContent, IMPORTED_LAYER_NAME, LayerId, MAX_LAYER_NAME_CHARS,
    MAX_LAYERS, RasterLayer,
};

pub const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_NEW_EDGE: u32 = 16_384;
pub const DEFAULT_JPEG_QUALITY: u8 = 92;

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
    pub content: DocumentContent,
    pub canvas: RenderedCanvas,
}

pub struct RenderedCanvas {
    pub pixels: Arc<RgbaImage>,
    pub preview: Preview,
    pub thumbnails: Vec<(LayerId, Arc<RenderImage>)>,
}

impl RenderedCanvas {
    pub fn new(content: &DocumentContent) -> Self {
        let pixels = content.composite();
        let preview = render_preview(&pixels);
        let thumbnails = content
            .layers()
            .iter()
            .map(|layer| (layer.id, render_thumbnail(&layer.pixels)))
            .collect();
        Self {
            pixels,
            preview,
            thumbnails,
        }
    }

    pub fn thumbnail(&self, id: LayerId) -> Option<&Arc<RenderImage>> {
        self.thumbnails
            .iter()
            .find_map(|(layer, image)| (*layer == id).then_some(image))
    }

    // Every GPU image this canvas owns, for release when it is replaced or closed.
    pub fn into_images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        std::iter::once(self.preview.image)
            .chain(self.thumbnails.into_iter().map(|(_, image)| image))
    }
}

pub struct Preview {
    pub image: Arc<RenderImage>,
    pub has_transparency: bool,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().context("The file could not be found")?;
        ensure!(path.is_file(), "Choose an image file");
        let mut reader = BufReader::new(File::open(&path).context("The file could not be opened")?);
        let (format, content) = if reader.fill_buf()?.starts_with(project::MAGIC) {
            (FileFormat::Project, project::read(reader)?)
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
            let content =
                DocumentContent::single_layer(IMPORTED_LAYER_NAME.to_owned(), image.into_rgba8());
            (format, content)
        };
        Ok(Self {
            name: file_name(&path),
            path: Some(path),
            format,
            canvas: RenderedCanvas::new(&content),
            content,
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

    // Raster Save is allowed only while the format can hold everything the user edited.
    pub fn can_save_as_raster(&self) -> bool {
        self.format != FileFormat::Project && self.content.is_plain_raster()
    }

    pub fn blank(name: String, width: u32, height: u32, background: Rgba<u8>) -> Self {
        let content = DocumentContent::single_layer(
            "Background".to_owned(),
            RgbaImage::from_pixel(width, height, background),
        );
        Self {
            path: None,
            name,
            format: FileFormat::Project,
            canvas: RenderedCanvas::new(&content),
            content,
        }
    }

    pub fn width(&self) -> u32 {
        self.content.width
    }

    pub fn height(&self) -> u32 {
        self.content.height
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

fn render_thumbnail(image: &RgbaImage) -> Arc<RenderImage> {
    const EDGE: u32 = 64;
    let (width, height) = image.dimensions();
    let scale = |length: u32| {
        ((u64::from(length) * u64::from(EDGE) / u64::from(width.max(height))) as u32).max(1)
    };
    let mut pixels = if width.max(height) > EDGE {
        imageops::thumbnail(image, scale(width), scale(height))
    } else {
        image.clone()
    };
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(smallvec![Frame::new(pixels)]))
}

fn render_preview(image: &RgbaImage) -> Preview {
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

pub fn save_image(content: &DocumentContent, path: &Path, format: FileFormat) -> Result<()> {
    write_atomically(path, |writer| match format {
        FileFormat::Project => project::write(writer, content),
        _ => write_raster(writer, &content.composite(), format, DEFAULT_JPEG_QUALITY),
    })
}

pub fn export_image(
    content: &DocumentContent,
    path: &Path,
    format: FileFormat,
    jpeg_quality: u8,
) -> Result<()> {
    ensure!(
        format != FileFormat::Project,
        "Export writes PNG, JPEG, or WebP images."
    );
    write_atomically(path, |writer| {
        write_raster(writer, &content.composite(), format, jpeg_quality)
    })
}

// Writes beside the destination and replaces it only after a complete, synced write.
fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<&mut File>) -> Result<()>,
) -> Result<()> {
    let parent = path.parent().context("Choose a destination folder")?;
    let mut temporary =
        NamedTempFile::new_in(parent).context("Could not create the output file")?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        write(&mut writer)?;
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

fn write_raster(
    writer: impl Write,
    pixels: &RgbaImage,
    format: FileFormat,
    jpeg_quality: u8,
) -> Result<()> {
    let (width, height) = pixels.dimensions();
    let rgba = ExtendedColorType::Rgba8;
    match format {
        FileFormat::Project => unreachable!("projects are saved as layers"),
        FileFormat::Png => codecs::png::PngEncoder::new(writer).write_image(
            pixels.as_raw(),
            width,
            height,
            rgba,
        )?,
        FileFormat::WebP => codecs::webp::WebPEncoder::new_lossless(writer).write_image(
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
            codecs::jpeg::JpegEncoder::new_with_quality(writer, jpeg_quality).write_image(
                rgb.as_raw(),
                width,
                height,
                ExtendedColorType::Rgb8,
            )?;
        }
    }
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
        let mut content = DocumentContent::single_layer("Image".into(), source);
        let preview = RenderedCanvas::new(&content).preview;
        assert!(preview.has_transparency);
        assert_eq!(u32::from(preview.image.size(0).width), 4096);

        assert!(content.crop(CropRect::between((4999, 2), (4997, 0))));
        let layer = content.top_layer_id();
        assert!(content.fill(layer, 0, 1, ink));
        let filled = content.composite();
        assert_eq!(filled.dimensions(), (3, 3));
        for y in 0..3 {
            assert_eq!(filled.get_pixel(0, y).0, ink.0);
            assert_eq!(filled.get_pixel(1, y).0, wall.0);
            assert_eq!(filled.get_pixel(2, y).0, background.0);
        }
        assert!(!content.fill(layer, 0, 0, ink));
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
            let content = DocumentContent::single_layer("Original layer".into(), pixels.clone());
            save_image(&content, &path, format).unwrap();
            let loaded = Document::open(&path).unwrap();
            assert_eq!(loaded.format, format);
            assert_eq!(*loaded.canvas.pixels, pixels);
            if format == FileFormat::Project {
                assert_eq!(
                    loaded.content.full_canvas_layer().unwrap().name,
                    "Original layer"
                );
            }
        }
    }

    #[test]
    fn failed_project_save_preserves_the_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.imageedit");
        let original = RgbaImage::from_pixel(2, 1, solid_color(0xff5500));
        let original = DocumentContent::single_layer("Original".into(), original);
        save_image(&original, &path, FileFormat::Project).unwrap();
        let previous_bytes = std::fs::read(&path).unwrap();
        let replacement = RgbaImage::from_pixel(1, 1, solid_color(0x000000));
        let replacement = DocumentContent::single_layer("x".repeat(65_536), replacement);
        assert!(save_image(&replacement, &path, FileFormat::Project).is_err());

        let mut crowded = original.clone();
        for id in 1..=128 {
            let mut layer = crowded.layers[0].clone();
            layer.id = LayerId(id);
            crowded.layers.push(layer);
        }
        assert!(save_image(&crowded, &path, FileFormat::Project).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous_bytes);
        let reopened = Document::open(&path).unwrap();
        assert_eq!(reopened.content.layers().len(), 1);
        assert_eq!(reopened.canvas.pixels.get_pixel(1, 0).0, [255, 85, 0, 255]);
        let leftovers = std::fs::read_dir(directory.path()).unwrap().count();
        assert_eq!(leftovers, 1);
    }

    #[test]
    fn malformed_project_does_not_open() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.imageedit");
        let content = DocumentContent::single_layer("Layer".into(), RgbaImage::new(2, 2));
        save_image(&content, &path, FileFormat::Project).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 1);
        std::fs::write(&path, bytes).unwrap();
        assert!(Document::open(&path).is_err());
    }

    #[test]
    fn raster_save_uses_the_composite_and_projects_keep_layer_offsets() {
        let directory = tempfile::tempdir().unwrap();
        let mut content = DocumentContent::single_layer(
            "Image".into(),
            RgbaImage::from_pixel(2, 1, Rgba([70, 80, 90, 255])),
        );
        content.layers[0].offset = (1, 0);
        let png = directory.path().join("offset.png");
        save_image(&content, &png, FileFormat::Png).unwrap();
        let opened = Document::open(&png).unwrap();
        assert_eq!(opened.canvas.pixels.get_pixel(0, 0).0, [0; 4]);
        assert_eq!(opened.canvas.pixels.get_pixel(1, 0).0, [70, 80, 90, 255]);

        let project = directory.path().join("offset.imageedit");
        save_image(&content, &project, FileFormat::Project).unwrap();
        let reopened = Document::open(&project).unwrap();
        assert_eq!(reopened.format, FileFormat::Project);
        assert_eq!(reopened.content.layers()[0].offset, (1, 0));
        assert_eq!(*reopened.canvas.pixels, *opened.canvas.pixels);
    }

    #[test]
    fn export_flattens_a_layered_project_without_touching_it() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("layers.imageedit");
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/projects/v2-two-layers.imageedit"
        );
        std::fs::copy(fixture, &project).unwrap();
        let project_bytes = std::fs::read(&project).unwrap();
        let mut document = Document::open(&project).unwrap();
        document.content.layers[0].visible = false;
        document.content.layers[1].visible = true;
        let composite = document.content.composite();
        assert!(composite.pixels().any(|pixel| pixel.0[3] == 0));
        assert!(
            composite
                .pixels()
                .any(|pixel| (1..255).contains(&pixel.0[3]))
        );

        for format in [FileFormat::Png, FileFormat::WebP, FileFormat::Jpeg] {
            let path = directory
                .path()
                .join(format!("export.{}", format.extension()));
            export_image(&document.content, &path, format, 100).unwrap();
            let exported = Document::open(&path).unwrap();
            assert_eq!(exported.format, format);
            assert_eq!(exported.content.layers().len(), 1);
            let pixels = exported.canvas.pixels;
            if format == FileFormat::Jpeg {
                for (actual, expected) in pixels.pixels().zip(composite.pixels()) {
                    let [r, g, b, a] = expected.0.map(u16::from);
                    let over_white = [r, g, b].map(|c| (c * a + 255 * (255 - a) + 127) / 255);
                    assert_eq!(actual.0[3], 255);
                    for (actual, expected) in actual.0.iter().zip(over_white) {
                        assert!(u16::from(*actual).abs_diff(expected) <= 6);
                    }
                }
            } else {
                assert_eq!(*pixels, *composite);
            }
        }
        assert_eq!(std::fs::read(&project).unwrap(), project_bytes);
        assert!(export_image(&document.content, &project, FileFormat::Project, 92).is_err());
        let missing = directory.path().join("missing/export.png");
        assert!(export_image(&document.content, &missing, FileFormat::Png, 92).is_err());
        assert!(!missing.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 4);
    }

    #[test]
    fn only_plain_raster_documents_can_save_as_raster() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plain.png");
        let content = DocumentContent::single_layer("Image".into(), RgbaImage::new(2, 2));
        save_image(&content, &path, FileFormat::Png).unwrap();
        let mut document = Document::open(&path).unwrap();
        assert!(document.can_save_as_raster());
        let layer = document.content.top_layer_id();
        assert!(document.content.fill(layer, 0, 0, solid_color(0x123456)));
        assert!(document.content.crop(CropRect::between((0, 0), (0, 1))));
        assert!(document.can_save_as_raster());

        document.content.layers[0].opacity = 254;
        assert!(!document.can_save_as_raster());
        document.content.layers[0].opacity = 255;
        document.content.layers[0].name = "Renamed".into();
        assert!(!document.can_save_as_raster());

        let blank = Document::blank("Untitled".into(), 2, 2, Rgba([0; 4]));
        assert!(!blank.can_save_as_raster());
        let project = directory.path().join("plain.imageedit");
        save_image(&content, &project, FileFormat::Project).unwrap();
        assert!(!Document::open(&project).unwrap().can_save_as_raster());
    }

    #[test]
    fn jpeg_save_flattens_transparency_to_white() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.jpg");
        let pixels = RgbaImage::from_pixel(8, 8, Rgba([200, 0, 120, 0]));
        let content = DocumentContent::single_layer("Image".into(), pixels);
        save_image(&content, &path, FileFormat::Jpeg).unwrap();
        assert!(
            Document::open(&path)
                .unwrap()
                .canvas
                .pixels
                .pixels()
                .all(|pixel| pixel.0 == [255; 4])
        );
    }
}
