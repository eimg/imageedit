use std::{
    collections::HashSet,
    io::{BufRead, Cursor, Read, Seek, SeekFrom, Write},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use image::{ExtendedColorType, ImageDecoder, ImageEncoder, codecs::png};
use serde::{Deserialize, Serialize};

use crate::document::{
    BlendMode, DocumentContent, LayerId, MAX_IMAGE_BYTES, MAX_LAYERS, MAX_STACK_BYTES, RasterLayer,
    decode_image,
};

pub const EXTENSION: &str = "imageedit";
pub const MAGIC: &[u8; 8] = b"IMGEDIT\0";
const VERSION: u32 = 3;
const MAX_MANIFEST_BYTES: u32 = 64 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 768 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    mode: crate::transform::ColorMode,
    width: u32,
    height: u32,
    // Bottom to top, in payload order.
    layers: Vec<LayerEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LayerEntry {
    id: u64,
    name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    visible: bool,
    opacity: u8,
    blend_mode: WireBlendMode,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum WireBlendMode {
    Normal,
}

impl From<BlendMode> for WireBlendMode {
    fn from(mode: BlendMode) -> Self {
        match mode {
            BlendMode::Normal => Self::Normal,
        }
    }
}

impl From<WireBlendMode> for BlendMode {
    fn from(mode: WireBlendMode) -> Self {
        match mode {
            WireBlendMode::Normal => Self::Normal,
        }
    }
}

// A project starts with the magic and a little-endian u32 JSON manifest length,
// followed by one little-endian u64 length-prefixed PNG per layer in manifest order.
pub fn read(mut reader: impl BufRead + Seek) -> Result<DocumentContent> {
    let mut magic = [0; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .context("Incomplete project header")?;
    ensure!(&magic == MAGIC, "Invalid ImageEdit project header");
    let mut length = [0; 4];
    reader
        .read_exact(&mut length)
        .context("Incomplete project header")?;
    let length = u32::from_le_bytes(length);
    ensure!(
        length <= MAX_MANIFEST_BYTES,
        "Project manifest is too large"
    );
    let mut manifest = vec![0; length as usize];
    reader
        .read_exact(&mut manifest)
        .context("Incomplete project manifest")?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest).context("Invalid project manifest")?;
    ensure!(
        manifest.version == VERSION,
        "Unsupported ImageEdit project version {}",
        manifest.version
    );
    check_limits(&manifest)?;

    let start = reader.stream_position()?;
    let mut remaining = reader
        .seek(SeekFrom::End(0))?
        .checked_sub(start)
        .context("Project was truncated while opening")?;
    reader.seek(SeekFrom::Start(start))?;
    let mut payload_budget = MAX_PAYLOAD_BYTES;
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for entry in manifest.layers {
        let mut length = [0; 8];
        ensure!(remaining >= 8, "Layer \"{}\" is missing", entry.name);
        reader.read_exact(&mut length)?;
        remaining -= 8;
        let length = u64::from_le_bytes(length);
        ensure!(
            length > 0 && length <= remaining,
            "Layer \"{}\" is truncated",
            entry.name
        );
        payload_budget = payload_budget
            .checked_sub(length)
            .context("Project layer data is too large")?;
        remaining -= length;

        let mut payload = Vec::with_capacity(usize::try_from(length)?);
        (&mut reader).take(length).read_to_end(&mut payload)?;
        ensure!(
            payload.len() as u64 == length,
            "Layer \"{}\" is truncated",
            entry.name
        );
        let decoder = png::PngDecoder::new(Cursor::new(payload))
            .with_context(|| format!("Layer \"{}\" is not a valid PNG", entry.name))?;
        ensure!(
            decoder.dimensions() == (entry.width, entry.height),
            "Layer \"{}\" does not match its recorded size",
            entry.name
        );
        layers.push(RasterLayer {
            id: LayerId(entry.id),
            pixels: Arc::new(decode_image(decoder)?.into_rgba8()),
            offset: (entry.x, entry.y),
            visible: entry.visible,
            opacity: entry.opacity,
            blend_mode: entry.blend_mode.into(),
            name: entry.name,
        });
    }
    ensure!(
        remaining == 0,
        "Project contains unexpected data after its layers"
    );
    let mut content = DocumentContent::from_layers(manifest.width, manifest.height, layers)?;
    content.set_stored_mode(manifest.mode);
    Ok(content)
}

pub fn write(mut writer: impl Write, content: &DocumentContent) -> Result<()> {
    let (width, height) = content.size();
    let manifest = Manifest {
        version: VERSION,
        mode: content.mode(),
        width,
        height,
        layers: content
            .layers()
            .iter()
            .map(|layer| LayerEntry {
                id: layer.id.0,
                name: layer.name.clone(),
                x: layer.offset.0,
                y: layer.offset.1,
                width: layer.pixels.width(),
                height: layer.pixels.height(),
                visible: layer.visible,
                opacity: layer.opacity,
                blend_mode: layer.blend_mode.into(),
            })
            .collect(),
    };
    check_limits(&manifest)?;
    let manifest = serde_json::to_vec(&manifest)?;
    ensure!(
        manifest.len() <= MAX_MANIFEST_BYTES as usize,
        "Project manifest is too large"
    );
    writer.write_all(MAGIC)?;
    writer.write_all(&(manifest.len() as u32).to_le_bytes())?;
    writer.write_all(&manifest)?;

    let mut payload_budget = MAX_PAYLOAD_BYTES;
    let mut payload = Vec::new();
    for layer in content.layers() {
        payload.clear();
        png::PngEncoder::new(&mut payload).write_image(
            layer.pixels.as_raw(),
            layer.pixels.width(),
            layer.pixels.height(),
            ExtendedColorType::Rgba8,
        )?;
        let length = payload.len() as u64;
        payload_budget = payload_budget
            .checked_sub(length)
            .context("Project layer data is too large")?;
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&payload)?;
    }
    Ok(())
}

// Reads and writes share these checks, which run before any pixel buffer is allocated.
fn check_limits(manifest: &Manifest) -> Result<()> {
    ensure!(
        pixel_bytes(manifest.width, manifest.height).is_some_and(|bytes| bytes <= MAX_IMAGE_BYTES),
        "The project canvas is empty or too large"
    );
    ensure!(
        (1..=MAX_LAYERS).contains(&manifest.layers.len()),
        "A project must have between 1 and {MAX_LAYERS} layers"
    );
    let mut total: u64 = 0;
    let mut ids = HashSet::with_capacity(manifest.layers.len());
    for layer in &manifest.layers {
        let bytes = pixel_bytes(layer.width, layer.height)
            .filter(|&bytes| bytes <= MAX_IMAGE_BYTES)
            .with_context(|| format!("Layer \"{}\" is empty or too large", layer.name))?;
        total = total
            .checked_add(bytes)
            .filter(|&total| total <= MAX_STACK_BYTES)
            .context("The project layers are too large")?;
        ensure!(ids.insert(layer.id), "Layer ID {} is repeated", layer.id);
    }
    Ok(())
}

fn pixel_bytes(width: u32, height: u32) -> Option<u64> {
    (width > 0 && height > 0)
        .then(|| u64::from(width) * u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use serde_json::json;

    use super::*;

    const V2_GOLDEN: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/projects/v3-two-layers.imageedit"
    ));

    fn read_bytes(bytes: &[u8]) -> Result<DocumentContent> {
        read(Cursor::new(bytes))
    }

    fn write_bytes(content: &DocumentContent) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        write(&mut bytes, content)?;
        Ok(bytes)
    }

    fn two_layers() -> DocumentContent {
        let bottom = RasterLayer {
            id: LayerId(7),
            name: "Background".into(),
            pixels: Arc::new(RgbaImage::from_fn(4, 3, |x, y| {
                Rgba([x as u8 * 60, y as u8 * 80, 200, 255])
            })),
            offset: (0, 0),
            visible: true,
            opacity: 255,
            blend_mode: BlendMode::Normal,
        };
        let top = RasterLayer {
            id: LayerId(3),
            name: "Glaze ✦ 上".into(),
            pixels: Arc::new(RgbaImage::from_fn(3, 2, |x, y| match (x, y) {
                (0, 0) => Rgba([255, 0, 0, 128]),
                (1, 0) => Rgba([9, 99, 199, 0]),
                _ => Rgba([20, 200, 40, 255]),
            })),
            offset: (-1, 2),
            visible: false,
            opacity: 77,
            blend_mode: BlendMode::Normal,
        };
        DocumentContent::from_layers(4, 3, vec![bottom, top]).unwrap()
    }

    fn assert_same(actual: &DocumentContent, expected: &DocumentContent) {
        assert_eq!(actual.mode(), expected.mode());
        assert_eq!(actual.size(), expected.size());
        assert_eq!(actual.layers().len(), expected.layers().len());
        for (actual, expected) in actual.layers().iter().zip(expected.layers()) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.offset, expected.offset);
            assert_eq!(actual.visible, expected.visible);
            assert_eq!(actual.opacity, expected.opacity);
            assert_eq!(actual.blend_mode, expected.blend_mode);
            assert_eq!(*actual.pixels, *expected.pixels);
        }
    }

    fn manifest_json(bytes: &[u8]) -> serde_json::Value {
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[12..12 + length]).unwrap()
    }

    // Replaces the manifest of a valid project and keeps its payloads.
    fn with_manifest(bytes: &[u8], manifest: &serde_json::Value) -> Vec<u8> {
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let manifest = serde_json::to_vec(manifest).unwrap();
        let mut output = MAGIC.to_vec();
        output.extend((manifest.len() as u32).to_le_bytes());
        output.extend(manifest);
        output.extend(&bytes[12 + length..]);
        output
    }

    fn payload(pixels: &RgbaImage) -> Vec<u8> {
        let mut png = Vec::new();
        png::PngEncoder::new(&mut png)
            .write_image(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let mut framed = (png.len() as u64).to_le_bytes().to_vec();
        framed.extend(png);
        framed
    }

    #[test]
    fn round_trips_layer_order_ids_offsets_alpha_and_properties() {
        let content = two_layers();
        let bytes = write_bytes(&content).unwrap();
        assert_same(&read_bytes(&bytes).unwrap(), &content);
        assert_same(&read_bytes(V2_GOLDEN).unwrap(), &content);
        assert_eq!(
            manifest_json(&bytes)["layers"][1],
            json!({
                "id": 3, "name": "Glaze ✦ 上", "x": -1, "y": 2, "width": 3, "height": 2,
                "visible": false, "opacity": 77, "blend_mode": "normal",
            })
        );
    }

    #[test]
    fn malformed_manifests_are_rejected() {
        let valid = write_bytes(&two_layers()).unwrap();
        let manifest = manifest_json(&valid);
        type Edit<'a> = (&'a str, &'a dyn Fn(&mut serde_json::Value));
        let edits: [Edit; 18] = [
            ("unknown version", &|m| m["version"] = json!(4)),
            ("missing mode", &|m| {
                m.as_object_mut().unwrap().remove("mode");
            }),
            ("unknown mode", &|m| m["mode"] = json!("cmyk")),
            ("version zero", &|m| m["version"] = json!(0)),
            ("missing version", &|m| {
                m.as_object_mut().unwrap().remove("version");
            }),
            ("unknown field", &|m| m["selection"] = json!([])),
            ("unknown layer field", &|m| {
                m["layers"][0]["path"] = json!(1)
            }),
            ("missing layer field", &|m| {
                m["layers"][0].as_object_mut().unwrap().remove("visible");
            }),
            ("duplicate id", &|m| m["layers"][1]["id"] = json!(7)),
            ("blend mode", &|m| {
                m["layers"][1]["blend_mode"] = json!("multiply")
            }),
            ("opacity", &|m| m["layers"][1]["opacity"] = json!(256)),
            ("offset", &|m| {
                m["layers"][1]["x"] = json!(i64::from(i32::MAX) + 1)
            }),
            ("no layers", &|m| m["layers"] = json!([])),
            ("too many layers", &|m| {
                let layer = m["layers"][0].clone();
                m["layers"] = (0..129)
                    .map(|id| {
                        let mut layer = layer.clone();
                        layer["id"] = json!(id);
                        layer
                    })
                    .collect();
            }),
            ("empty canvas", &|m| m["width"] = json!(0)),
            ("canvas too large", &|m| {
                m["width"] = json!(100_000);
                m["height"] = json!(100_000);
            }),
            ("empty layer", &|m| m["layers"][0]["height"] = json!(0)),
            ("layers too large", &|m| {
                let mut layer = m["layers"][0].clone();
                layer["width"] = json!(8192);
                layer["height"] = json!(8192);
                m["layers"] = (0..3)
                    .map(|id| {
                        layer["id"] = json!(id);
                        layer.clone()
                    })
                    .collect();
            }),
        ];
        for (label, edit) in edits {
            let mut manifest = manifest.clone();
            edit(&mut manifest);
            assert!(
                read_bytes(&with_manifest(&valid, &manifest)).is_err(),
                "{label} was accepted"
            );
        }
        assert!(read_bytes(&with_manifest(&valid, &json!("not an object"))).is_err());
    }

    #[test]
    fn malformed_payloads_and_framing_are_rejected() {
        let content = two_layers();
        let valid = write_bytes(&content).unwrap();
        let manifest_end = 12 + u32::from_le_bytes(valid[8..12].try_into().unwrap()) as usize;
        let first_length =
            u64::from_le_bytes(valid[manifest_end..manifest_end + 8].try_into().unwrap());

        let mut bad_magic = valid.clone();
        bad_magic[0] = b'X';
        let mut huge_manifest = valid.clone();
        huge_manifest[8..12].copy_from_slice(&(MAX_MANIFEST_BYTES + 1).to_le_bytes());
        let mut extra = valid.clone();
        extra.push(0);
        let mut zero_length = valid.clone();
        zero_length[manifest_end..manifest_end + 8].copy_from_slice(&0u64.to_le_bytes());
        let mut overlong = valid.clone();
        overlong[manifest_end..manifest_end + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut not_png = valid.clone();
        not_png[manifest_end + 8] ^= 0xff;
        let mut wrong_size = valid[..manifest_end].to_vec();
        wrong_size.extend(payload(&RgbaImage::new(4, 2)));
        wrong_size.extend(&valid[manifest_end + 8 + first_length as usize..]);
        let mut missing_layer = valid[..manifest_end].to_vec();
        missing_layer.extend(payload(&content.layers()[0].pixels));

        let cases = [
            ("bad magic", bad_magic),
            ("short header", valid[..10].to_vec()),
            ("huge manifest", huge_manifest),
            ("short manifest", valid[..manifest_end - 1].to_vec()),
            ("truncated payload", valid[..valid.len() - 1].to_vec()),
            ("missing length", valid[..manifest_end + 4].to_vec()),
            ("missing layer", missing_layer),
            ("extra data", extra),
            ("zero length", zero_length),
            ("overlong length", overlong),
            ("not a png", not_png),
            ("wrong layer size", wrong_size),
        ];
        for (label, bytes) in cases {
            assert!(read_bytes(&bytes).is_err(), "{label} was accepted");
        }
    }

    #[test]
    fn writer_applies_the_reader_limits() {
        let base = two_layers();
        let layer = base.layers()[0].clone();
        let layers = (0..=MAX_LAYERS as u64)
            .map(|id| RasterLayer {
                id: LayerId(id),
                ..layer.clone()
            })
            .collect();
        let crowded = DocumentContent::from_layers(4, 3, layers).unwrap();
        let mut bytes = Vec::new();
        assert!(write(&mut bytes, &crowded).is_err());
        assert!(bytes.is_empty());
    }
}
