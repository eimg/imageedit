use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Result, ensure};
use gpui::RenderImage;
use image::{DynamicImage, Frame, ImageDecoder, ImageFormat, ImageReader};
use smallvec::smallvec;

pub struct Document {
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
    pub preview: Arc<RenderImage>,
}

impl Document {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize().context("The file could not be found")?;
        ensure!(path.is_file(), "Choose an image file");
        let reader = ImageReader::open(&path)
            .context("The file could not be opened")?
            .with_guessed_format()
            .context("The file type could not be read")?;
        let format = match reader.format() {
            Some(ImageFormat::Png) => "PNG",
            Some(ImageFormat::Jpeg) => "JPEG",
            Some(ImageFormat::WebP) => "WebP",
            _ => anyhow::bail!("Choose a PNG, JPEG, or WebP image"),
        };
        let mut decoder = reader.into_decoder().context("Invalid image data")?;
        let orientation = decoder.orientation().context("Invalid image metadata")?;
        ensure!(
            decoder.total_bytes() <= 256 * 1024 * 1024,
            "This image is too large to open"
        );
        let mut image = DynamicImage::from_decoder(decoder).context("Invalid image data")?;
        image.apply_orientation(orientation);
        let (width, height) = (image.width(), image.height());
        // The preview fits a GPU texture. Keep source dimensions for the document.
        const MAX_PREVIEW_EDGE: u32 = 4096;
        if width.max(height) > MAX_PREVIEW_EDGE {
            image = image.resize(
                MAX_PREVIEW_EDGE,
                MAX_PREVIEW_EDGE,
                image::imageops::FilterType::Triangle,
            );
        }

        // GPUI's render image uses BGRA, while the decoder returns RGBA.
        let mut pixels = image.into_rgba8();
        for pixel in pixels.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Self {
            name: file_name(&path),
            path,
            width,
            height,
            format,
            preview: Arc::new(RenderImage::new(smallvec![Frame::new(pixels)])),
        })
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
