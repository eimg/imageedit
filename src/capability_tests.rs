//! Cross-feature regressions for the capability pass. Native interaction stays
//! with the maintainer; these exercise persisted pixels, metadata and budgets.
use crate::{
    adjustments::Adjustment,
    document::layer_effect::EffectSpec,
    document::{self, BlendMode, DocumentContent, LayerId, MaskEdit},
    preview::{Dirty, PixelRect},
    selection::Selection,
    transform::{self, Resample},
};
use image::{Rgba, RgbaImage};
use std::{
    io::Cursor,
    sync::{Arc, atomic::AtomicBool},
};

fn scene() -> DocumentContent {
    DocumentContent::single_layer(
        "Art".into(),
        RgbaImage::from_fn(6, 4, |x, y| Rgba([x as u8 * 35, y as u8 * 60, 90, 200])),
    )
}
fn soft_selection() -> Selection {
    Selection::from_coverage(6, 4, |x, y| ((x + y) * 28).min(255) as u8)
}
fn roundtrip(content: &DocumentContent) -> DocumentContent {
    let mut bytes = Vec::new();
    crate::project::write(&mut bytes, content).unwrap();
    crate::project::read(Cursor::new(bytes)).unwrap()
}
#[test]
fn masks_keep_source_pixels_round_trip_and_apply_the_displayed_alpha() {
    let mut content = scene();
    let id = content.top_layer_id();
    let source = content.layer(id).unwrap().pixels.clone();
    content.mask_from_selection(id, &soft_selection());
    let shown = content.composite();
    assert_eq!(content.layer(id).unwrap().pixels, source);
    assert_eq!(shown.get_pixel(2, 1)[3], ((200u16 * 84 + 127) / 255) as u8);
    let mut loaded = roundtrip(&content);
    assert_eq!(loaded.composite(), shown);
    loaded.edit_mask(id, MaskEdit::Toggle);
    assert_eq!(loaded.composite(), source);
    loaded.edit_mask(id, MaskEdit::Toggle);
    loaded.edit_mask(id, MaskEdit::Invert);
    assert_ne!(loaded.composite(), shown);
    loaded.edit_mask(id, MaskEdit::Invert);
    loaded.edit_mask(id, MaskEdit::Apply);
    assert!(loaded.layer(id).unwrap().mask.is_none());
    for (actual, expected) in loaded.composite().pixels().zip(shown.pixels()) {
        assert_eq!(actual[3], expected[3]);
        if actual[3] != 0 {
            assert_eq!(actual, expected);
        }
    }
}
#[test]
fn masks_follow_turn_flip_scale_crop_and_layer_offsets() {
    let mut content = scene();
    let id = content.top_layer_id();
    content.mask_from_selection(id, &soft_selection());
    let before = content.composite();
    assert!(content.turn_image(transform::Turn::Right));
    assert_eq!(*content.composite(), image::imageops::rotate90(&*before));
    assert!(content.mirror_image(transform::Mirror::Horizontal));
    let before = content.composite();
    assert!(content.scale_image(8, 12, Resample::Nearest));
    assert_eq!(
        *content.composite(),
        transform::scale_pixels(&before, 8, 12, Resample::Nearest)
    );
    let before = content.composite();
    assert!(content.crop(document::CropRect {
        x: 1,
        y: 2,
        width: 5,
        height: 7
    }));
    assert_eq!(
        *content.composite(),
        image::imageops::crop_imm(&*before, 1, 2, 5, 7).to_image()
    );
    let layer = content.layer(id).unwrap();
    assert_eq!(
        layer.mask.as_ref().unwrap().pixels.dimensions(),
        layer.pixels.dimensions()
    );
}
#[test]
fn retained_effect_reediting_replaces_parameters_and_identity_restores_source() {
    let mut content = scene();
    let id = content.top_layer_id();
    let selection = soft_selection();
    let original = content.clone();
    let invert = EffectSpec::Adjustment(Adjustment::Invert);
    assert_ne!(
        content.apply_retained_effect(id, invert, &selection, false),
        Dirty::None
    );
    let displayed = content.composite();
    assert_eq!(
        content.apply_retained_effect(id, invert, &selection, true),
        Dirty::None
    );
    let mut loaded = roundtrip(&content);
    assert_eq!(loaded.composite(), displayed);
    let brightness = Adjustment::BrightnessContrast {
        brightness: 30,
        contrast: -10,
    };
    assert_ne!(
        loaded.apply_retained_effect(
            id,
            EffectSpec::Adjustment(brightness),
            &Selection::Unrestricted,
            true
        ),
        Dirty::None
    );
    let mut expected = original.clone();
    expected.apply_adjustment(id, brightness, &selection);
    assert_eq!(loaded.composite(), expected.composite());
    assert_ne!(
        loaded.apply_retained_effect(
            id,
            EffectSpec::Adjustment(Adjustment::BrightnessContrast {
                brightness: 0,
                contrast: 0
            }),
            &Selection::Empty,
            true
        ),
        Dirty::None
    );
    assert_eq!(loaded.composite(), original.composite());
    assert!(loaded.layer(id).unwrap().effect.is_none());
}
#[test]
fn retained_filter_mask_and_blend_persist_together_and_bake_on_pixel_edits() {
    let mut content = scene();
    let id = content.top_layer_id();
    content.mask_from_selection(id, &soft_selection());
    content.change_blend_mode(id, BlendMode::Multiply);
    let original = content.layer(id).unwrap().pixels.clone();
    content.apply_retained_effect(
        id,
        EffectSpec::Blur { radius: 2 },
        &Selection::Unrestricted,
        false,
    );
    let mut loaded = roundtrip(&content);
    assert_eq!(loaded.composite(), content.composite());
    assert_eq!(
        loaded.layer(id).unwrap().effect.as_ref().unwrap().source,
        original
    );
    let shown = loaded.layer(id).unwrap().pixels.clone();
    assert_eq!(loaded.remove_retained_effect(id, true), Dirty::Unpainted);
    assert_eq!(loaded.layer(id).unwrap().pixels, shown);
    assert!(loaded.layer(id).unwrap().effect.is_none());
    content.remove_retained_effect(id, false);
    assert_eq!(content.layer(id).unwrap().pixels, original);
    content.apply_retained_effect(
        id,
        EffectSpec::Adjustment(Adjustment::Invert),
        &Selection::Unrestricted,
        false,
    );
    assert!(content.turn_layer(id, transform::Turn::Right));
    assert!(content.layer(id).unwrap().effect.is_none());
    assert_eq!(
        content.layer(id).unwrap().pixels.dimensions(),
        content
            .layer(id)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .pixels
            .dimensions()
    );
}
#[test]
fn retained_coverage_stays_layer_local_after_moving_off_canvas() {
    let mut content = scene();
    let id = content.top_layer_id();
    let original = content.layer(id).unwrap().pixels.clone();
    content.apply_retained_effect(
        id,
        EffectSpec::Adjustment(Adjustment::Invert),
        &soft_selection(),
        false,
    );
    content.set_offset(id, (-4, -2));
    let effect = EffectSpec::Adjustment(Adjustment::BrightnessContrast {
        brightness: 25,
        contrast: 0,
    });
    content.apply_retained_effect(id, effect, &Selection::Empty, true);
    let mut expected = DocumentContent::single_layer("Expected".into(), (*original).clone());
    expected.apply_adjustment(
        id,
        Adjustment::BrightnessContrast {
            brightness: 25,
            contrast: 0,
        },
        &soft_selection(),
    );
    assert_eq!(
        content.layer(id).unwrap().pixels,
        expected.layer(id).unwrap().pixels
    );
}
#[test]
fn retained_effect_admission_rejects_a_full_shared_stack_without_changes() {
    let image = Arc::new(RgbaImage::new(1024, 1024));
    let template =
        DocumentContent::single_layer("Layer".into(), RgbaImage::new(1, 1)).layers()[0].clone();
    let layers = (0..128)
        .map(|index| document::RasterLayer {
            id: LayerId(index),
            pixels: image.clone(),
            ..template.clone()
        })
        .collect();
    let mut content = DocumentContent::from_layers(1, 1, layers).unwrap();
    assert!(!content.can_retain_effect(LayerId(0), &Selection::Unrestricted, false));
    assert_eq!(
        content.apply_retained_effect(
            LayerId(0),
            EffectSpec::Blur { radius: 1 },
            &Selection::Unrestricted,
            false
        ),
        Dirty::None
    );
    assert!(content.layers().iter().all(|layer| layer.effect.is_none()));
}
#[test]
fn feather_geometry_survives_edits_without_repeatedly_blurring_the_mask() {
    let hard = Selection::ellipse_rect_quality(PixelRect::new(-2, 1, 8, 5), 12, 10, true);
    let soft = hard.feather(2, 12, 10);
    let geometry = soft.feather_geometry().unwrap();
    assert_eq!(geometry.rect, PixelRect::new(-2, 1, 8, 5));
    let moved =
        geometry
            .shape(PixelRect::new(2, 3, 7, 6), 12, 10)
            .feather(geometry.feather, 12, 10);
    assert_eq!(moved.feather_geometry().unwrap().feather, 2);
    let expected = Selection::ellipse_rect_quality(PixelRect::new(2, 3, 7, 6), 12, 10, true)
        .feather(2, 12, 10);
    assert_eq!(moved, expected);
    let combo = soft.combine(
        Selection::Rect(PixelRect::new(10, 8, 1, 1)),
        crate::selection::SelectionOp::Add,
        12,
        10,
    );
    assert!(combo.feather_geometry().is_none());
}
#[test]
fn wand_recovers_color_and_alpha_edges_without_expanding_connectivity() {
    let image = RgbaImage::from_fn(5, 1, |x, _| match x {
        0 | 1 => Rgba([255, 255, 255, 255]),
        2 => Rgba([64, 64, 64, 255]),
        _ => Rgba([0, 0, 0, 255]),
    });
    let matching = crate::selection::RegionMatch::default();
    let hard =
        Selection::flood_region_quality(&image, (0, 0), matching, (0, 0), (5, 1), None, false);
    let soft =
        Selection::flood_region_quality(&image, (0, 0), matching, (0, 0), (5, 1), None, true);
    assert_eq!(hard.coverage(2, 0, 5, 1), 0);
    assert_eq!(soft.coverage(2, 0, 5, 1), 64);
    assert_eq!(soft.coverage(3, 0, 5, 1), 0);
    let clear = RgbaImage::from_fn(5, 1, |x, _| match x {
        0 | 1 => Rgba([10, 99, 200, 0]),
        2 => Rgba([0, 0, 0, 192]),
        _ => Rgba([0, 0, 0, 255]),
    });
    let selected =
        Selection::flood_region_quality(&clear, (0, 0), matching, (0, 0), (5, 1), None, true);
    assert_eq!(selected.coverage(2, 0, 5, 1), 63);
}
#[test]
fn recovery_skips_live_sessions_recovers_crashes_and_never_overwrites_source() {
    let root = tempfile::tempdir().unwrap();
    let (store, docs, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(docs.is_empty() && errors.is_empty());
    let content = scene();
    let alive = AtomicBool::new(true);
    store
        .snapshot(7, "Test image", &content, None, &alive)
        .unwrap();
    let (other, docs, _) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(docs.is_empty());
    other.clean_exit();
    drop(other);
    drop(store);
    let (reopened, docs, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(docs.len(), 1);
    assert!(docs[0].document.path.is_none());
    assert_eq!(docs[0].document.name, "Recovered Test image");
    assert_eq!(docs[0].document.content.composite(), content.composite());
    reopened.clean_exit();
    let (empty, docs, _) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(docs.is_empty());
    empty.clean_exit();
}

#[test]
fn checked_document_preparation_cancels_inside_work_and_keeps_tiles_consistent() {
    let mut content = scene();
    let id = content.top_layer_id();
    content.add_layer(id, "Top".into(), Rgba([10, 20, 30, 120]));
    let expected = content.composite();
    let prepared =
        document::Document::from_content_checked("Prepared".into(), content.clone(), &|| Ok(()))
            .unwrap();
    assert_eq!(prepared.canvas.pixels, expected);
    for cache in prepared.canvas.level_caches() {
        for span in cache.spans() {
            assert_eq!(
                cache.slot(span.index).unwrap().as_ref(),
                &image::imageops::crop_imm(
                    &*expected,
                    span.stored.x as u32,
                    span.stored.y as u32,
                    span.stored.width,
                    span.stored.height
                )
                .to_image()
            );
        }
    }
    let checks = std::cell::Cell::new(0);
    let result =
        document::Document::from_content_checked("Canceled".into(), content.clone(), &|| {
            let count = checks.get() + 1;
            checks.set(count);
            if count >= 4 {
                Err(crate::pdf::PdfCanceled.into())
            } else {
                Ok(())
            }
        });
    assert!(result.err().unwrap().is::<crate::pdf::PdfCanceled>());
    assert_eq!(content.composite(), expected);
}

#[test]
fn corrupt_recovery_files_survive_clean_exit_and_canceled_snapshots_do_not_reappear() {
    let root = tempfile::tempdir().unwrap();
    let (store, _, _) = crate::recovery::Store::open(root.path()).unwrap();
    let session = std::fs::read_dir(root.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let damaged = session.join("tab-99.lime");
    std::fs::write(&damaged, b"IMGEDIT\0bad header").unwrap();
    let dead = AtomicBool::new(false);
    store
        .snapshot(0, "Canceled", &scene(), None, &dead)
        .unwrap();
    assert!(!session.join("tab-0.lime").exists());
    drop(store);
    let (reopened, documents, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(documents.is_empty());
    assert!(!errors.is_empty());
    reopened.clean_exit();
    assert!(damaged.exists());
}

#[test]
fn recovered_failed_text_source_and_name_survive_repeated_crashes() {
    let root = tempfile::tempdir().unwrap();
    let content = scene();
    let text = serde_json::json!({"source":"Preserve this draft even when its renderer fails"});
    let (first, _, _) = crate::recovery::Store::open(root.path()).unwrap();
    first
        .snapshot(5, "Original", &content, Some(&text), &AtomicBool::new(true))
        .unwrap();
    drop(first);
    let (second, documents, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(documents[0].text.as_ref(), Some(&text));
    drop(second);
    let (third, documents, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(documents[0].text.as_ref(), Some(&text));
    assert_eq!(documents[0].document.name, "Recovered Original");
    third.clean_exit();
}

#[test]
fn transforming_a_mask_on_uniform_pixels_is_still_a_real_edit() {
    let mut content = DocumentContent::single_layer(
        "Uniform".into(),
        RgbaImage::from_pixel(4, 2, Rgba([80, 100, 120, 255])),
    );
    let id = content.top_layer_id();
    content.mask_from_selection(id, &Selection::Rect(PixelRect::new(0, 0, 2, 2)));
    let before = content.composite();
    let layer = content.layer(id).unwrap();
    let mut mask = layer.mask.clone().unwrap();
    mask.pixels = Arc::new(image::imageops::flip_horizontal(&*mask.pixels));
    let pixels = (*layer.pixels).clone();
    let offset = layer.offset;
    assert_ne!(
        content.place_layer_with_mask(id, pixels, offset, Some(mask)),
        Dirty::None
    );
    assert_eq!(
        *content.composite(),
        image::imageops::flip_horizontal(&*before)
    );
}

#[test]
fn changing_retained_parameters_keeps_them_even_when_output_pixels_match() {
    let mut content = DocumentContent::single_layer(
        "Flat".into(),
        RgbaImage::from_pixel(2, 2, Rgba([128, 128, 128, 255])),
    );
    let id = content.top_layer_id();
    content.apply_retained_effect(
        id,
        EffectSpec::Adjustment(Adjustment::BrightnessContrast {
            brightness: 100,
            contrast: 0,
        }),
        &Selection::Unrestricted,
        false,
    );
    let before = content.composite();
    let changed = EffectSpec::Adjustment(Adjustment::BrightnessContrast {
        brightness: 100,
        contrast: 50,
    });
    assert_eq!(
        content.apply_retained_effect(id, changed, &Selection::Unrestricted, true),
        Dirty::Unpainted
    );
    assert_eq!(content.composite(), before);
    assert_eq!(
        roundtrip(&content)
            .layer(id)
            .unwrap()
            .effect
            .as_ref()
            .unwrap()
            .spec,
        changed
    );
}

#[test]
fn a_failed_recovery_replacement_keeps_its_previous_embedded_draft() {
    let root = tempfile::tempdir().unwrap();
    let (store, _, _) = crate::recovery::Store::open(root.path()).unwrap();
    let session = std::fs::read_dir(root.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = serde_json::json!({"source":"Last complete recovery draft"});
    store
        .snapshot(2, "Atomic", &scene(), Some(&text), &AtomicBool::new(true))
        .unwrap();
    let original = std::fs::read(session.join("tab-2.lime")).unwrap();
    let quota = std::fs::File::create(session.join("quota-test")).unwrap();
    quota.set_len(1024 * 1024 * 1024).unwrap();
    assert!(
        store
            .snapshot(2, "Replacement", &scene(), None, &AtomicBool::new(true))
            .is_err()
    );
    assert_eq!(std::fs::read(session.join("tab-2.lime")).unwrap(), original);
    drop(quota);
    std::fs::remove_file(session.join("quota-test")).unwrap();
    drop(store);
    let (recovered, documents, errors) = crate::recovery::Store::open(root.path()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(documents[0].text.as_ref(), Some(&text));
    assert_eq!(documents[0].document.name, "Recovered Atomic");
    recovered.clean_exit();
}
