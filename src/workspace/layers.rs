use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui::{
    Action, AnyView, App, BorderStyle, Bounds, ClickEvent, Context, CursorStyle, Div, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, Point, RenderImage, SharedString, Stateful,
    Window, canvas, div, img, point, prelude::*, px, quad, rgb, size,
};
use image::RgbaImage;

use super::Workspace;
use super::view::{
    ACCENT, BORDER, CANVAS, CHROME, DISABLED, HOVER, MUTED, SELECTED, TEXT, icon, section_label,
};
use crate::commands::{
    AlignLayer, CropImageToContent, CropLayerToContent, DeleteLayer, DuplicateLayer, LayerOpacity,
    LayerToImageSize, LowerLayer, MergeDown, NewLayer, RaiseLayer, RenameLayer,
    ToggleLayerVisibility,
};
use crate::document::{self, LayerGeometry, LayerId};
use crate::geometry::{self, Axis, ContentCrop};
use crate::preview::Dirty;

pub(super) struct MoveDrag {
    layer: LayerId,
    origin: (i32, i32),
    pub(super) offset: (i32, i32),
    anchor: (i64, i64),
    anchor_screen: Point<Pixels>,
}

pub(super) struct MoveOverlay {
    base: Arc<RenderImage>,
    sprite: Arc<RenderImage>,
    offset: (i32, i32),
    size: (u32, u32),
}

#[derive(Clone)]
pub(super) struct MovePaint {
    pub base: Arc<RenderImage>,
    pub sprite: Arc<RenderImage>,
    pub offset: (i32, i32),
    pub size: (u32, u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LayerCommand {
    New,
    Duplicate,
    Rename,
    MergeDown,
    Delete,
    ToggleVisibility,
    Opacity,
    Raise,
    Lower,
}

#[derive(Clone)]
pub(super) struct DraggedLayer {
    id: LayerId,
    name: SharedString,
}

impl Render for DraggedLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        label_view(self.name.clone()).border_color(rgb(ACCENT))
    }
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        label_view(self.0.clone())
    }
}

fn label_view(text: SharedString) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CHROME))
        .text_color(rgb(TEXT))
        .text_size(px(12.))
        .child(text)
}

pub(super) fn opacity_percent(opacity: u8) -> u32 {
    (u32::from(opacity) * 100 + 127) / 255
}

pub(super) fn opacity_from_percent(percent: u32) -> u8 {
    ((percent.min(100) * 255 + 50) / 100) as u8
}

impl Workspace {
    pub(super) fn layer_command_enabled(&self, command: LayerCommand) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        if tab.busy() || self.form.is_some() || self.dialog_open {
            return false;
        }
        let content = &tab.document.content;
        let count = content.layers().len();
        let index = content.layer_index(tab.active_layer);
        match command {
            LayerCommand::New | LayerCommand::Duplicate => content.can_add_layer(),
            LayerCommand::Rename | LayerCommand::ToggleVisibility | LayerCommand::Opacity => true,
            LayerCommand::MergeDown => content.can_merge_down(tab.active_layer),
            LayerCommand::Delete => count > 1,
            LayerCommand::Raise => index.is_some_and(|index| index + 1 < count),
            LayerCommand::Lower => index.is_some_and(|index| index > 0),
        }
    }

    pub(super) fn bind_layer_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let root = self.bind::<NewLayer>(root, LayerCommand::New, cx);
        let root = self.bind::<DuplicateLayer>(root, LayerCommand::Duplicate, cx);
        let root = self.bind::<RenameLayer>(root, LayerCommand::Rename, cx);
        let root = self.bind::<MergeDown>(root, LayerCommand::MergeDown, cx);
        let root = self.bind::<DeleteLayer>(root, LayerCommand::Delete, cx);
        let root = self.bind::<ToggleLayerVisibility>(root, LayerCommand::ToggleVisibility, cx);
        let root = self.bind::<LayerOpacity>(root, LayerCommand::Opacity, cx);
        let root = self.bind::<RaiseLayer>(root, LayerCommand::Raise, cx);
        self.bind::<LowerLayer>(root, LayerCommand::Lower, cx)
    }

    // Registers a handler only while the command can run, so its menu item disables itself.
    fn bind<A: Action>(&self, root: Div, command: LayerCommand, cx: &Context<Self>) -> Div {
        if !self.layer_command_enabled(command) {
            return root;
        }
        root.on_action(
            cx.listener(move |this, _: &A, window, cx| this.run_layer_command(command, window, cx)),
        )
    }

    pub(super) fn run_layer_command(
        &mut self,
        command: LayerCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.layer_command_enabled(command) {
            return;
        }
        let Some(tab) = self.active_tab() else {
            return;
        };
        let id = tab.active_layer;
        let content = &tab.document.content;
        let index = content.layer_index(id).unwrap_or_default();
        let visible = content.layer(id).is_some_and(|layer| layer.visible);
        match command {
            LayerCommand::New => self.new_layer_form(window, cx),
            LayerCommand::Rename => self.rename_layer_form(id, window, cx),
            LayerCommand::Opacity => self.layer_opacity_form(id, window, cx),
            LayerCommand::Duplicate => self.edit_content(
                move |content, active, _| {
                    if let Some(copy) = content.duplicate_layer(id) {
                        *active = copy;
                        Dirty::Canvas
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            ),
            LayerCommand::Delete => self.edit_content(
                move |content, active, _| {
                    if let Some(next) = content.delete_layer(id) {
                        *active = next;
                        Dirty::Canvas
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            ),
            LayerCommand::MergeDown => self.edit_content(
                move |content, active, _| {
                    if let Some(merged) = content.merge_down(id) {
                        *active = merged;
                        Dirty::Canvas
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            ),
            LayerCommand::ToggleVisibility => self.edit_content(
                move |content, _, _| {
                    if content.set_visible(id, !visible) {
                        content.layer_dirty(id)
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            ),
            LayerCommand::Raise => self.move_layer(id, index + 1, window, cx),
            LayerCommand::Lower => {
                self.move_layer(id, index.saturating_sub(1), window, cx);
            }
        }
    }

    fn move_layer(
        &mut self,
        id: LayerId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_content(
            move |content, _, _| {
                if content.move_layer(id, index) {
                    Dirty::Canvas
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    fn select_layer(&mut self, id: LayerId, cx: &mut Context<Self>) {
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        if !tab.busy() && tab.active_layer != id && tab.document.content.layer(id).is_some() {
            tab.active_layer = id;
            tab.notice = None;
            self.opacity_drag = None;
            cx.notify();
        }
    }

    fn toggle_visibility(&mut self, id: LayerId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(visible) = self
            .active_document()
            .and_then(|document| document.content.layer(id))
            .map(|layer| layer.visible)
        else {
            return;
        };
        self.edit_content(
            move |content, _, _| {
                if content.set_visible(id, !visible) {
                    content.layer_dirty(id)
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    fn drag_opacity(
        &mut self,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        start: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.busy() || (!start && self.opacity_drag.is_none()) {
            return;
        }
        let fraction = ((position.x - bounds.left()) / bounds.size.width).clamp(0., 1.);
        self.opacity_drag = Some((tab.active_layer, (fraction * 255.).round() as u8));
        cx.notify();
    }

    // A whole drag commits as one opacity change.
    fn end_opacity_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, opacity)) = self.opacity_drag.take() {
            self.edit_content(
                move |content, _, _| {
                    if content.set_opacity(id, opacity) {
                        content.layer_dirty(id)
                    } else {
                        Dirty::None
                    }
                },
                window,
                cx,
            );
            cx.notify();
        }
    }

    pub(super) fn layers_panel(&self, cx: &Context<Self>) -> Div {
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_label("LAYERS"));
        let panel = div().flex().flex_col().gap_3().p_4();
        let Some(tab) = self.active_tab() else {
            return panel.child(header).child(
                div()
                    .rounded_md()
                    .p_3()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(MUTED))
                    .child("Open or create an image to see its layers."),
            );
        };
        let content = &tab.document.content;
        let layers = content.layers();
        let rows = layers.iter().enumerate().rev().map(|(index, layer)| {
            let id = layer.id;
            let active = id == tab.active_layer;
            let name: SharedString = layer.name.clone().into();
            let thumbnail = tab.document.canvas.thumbnail(id).cloned();
            div()
                .id(("layer", id.0 as usize))
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(rgb(if active { ACCENT } else { BORDER }))
                .bg(rgb(if active { SELECTED } else { CHROME }))
                .cursor_pointer()
                .hover(|row| row.bg(rgb(HOVER)))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.select_layer(id, cx);
                    if event.click_count() == 2 {
                        this.run_layer_command(LayerCommand::Rename, window, cx);
                    }
                }))
                .on_drag(
                    DraggedLayer {
                        id,
                        name: name.clone(),
                    },
                    |dragged, _, _, cx| cx.new(|_| dragged.clone()),
                )
                .drag_over::<DraggedLayer>(|style, _, _, _| style.border_color(rgb(ACCENT)))
                .on_drop(
                    cx.listener(move |this, dragged: &DraggedLayer, window, cx| {
                        this.move_layer(dragged.id, index, window, cx);
                    }),
                )
                .child(
                    div()
                        .size(px(34.))
                        .flex_shrink_0()
                        .rounded_sm()
                        .bg(rgb(CANVAS))
                        .when_some(thumbnail, |frame, image| {
                            frame.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(rgb(if layer.visible { TEXT } else { MUTED }))
                        .child(name),
                )
                .child(
                    div()
                        .id(("layer-visibility", id.0 as usize))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(24.))
                        .flex_shrink_0()
                        .rounded_sm()
                        .text_color(rgb(if layer.visible { MUTED } else { DISABLED }))
                        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .tooltip(tooltip(if layer.visible {
                            "Hide layer"
                        } else {
                            "Show layer"
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_visibility(id, window, cx);
                        }))
                        .child(icon(if layer.visible { "eye" } else { "eye-off" }, 14.)),
                )
        });

        let active = content.layer(tab.active_layer);
        let opacity = match self.opacity_drag {
            Some((id, opacity)) if id == tab.active_layer => opacity,
            _ => active.map_or(255, |layer| layer.opacity),
        };
        let buttons = [
            ("plus", "New Layer…", LayerCommand::New),
            ("duplicate", "Duplicate Layer", LayerCommand::Duplicate),
            ("arrow-up", "Raise Layer", LayerCommand::Raise),
            ("arrow-down", "Lower Layer", LayerCommand::Lower),
            ("merge", "Merge Down", LayerCommand::MergeDown),
            ("trash", "Delete Layer", LayerCommand::Delete),
        ]
        .map(|(symbol, label, command)| {
            let enabled = self.layer_command_enabled(command);
            div()
                .id(label)
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h(px(28.))
                .rounded_md()
                .bg(rgb(CHROME))
                .text_color(rgb(if enabled { MUTED } else { DISABLED }))
                .tooltip(tooltip(label))
                .when(enabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_layer_command(command, window, cx)
                        }))
                })
                .child(icon(symbol, 15.))
        });

        panel
            .child(
                header.child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(layers.len().to_string()),
                ),
            )
            .child(div().flex().flex_col().gap_1().children(rows))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_color(rgb(MUTED))
                            .child("Opacity")
                            .child(format!("{}%", opacity_percent(opacity))),
                    )
                    .child(self.opacity_slider(opacity, cx)),
            )
            .child(div().flex().gap_1().children(buttons))
    }

    fn opacity_slider(&self, opacity: u8, cx: &Context<Self>) -> Stateful<Div> {
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let (layout, down, moved) = (bounds.clone(), bounds.clone(), bounds);
        div()
            .id("layer-opacity")
            .h(px(20.))
            .w_full()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.drag_opacity(down.get(), event.position, true, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.dragging() {
                    this.drag_opacity(moved.get(), event.position, false, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_opacity_drag(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.end_opacity_drag(window, cx)),
            )
            .child(
                canvas(
                    move |bounds, _, _| layout.set(bounds),
                    move |bounds, _, window, _| paint_slider(bounds, opacity, window),
                )
                .size_full(),
            )
    }

    pub(super) fn bind_geometry_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let root = self.bind_align(root, AlignLayer::Horizontal, cx);
        let root = self.bind_align(root, AlignLayer::Vertical, cx);
        let root = self.bind_align(root, AlignLayer::Both, cx);
        if !self.geometry_enabled() {
            return root;
        }
        root.on_action(cx.listener(|this, _: &LayerToImageSize, window, cx| {
            this.layer_to_image_size(window, cx);
        }))
        .on_action(cx.listener(|this, _: &CropLayerToContent, window, cx| {
            this.crop_layer_to_content(window, cx);
        }))
        .on_action(cx.listener(|this, _: &CropImageToContent, window, cx| {
            this.crop_image_to_content(window, cx);
        }))
    }

    fn bind_align(&self, root: Div, axis: AlignLayer, cx: &Context<Self>) -> Div {
        if !self.geometry_enabled() {
            return root;
        }
        root.on_action(cx.listener(move |this, action: &AlignLayer, window, cx| {
            if *action == axis {
                this.align_active(*action, window, cx);
            }
        }))
    }

    pub(super) fn geometry_enabled(&self) -> bool {
        self.move_drag.is_none()
            && self
                .active_tab()
                .is_some_and(|tab| !tab.busy() && self.form.is_none() && !self.dialog_open)
    }

    fn align_active(&mut self, axis: AlignLayer, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        let axis = match axis {
            AlignLayer::Horizontal => Axis::Horizontal,
            AlignLayer::Vertical => Axis::Vertical,
            AlignLayer::Both => Axis::Both,
        };
        self.edit_content(
            move |content, _, _| content.align_layer(id, axis),
            window,
            cx,
        );
    }

    fn layer_to_image_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        self.edit_content(
            move |content, _, _| content.layer_to_image_size(id),
            window,
            cx,
        );
    }

    fn crop_layer_to_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let id = tab.active_layer;
        let Some(layer) = tab.document.content.layer(id) else {
            return;
        };
        if geometry::layer_content_crop(&layer.pixels) == ContentCrop::Empty {
            self.notice("The layer has no pixels to crop.", cx);
            return;
        }
        self.edit_content(
            move |content, _, _| match content.crop_layer_to_content(id) {
                LayerGeometry::Done(dirty) => dirty,
                LayerGeometry::Empty | LayerGeometry::Unchanged => Dirty::None,
            },
            window,
            cx,
        );
    }

    fn crop_image_to_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let rect = match tab.document.content.image_content_rect() {
            ContentCrop::Empty => {
                self.notice("The image has no pixels to crop.", cx);
                return;
            }
            ContentCrop::Unchanged => return,
            ContentCrop::Crop(rect) => rect,
        };
        let crop = document::CropRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        };
        self.edit_content(
            move |content, _, selection| match content.crop_image_to_content() {
                LayerGeometry::Done(dirty) => {
                    *selection = selection.rebase(
                        i64::from(crop.x),
                        i64::from(crop.y),
                        crop.width,
                        crop.height,
                    );
                    dirty
                }
                LayerGeometry::Empty | LayerGeometry::Unchanged => Dirty::None,
            },
            window,
            cx,
        );
    }

    pub(super) fn begin_move(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(anchor) = self.pointer_pixel(position, scale) else {
            return;
        };
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        let Some(layer) = tab.document.content.layer(tab.active_layer) else {
            return;
        };
        if !layer.visible {
            tab.notice = Some("The active layer is hidden. Show it before moving.".into());
            cx.notify();
            return;
        }
        let id = layer.id;
        let origin = layer.offset;
        let size = layer.pixels.dimensions();
        let opacity = layer.opacity;
        let pixels = layer.pixels.clone();
        let content = tab.document.content.clone();
        self.move_token = self.move_token.wrapping_add(1);
        let token = self.move_token;
        self.drop_move_overlay(window, cx);
        self.move_drag = Some(MoveDrag {
            layer: id,
            origin,
            offset: origin,
            anchor,
            anchor_screen: position,
        });
        let job = cx.background_executor().spawn(async move {
            let base = document::preview_render(&content.composite_without(id));
            let sprite = document::preview_render(&with_layer_opacity(&pixels, opacity));
            (base, sprite)
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let (base, sprite) = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.move_token != token {
                    cx.drop_image(base, Some(window));
                    cx.drop_image(sprite, Some(window));
                    return;
                }
                let offset = workspace
                    .move_drag
                    .as_ref()
                    .map(|drag| drag.offset)
                    .or(workspace.move_settled)
                    .unwrap_or(origin);
                workspace.move_overlay = Some(MoveOverlay {
                    base,
                    sprite,
                    offset,
                    size,
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn extend_move(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.move_drag.as_ref() else {
            return;
        };
        let delta = position - drag.anchor_screen;
        let offset = if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 3. {
            drag.origin
        } else {
            let Some(pointer) = self.pointer_pixel(position, scale) else {
                return;
            };
            (
                shift_offset(drag.origin.0, pointer.0 - drag.anchor.0),
                shift_offset(drag.origin.1, pointer.1 - drag.anchor.1),
            )
        };
        if self
            .move_drag
            .as_ref()
            .is_some_and(|drag| drag.offset == offset)
        {
            return;
        }
        if let Some(drag) = self.move_drag.as_mut() {
            drag.offset = offset;
        }
        if let Some(overlay) = self.move_overlay.as_mut() {
            overlay.offset = offset;
        }
        cx.notify();
    }

    pub(super) fn finish_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_some_and(|tab| tab.busy()) {
            self.cancel_move(window, cx);
            return;
        }
        let Some(drag) = self.move_drag.take() else {
            return;
        };
        if drag.offset == drag.origin {
            self.clear_move_preview(window, cx);
            cx.notify();
            return;
        }
        self.move_settled = Some(drag.offset);
        if let Some(overlay) = self.move_overlay.as_mut() {
            overlay.offset = drag.offset;
        }
        let id = drag.layer;
        let offset = drag.offset;
        self.edit_content(
            move |content, _, _| content.set_offset(id, offset),
            window,
            cx,
        );
    }

    pub(super) fn cancel_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.move_drag.is_none() && self.move_overlay.is_none() {
            return;
        }
        self.clear_move_preview(window, cx);
        cx.notify();
    }

    pub(super) fn clear_move_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_token = self.move_token.wrapping_add(1);
        self.move_drag = None;
        self.drop_move_overlay(window, cx);
    }

    pub(super) fn drop_move_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_settled = None;
        if let Some(overlay) = self.move_overlay.take() {
            cx.drop_image(overlay.base, Some(window));
            cx.drop_image(overlay.sprite, Some(window));
        }
    }

    pub(super) fn move_paint(&self) -> Option<MovePaint> {
        let overlay = self.move_overlay.as_ref()?;
        let offset = self
            .move_drag
            .as_ref()
            .map(|drag| drag.offset)
            .unwrap_or(overlay.offset);
        Some(MovePaint {
            base: overlay.base.clone(),
            sprite: overlay.sprite.clone(),
            offset,
            size: overlay.size,
        })
    }

    pub(super) fn notice(&mut self, message: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.active {
            self.tabs[index].notice = Some(message.into());
            cx.notify();
        }
    }
}

fn shift_offset(origin: i32, delta: i64) -> i32 {
    i64::from(origin)
        .saturating_add(delta)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn with_layer_opacity(pixels: &RgbaImage, opacity: u8) -> RgbaImage {
    if opacity == 255 {
        return pixels.clone();
    }
    let mut image = pixels.clone();
    for pixel in image.pixels_mut() {
        pixel.0[3] = ((u16::from(pixel.0[3]) * u16::from(opacity)) / 255) as u8;
    }
    image
}

fn tooltip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tooltip(text.into())).into()
}

fn paint_slider(bounds: Bounds<Pixels>, opacity: u8, window: &mut Window) {
    let track = Bounds::centered_at(bounds.center(), size(bounds.size.width, px(6.)));
    window.paint_quad(quad(
        track,
        px(3.),
        rgb(CANVAS),
        px(1.),
        rgb(BORDER),
        BorderStyle::Solid,
    ));
    let fraction = f32::from(opacity) / 255.;
    let filled = Bounds::new(
        track.origin,
        size(track.size.width * fraction, track.size.height),
    );
    window.paint_quad(quad(
        filled,
        px(3.),
        rgb(ACCENT),
        px(0.),
        rgb(0),
        BorderStyle::Solid,
    ));
    let knob = point(
        bounds.left() + bounds.size.width * fraction,
        bounds.center().y,
    );
    window.paint_quad(quad(
        Bounds::centered_at(knob, size(px(12.), px(12.))),
        px(6.),
        rgb(TEXT),
        px(1.),
        rgb(CANVAS),
        BorderStyle::Solid,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_percent_round_trips_every_percent() {
        for percent in 0..=100 {
            assert_eq!(opacity_percent(opacity_from_percent(percent)), percent);
        }
        assert_eq!(opacity_from_percent(100), 255);
        assert_eq!(opacity_from_percent(0), 0);
    }
}
