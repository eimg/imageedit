use std::path::PathBuf;

use gpui::{
    ClipboardEntry, ClipboardItem, Context, Div, Image, ImageFormat, PathPromptOptions, Window,
    prelude::*,
};
use image::RgbaImage;

use super::Workspace;
use super::pdf_ui::PdfTarget;
use crate::commands::{
    CopyImage, CutImage, NewFromClipboard, OpenAsLayers, PasteAsLayer, PasteImage,
};
use crate::document::{self, ImageLayer, InsertStop};
use crate::preview::Dirty;
use crate::selection::Selection;

impl Workspace {
    pub(super) fn bind_clipboard_actions(&self, mut root: Div, cx: &Context<Self>) -> Div {
        if !self.creating {
            root = root.on_action(cx.listener(|this, _: &NewFromClipboard, window, cx| {
                this.new_from_clipboard(window, cx);
            }));
        }
        if self.text_draft.is_some() {
            return root
                .on_action(cx.listener(|this, _: &CopyImage, _, cx| this.copy_text(cx)))
                .on_action(cx.listener(|this, _: &CutImage, window, cx| this.cut_text(window, cx)))
                .on_action(
                    cx.listener(|this, _: &PasteImage, window, cx| this.paste_text(window, cx)),
                );
        }
        if !self.clipboard_target() {
            return root;
        }
        root = root.on_action(
            cx.listener(|this, _: &CopyImage, window, cx| this.copy_image(false, window, cx)),
        );
        let visible = self.active_tab().is_some_and(|tab| {
            tab.document
                .content
                .layer(tab.active_layer)
                .is_some_and(|layer| layer.visible)
        });
        if visible
            && self.active_tab().is_some_and(|tab| {
                tab.document
                    .content
                    .layer(tab.active_layer)
                    .is_some_and(|layer| layer.text.is_none() && !layer.locks.alpha)
            })
        {
            root = root.on_action(
                cx.listener(|this, _: &CutImage, window, cx| this.copy_image(true, window, cx)),
            );
        }
        let room = self
            .active_tab()
            .is_some_and(|tab| tab.document.content.can_add_layer());
        if room {
            root = root
                .on_action(
                    cx.listener(|this, _: &PasteImage, window, cx| this.paste_layer(window, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &PasteAsLayer, window, cx| this.paste_layer(window, cx)),
                )
                .on_action(cx.listener(|this, _: &OpenAsLayers, window, cx| {
                    this.open_as_layers(window, cx);
                }));
        }
        root
    }

    fn clipboard_target(&self) -> bool {
        self.move_drag.is_none()
            && !self.has_pending_text()
            && self.active_tab().is_some_and(|tab| !tab.busy())
    }

    fn copy_image(&mut self, cut: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.clipboard_target() {
            return;
        }
        if cut && self.refuse_text_pixels(false, cx) {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id;
        let generation = tab.history.generation();
        let layer_id = tab.active_layer;
        let Some(layer) = tab.document.content.layer(layer_id) else {
            return;
        };
        if cut && layer.locks.alpha {
            self.notice("Unlock the layer's alpha before cutting pixels.", cx);
            return;
        }
        if cut && !layer.visible {
            self.notice("The active layer is hidden. Show it before cutting.", cx);
            return;
        }
        if tab.selection == Selection::Empty {
            self.notice("Nothing is selected.", cx);
            return;
        }
        let pixels = layer.pixels.clone();
        let mask = layer.mask.clone().filter(|mask| mask.enabled);
        let offset = layer.offset;
        let selection = tab.selection.clone();
        let canvas = tab.document.content.size();
        self.copy_token = self.copy_token.wrapping_add(1);
        let token = self.copy_token;
        let job = cx.background_executor().spawn(async move {
            let masked = mask.map(|mask| {
                image::RgbaImage::from_fn(pixels.width(), pixels.height(), |x, y| {
                    let mut pixel = *pixels.get_pixel(x, y);
                    pixel[3] = ((u16::from(pixel[3]) * u16::from(mask.pixels.get_pixel(x, y)[3])
                        + 127)
                        / 255) as u8;
                    pixel
                })
            });
            let clipped = clip_layer(
                offset,
                masked.as_ref().unwrap_or(&pixels),
                selection,
                canvas,
            )?;
            document::encode_png(&clipped).ok()
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let png = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.copy_token != token {
                    return;
                }
                let Some(index) = workspace.tab_index(tab_id) else {
                    return;
                };
                let Some(png) = png else {
                    workspace.tabs[index].notice =
                        Some("The selection does not cover the active layer.".into());
                    cx.notify();
                    return;
                };
                cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                    ImageFormat::Png,
                    png.clone(),
                )));
                if !clipboard_has_png(cx.read_from_clipboard().as_ref(), &png) {
                    workspace.tabs[index].notice =
                        Some("The image could not be copied to the clipboard.".into());
                    cx.notify();
                    return;
                }
                if !cut {
                    workspace.tabs[index].notice = None;
                    cx.notify();
                    return;
                }
                let tab = &workspace.tabs[index];
                if workspace.active != Some(index)
                    || tab.history.generation() != generation
                    || tab.active_layer != layer_id
                    || tab.busy()
                {
                    workspace.tabs[index].notice = Some(
                        "The image was copied, but the layer changed before it could be cut."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                workspace.edit_content(
                    move |content, _, selection| {
                        content
                            .clear(layer_id, selection)
                            .map_or(Dirty::None, Dirty::Region)
                    },
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    // Paste and Paste as New Layer both add a layer. There is no floating paste.
    fn paste_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.clipboard_target() {
            return;
        }
        let Some(bytes) = cx
            .read_from_clipboard()
            .as_ref()
            .and_then(clipboard_image_bytes)
            .map(|bytes| bytes.to_vec())
        else {
            self.notice("The clipboard has no image.", cx);
            return;
        };
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id;
        let above = tab.active_layer;
        let generation = tab.history.generation();
        self.paste_token = self.paste_token.wrapping_add(1);
        let token = self.paste_token;
        let job = cx
            .background_executor()
            .spawn(async move { document::decode_raster_bytes(&bytes).ok() });
        cx.spawn_in(window, async move |workspace, cx| {
            let pixels = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.paste_token != token
                    || workspace.active_tab().is_none_or(|tab| {
                        tab.id != tab_id
                            || tab.history.generation() != generation
                            || tab.active_layer != above
                            || tab.busy()
                    })
                {
                    return;
                }
                let Some(pixels) = pixels else {
                    workspace.notice("The clipboard image could not be read.", cx);
                    return;
                };
                if let Err(stop) = workspace
                    .active_tab()
                    .unwrap()
                    .document
                    .content
                    .room_for(0, 0, &pixels)
                {
                    workspace.notice(insert_stop_message(stop), cx);
                    return;
                }
                let image = ImageLayer {
                    name: "Pasted Layer".into(),
                    pixels,
                };
                workspace.edit_content(
                    move |content, active, _| {
                        let inserted = content.insert_centered_layers(above, vec![image]);
                        if let Some(top) = inserted.top {
                            *active = top;
                        }
                        if inserted.added == 0 {
                            Dirty::None
                        } else {
                            Dirty::Canvas
                        }
                    },
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    fn new_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.creating || self.form.is_some() || self.dialog_open {
            return;
        }
        let Some(bytes) = cx
            .read_from_clipboard()
            .as_ref()
            .and_then(clipboard_image_bytes)
            .map(|bytes| bytes.to_vec())
        else {
            self.tell("The clipboard has no image.", cx);
            return;
        };
        self.creating = true;
        let name = format!("Untitled {}", self.next_untitled);
        let job = cx.background_executor().spawn(async move {
            document::decode_raster_bytes(&bytes)
                .ok()
                .map(|pixels| document::Document::from_pixels(name, pixels))
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let document = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.creating = false;
                match document {
                    Some(document) => {
                        workspace.next_untitled += 1;
                        workspace.show_document(document, window, cx);
                    }
                    None => workspace.tell("The clipboard image could not be read.", cx),
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn open_as_layers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_open || self.form.is_some() || !self.clipboard_target() {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        if !tab.document.content.can_add_layer() {
            self.notice("The document cannot hold another layer.", cx);
            return;
        }
        let tab_id = tab.id;
        self.dialog_open = true;
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open as Layers".into()),
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let result = prompt.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                workspace.dialog_open = false;
                match result {
                    Ok(Ok(Some(paths))) if !paths.is_empty() => {
                        if paths.iter().any(|path| crate::pdf::is_pdf_path(path)) {
                            if paths.len() != 1 {
                                workspace.notice(
                                    "Open PDF files one at a time with Open as Layers.",
                                    cx,
                                );
                            } else if workspace.active_tab().map(|tab| tab.id) != Some(tab_id) {
                                workspace.tell(
                                    "Open as Layers was canceled because the image changed.",
                                    cx,
                                );
                            } else if let Some(tab) = workspace.active_tab() {
                                if !tab.document.content.can_add_layer() {
                                    workspace.notice("The document cannot hold another layer.", cx);
                                } else {
                                    let target = PdfTarget::Layers {
                                        tab_id,
                                        generation: tab.history.generation(),
                                        above: tab.active_layer,
                                    };
                                    workspace.request_pdf_import(
                                        paths[0].clone(),
                                        target,
                                        window,
                                        cx,
                                    );
                                }
                            }
                        } else {
                            workspace.begin_import(tab_id, paths, window, cx);
                        }
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => workspace
                        .errors
                        .push(format!("Could not open the file picker: {error}")),
                    Err(_) => workspace
                        .errors
                        .push("The file picker closed unexpectedly.".into()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn begin_import(
        &mut self,
        tab_id: usize,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().map(|tab| tab.id) != Some(tab_id) || !self.clipboard_target() {
            self.tell("Open as Layers was canceled because the image changed.", cx);
            return;
        }
        let tab = self.active_tab().unwrap();
        let generation = tab.history.generation();
        let above = tab.active_layer;
        let content = tab.document.content.clone();
        let job = cx
            .background_executor()
            .spawn(async move { load_imports(&content, paths, document::read_raster) });
        cx.spawn_in(window, async move |workspace, cx| {
            let (loaded, failed) = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.active_tab().is_none_or(|tab| {
                    tab.id != tab_id
                        || tab.history.generation() != generation
                        || tab.active_layer != above
                }) {
                    workspace.tell("Open as Layers was canceled because the image changed.", cx);
                    return;
                }
                workspace.finish_import(tab_id, loaded, failed, window, cx);
            });
        })
        .detach();
    }

    fn finish_import(
        &mut self,
        tab_id: usize,
        loaded: Vec<ImageLayer>,
        mut failed: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().map(|tab| tab.id) != Some(tab_id) || !self.clipboard_target() {
            self.tell("Open as Layers was canceled because the image changed.", cx);
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let above = tab.active_layer;
        let mut accepted = Vec::new();
        let mut extra_layers = 0;
        let mut extra_bytes = 0;
        for layer in loaded {
            match tab
                .document
                .content
                .room_for(extra_layers, extra_bytes, &layer.pixels)
            {
                Ok(()) => {
                    extra_bytes += u64::from(layer.pixels.width())
                        .saturating_mul(u64::from(layer.pixels.height()))
                        .saturating_mul(4);
                    extra_layers += 1;
                    accepted.push(layer);
                }
                Err(InsertStop::Full) => {
                    failed.push("The document cannot hold another layer.".into());
                    break;
                }
                Err(InsertStop::TooLarge) => {
                    failed.push(format!("{} is too large to add.", layer.name));
                }
            }
        }
        let added = accepted.len();
        if added > 0 {
            self.edit_content(
                move |content, active, _| {
                    let inserted = content.insert_centered_layers(above, accepted);
                    if let Some(top) = inserted.top {
                        *active = top;
                    }
                    if inserted.added == 0 {
                        Dirty::None
                    } else {
                        Dirty::Canvas
                    }
                },
                window,
                cx,
            );
        }
        if let Some(message) = failure_note(&failed, added) {
            self.notice(&message, cx);
        }
    }

    fn tell(&mut self, message: &str, cx: &mut Context<Self>) {
        if self.active.is_some() {
            self.notice(message, cx);
        } else {
            self.errors.push(message.into());
            cx.notify();
        }
    }
}

// Accept each decoded image before loading the next; a batch cannot retain more
// pixels or layers than the document can actually use.
fn load_imports(
    content: &document::DocumentContent,
    paths: Vec<PathBuf>,
    mut decode: impl FnMut(&std::path::Path) -> anyhow::Result<RgbaImage>,
) -> (Vec<ImageLayer>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut failed = Vec::new();
    let mut extra_bytes = 0;
    for path in paths {
        if content.layers().len() + loaded.len() >= document::MAX_LAYERS {
            failed.push("The document cannot hold another layer.".into());
            break;
        }
        match decode(&path) {
            Ok(pixels) => match content.room_for(loaded.len(), extra_bytes, &pixels) {
                Ok(()) => {
                    extra_bytes += pixels.as_raw().len() as u64;
                    loaded.push(ImageLayer {
                        name: import_name(&path),
                        pixels,
                    });
                }
                Err(stop) => failed.push(format!(
                    "{}: {}",
                    document::file_name(&path),
                    insert_stop_message(stop)
                )),
            },
            Err(error) => failed.push(format!("{}: {error}", document::file_name(&path))),
        }
    }
    (loaded, failed)
}

// The copied image is the part of the layer the selection covers, including transparent pixels.
fn clip_layer(
    offset: (i32, i32),
    pixels: &RgbaImage,
    selection: Selection,
    canvas: (u32, u32),
) -> Option<RgbaImage> {
    let (width, height) = pixels.dimensions();
    let origin_x = i64::from(offset.0);
    let origin_y = i64::from(offset.1);
    let (mut min_x, mut min_y) = (width, height);
    let (mut max_x, mut max_y) = (0, 0);
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            if !selection.covers(
                origin_x + i64::from(x),
                origin_y + i64::from(y),
                canvas.0,
                canvas.1,
            ) {
                continue;
            }
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if !found {
        return None;
    }
    let mut image = RgbaImage::new(max_x - min_x + 1, max_y - min_y + 1);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let canvas_x = origin_x + i64::from(x);
            let canvas_y = origin_y + i64::from(y);
            let coverage = selection.coverage(canvas_x, canvas_y, canvas.0, canvas.1);
            if coverage > 0 {
                let mut pixel = *pixels.get_pixel(x, y);
                pixel[3] = crate::selection::apply_coverage(pixel[3], coverage);
                image.put_pixel(x - min_x, y - min_y, pixel);
            }
        }
    }
    Some(image)
}

fn clipboard_image_bytes(item: &ClipboardItem) -> Option<&[u8]> {
    let mut fallback = None;
    for entry in item.entries() {
        let ClipboardEntry::Image(image) = entry else {
            continue;
        };
        if image.format == ImageFormat::Png {
            return Some(&image.bytes);
        }
        if fallback.is_none() && matches!(image.format, ImageFormat::Jpeg | ImageFormat::Webp) {
            fallback = Some(image.bytes.as_slice());
        }
    }
    fallback
}

fn clipboard_has_png(item: Option<&ClipboardItem>, png: &[u8]) -> bool {
    item.and_then(clipboard_image_bytes)
        .is_some_and(|read| png_matches(png, read))
}

fn png_matches(written: &[u8], read: &[u8]) -> bool {
    if written == read {
        return true;
    }
    match (
        document::decode_raster_bytes(written),
        document::decode_raster_bytes(read),
    ) {
        (Ok(left), Ok(right)) => {
            left.dimensions() == right.dimensions() && left.as_raw() == right.as_raw()
        }
        _ => false,
    }
}

fn import_name(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Layer".into())
}

fn insert_stop_message(stop: InsertStop) -> &'static str {
    match stop {
        InsertStop::Full => "The document cannot hold another layer.",
        InsertStop::TooLarge => "The image is too large to add.",
    }
}

fn failure_note(failed: &[String], added: usize) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let mut shown: Vec<_> = failed.iter().take(3).cloned().collect();
    if failed.len() > 3 {
        shown.push(format!("and {} more.", failed.len() - 3));
    }
    let detail = shown.join(" ");
    Some(if added == 0 {
        detail
    } else {
        let layers = if added == 1 { "layer" } else { "layers" };
        format!("Added {added} {layers}. {detail}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::PixelRect;
    use image::Rgba;

    #[test]
    fn copy_keeps_transparent_pixels_and_clips_to_the_selection() {
        let mut pixels = RgbaImage::new(3, 1);
        pixels.put_pixel(0, 0, Rgba([1, 2, 3, 128]));
        pixels.put_pixel(1, 0, Rgba([4, 0, 0, 255]));
        pixels.put_pixel(2, 0, Rgba([9, 9, 9, 255]));
        let copied = clip_layer((-1, 0), &pixels, Selection::Unrestricted, (2, 2)).unwrap();
        assert_eq!(copied.get_pixel(0, 0).0, [1, 2, 3, 128]);
        let png = document::encode_png(&copied).unwrap();
        let round = document::decode_raster_bytes(&png).unwrap();
        assert_eq!(round.as_raw(), copied.as_raw());

        let selected = clip_layer(
            (-1, 0),
            &pixels,
            Selection::Rect(PixelRect::new(0, 0, 1, 1)),
            (2, 2),
        )
        .unwrap();
        assert_eq!(selected.dimensions(), (1, 1));
        assert_eq!(selected.get_pixel(0, 0).0, [4, 0, 0, 255]);
        assert!(
            clip_layer(
                (0, 0),
                &pixels,
                Selection::Rect(PixelRect::new(0, 1, 1, 1)),
                (3, 2),
            )
            .is_none()
        );
    }

    #[test]
    fn copied_soft_masks_keep_their_color_and_use_the_same_coverage_alpha() {
        let pixels = RgbaImage::from_pixel(3, 3, Rgba([10, 20, 30, 128]));
        let mask = Selection::rectangle((1, 1), (1, 1), 3, 3).feather(1, 3, 3);
        let clipped = clip_layer((0, 0), &pixels, mask, (3, 3)).unwrap();
        assert_eq!(clipped.get_pixel(1, 1).0, [10, 20, 30, 14]);
        let source = RgbaImage::from_fn(3, 3, |x, y| {
            Rgba(if (x, y) == (1, 1) {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            })
        });
        let wand = Selection::flood_region_quality(
            &source,
            (1, 1),
            crate::selection::RegionMatch::default(),
            (0, 0),
            (3, 3),
            None,
            true,
        );
        let copied = clip_layer((0, 0), &pixels, wand, (3, 3)).unwrap();
        assert_eq!(copied.dimensions(), (1, 1));
        assert_eq!(copied.get_pixel(0, 0).0, [10, 20, 30, 96]);
    }

    #[test]
    fn an_inverse_selection_leaves_the_hole_transparent() {
        let mut pixels = RgbaImage::from_pixel(3, 3, Rgba([7, 0, 0, 255]));
        pixels.put_pixel(1, 1, Rgba([1, 1, 1, 200]));
        let copied = clip_layer(
            (0, 0),
            &pixels,
            Selection::Inverse(PixelRect::new(1, 1, 1, 1)),
            (3, 3),
        )
        .unwrap();
        assert_eq!(copied.dimensions(), (3, 3));
        assert_eq!(copied.get_pixel(1, 1).0, [0, 0, 0, 0]);
        assert_eq!(copied.get_pixel(0, 0).0, [7, 0, 0, 255]);
    }

    #[test]
    fn png_readback_matches_the_same_pixels() {
        let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 0]));
        let png = document::encode_png(&image).unwrap();
        assert!(png_matches(&png, &png));
        let other =
            document::encode_png(&RgbaImage::from_pixel(1, 1, Rgba([1, 0, 0, 255]))).unwrap();
        assert!(!png_matches(&png, &other));
        assert!(
            failure_note(&["a.txt: bad".into()], 2)
                .unwrap()
                .starts_with("Added 2 layers.")
        );
        assert!(failure_note(&[], 1).is_none());
    }

    #[test]
    fn read_raster_keeps_transparency_and_rejects_a_project() {
        let directory = tempfile::tempdir().unwrap();
        let mut pixels = RgbaImage::new(2, 1);
        pixels.put_pixel(1, 0, Rgba([8, 0, 0, 64]));
        let png = directory.path().join("mark.png");
        let content = document::DocumentContent::single_layer("Image".into(), pixels.clone());
        document::save_image(&content, &png, document::FileFormat::Png).unwrap();
        assert_eq!(
            document::read_raster(&png).unwrap().as_raw(),
            pixels.as_raw()
        );
        let project = directory.path().join("layers.imageedit");
        document::save_image(&content, &project, document::FileFormat::Project).unwrap();
        assert!(document::read_raster(&project).is_err());
        let text = directory.path().join("notes.txt");
        std::fs::write(&text, b"hello").unwrap();
        assert!(document::read_raster(&text).is_err());
    }
    #[test]
    fn imports_stop_decoding_at_the_layer_budget_and_keep_valid_files_after_errors() {
        let content = document::DocumentContent::single_layer("Image".into(), RgbaImage::new(1, 1));
        let mut attempts = 0;
        let paths = (0..1000)
            .map(|i| PathBuf::from(format!("{i}.png")))
            .collect();
        let (loaded, failed) = load_imports(&content, paths, |_| {
            attempts += 1;
            if attempts == 2 {
                anyhow::bail!("Invalid image");
            }
            Ok(RgbaImage::new(1, 1))
        });
        assert_eq!(loaded.len(), document::MAX_LAYERS - 1);
        assert_eq!(attempts, document::MAX_LAYERS);
        assert_eq!(failed.len(), 2);
        assert_eq!(loaded[0].name, "0");
        assert_eq!(loaded[1].name, "2");
    }
}
