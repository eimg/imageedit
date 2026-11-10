use std::io::{BufRead, Seek, Write};

use anyhow::{Context, Result, ensure};
use image::{ExtendedColorType, ImageDecoder, ImageEncoder, RgbaImage, codecs::png};
use serde::{Deserialize, Serialize};

use crate::document::decode_image;

pub const EXTENSION: &str = "imageedit";
pub const MAGIC: &[u8; 8] = b"IMGEDIT\0";
const VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u32 = 64 * 1024;

#[derive(Deserialize, Serialize)]
struct Manifest {
    version: u32,
    width: u32,
    height: u32,
    layers: Vec<Layer>,
}

#[derive(Deserialize, Serialize)]
struct Layer {
    name: String,
}

// Version 1 stores a length-prefixed JSON manifest followed by the single layer's PNG.
pub fn read(mut reader: impl BufRead + Seek) -> Result<(RgbaImage, String)> {
    let mut magic = [0; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .context("Incomplete project header")?;
    ensure!(&magic == MAGIC, "Invalid ImageEdit project header");
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    ensure!(
        length <= MAX_MANIFEST_BYTES,
        "Project manifest is too large"
    );
    let mut manifest = vec![0; length as usize];
    reader.read_exact(&mut manifest)?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).context("Invalid project manifest")?;
    ensure!(
        manifest.version == VERSION,
        "Unsupported ImageEdit project version {}",
        manifest.version
    );
    let [layer] = manifest.layers.as_slice() else {
        anyhow::bail!("This version of ImageEdit supports single-layer projects");
    };
    let decoder = png::PngDecoder::new(reader).context("Invalid project layer")?;
    ensure!(
        decoder.dimensions() == (manifest.width, manifest.height),
        "Project canvas and layer dimensions do not match"
    );
    Ok((decode_image(decoder)?.into_rgba8(), layer.name.clone()))
}

pub fn write(mut writer: impl Write, pixels: &RgbaImage, layer_name: &str) -> Result<()> {
    let manifest = serde_json::to_vec(&Manifest {
        version: VERSION,
        width: pixels.width(),
        height: pixels.height(),
        layers: vec![Layer {
            name: layer_name.into(),
        }],
    })?;
    ensure!(
        manifest.len() <= MAX_MANIFEST_BYTES as usize,
        "Project manifest is too large"
    );
    writer.write_all(MAGIC)?;
    writer.write_all(&(manifest.len() as u32).to_le_bytes())?;
    writer.write_all(&manifest)?;
    png::PngEncoder::new(writer).write_image(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(())
}
