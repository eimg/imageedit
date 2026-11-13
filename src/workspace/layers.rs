use std::{cell::Cell, rc::Rc, sync::Arc};

use gpui::{
    Action, BorderStyle, Bounds, ClickEvent, Context, CursorStyle, Div, MouseButton,
    MouseDownEvent, MouseUpEvent, ObjectFit, Pixels, Point, RenderImage, SharedString, Stateful,
    Window, canvas, div, img, point, prelude::*, px, quad, rgb, size,
};
use image::RgbaImage;

use super::Workspace;
use super::tooltip::{label_view, tooltip};
use super::view::{
    ACCENT, BORDER, CANVAS, CHECKER_DARK, CHECKER_LIGHT, CHROME, DISABLED, HOVER, MUTED, SELECTED,
    TEXT, icon, section_label,
};
use crate::commands::{
    AlignLayer, CropImageToContent, CropLayerToContent, DeleteLayer, DuplicateLayer, LayerOpacity,
    LayerToImageSize, LowerLayer, MergeDown, NewLayer, RaiseLayer, RenameLayer,
    ToggleLayerVisibility,
};
use crate::document::{self, LayerGeometry, LayerId};
use crate::geometry::{self, Alignment, ContentCrop};
use crate::preview::Dirty;

pub(super) struct MoveDrag {
    tab: usize,
    layer: LayerId,
    origin: (i32, i32),
    pub(super) offset: (i32, i32),
    anchor: (i64, i64),
    anchor_screen: Point<Pixels>,
    input: MoveInput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveInput {
    Pointer,
    Numeric,
    Keyboard,
}

pub(super) struct PendingNudge {
    tab: usize,
    layer: LayerId,
    generation: u64,
    delta: (i32, i32),
    held: bool,
}

impl PendingNudge {
    fn queue(&mut self, context: (usize, LayerId, u64), delta: (i32, i32)) {
        if (self.tab, self.layer, self.generation) == context {
            self.delta = (
                self.delta.0.saturating_add(delta.0),
                self.delta.1.saturating_add(delta.1),
            );
            self.held = true;
        }
    }

    fn accepts(&self, tab: &super::Tab, published: bool) -> bool {
        published
            && tab.id == self.tab
            && tab.active_layer == self.layer
            && !tab.busy()
            && self.generation.checked_add(1) == Some(tab.history.generation())
    }
}

impl MoveDrag {
    fn belongs_to(&self, tab: &super::Tab) -> bool {
        self.tab == tab.id && self.layer == tab.active_layer && !tab.busy()
    }
    fn nudge_offset(&self, delta: (i32, i32)) -> Option<(i32, i32)> {
        (self.input == MoveInput::Keyboard).then(|| {
            (
                shift_offset(self.offset.0, i64::from(delta.0)),
                shift_offset(self.offset.1, i64::from(delta.1)),
            )
        })
    }
}

pub(super) struct OpacityDrag {
    tab: usize,
    layer: LayerId,
    bounds: Bounds<Pixels>,
    opacity: u8,
}

impl OpacityDrag {
    fn new(
        tab: usize,
        layer: LayerId,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
    ) -> Option<Self> {
        Some(Self {
            tab,
            layer,
            bounds,
            opacity: Self::value_at(bounds, position)?,
        })
    }

    fn value_at(bounds: Bounds<Pixels>, position: Point<Pixels>) -> Option<u8> {
        let width = f32::from(bounds.size.width);
        let offset = f32::from(position.x - bounds.left());
        if !width.is_finite() || width <= 0. || !offset.is_finite() {
            return None;
        }
        Some(((offset / width).clamp(0., 1.) * 255.).round() as u8)
    }

    fn update(&mut self, position: Point<Pixels>) -> bool {
        let Some(opacity) = Self::value_at(self.bounds, position) else {
            return false;
        };
        if self.opacity == opacity {
            return false;
        }
        self.opacity = opacity;
        true
    }

    fn belongs_to(&self, tab: &super::Tab) -> bool {
        self.tab == tab.id && self.layer == tab.active_layer
    }
}

#[derive(Clone)]
pub(super) struct MovePaint {
    pub below: Option<Arc<RenderImage>>,
    pub sprite: Arc<RenderImage>,
    pub above: Option<Arc<RenderImage>>,
    pub offset: (i32, i32),
    pub size: (u32, u32),
}

impl MovePaint {
    pub(super) fn into_images(self) -> impl Iterator<Item = Arc<RenderImage>> {
        self.below
            .into_iter()
            .chain([self.sprite])
            .chain(self.above)
    }
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

const THUMBNAIL_EDGE: f32 = 34.;
const THUMBNAIL_CHECKER: f32 = 4.25;

fn thumbnail_size((width, height): (u32, u32)) -> (f32, f32) {
    let edge = width.max(height).max(1) as f32;
    let scale = THUMBNAIL_EDGE / edge;
    (
        (width as f32 * scale).max(1.),
        (height as f32 * scale).max(1.),
    )
}

fn thumbnail_checker() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            window.paint_quad(quad(
                bounds,
                px(0.),
                rgb(CHECKER_DARK),
                px(0.),
                rgb(0),
                BorderStyle::Solid,
            ));
            let columns = (f32::from(bounds.size.width) / THUMBNAIL_CHECKER).ceil() as i32;
            let rows = (f32::from(bounds.size.height) / THUMBNAIL_CHECKER).ceil() as i32;
            for y in 0..rows {
                for x in 0..columns {
                    if (x + y) % 2 != 0 {
                        continue;
                    }
                    let cell = Bounds::new(
                        bounds.origin
                            + point(
                                px(x as f32 * THUMBNAIL_CHECKER),
                                px(y as f32 * THUMBNAIL_CHECKER),
                            ),
                        size(px(THUMBNAIL_CHECKER), px(THUMBNAIL_CHECKER)),
                    )
                    .intersect(&bounds);
                    if !cell.is_empty() {
                        window.paint_quad(quad(
                            cell,
                            px(0.),
                            rgb(CHECKER_LIGHT),
                            px(0.),
                            rgb(0),
                            BorderStyle::Solid,
                        ));
                    }
                }
            }
        },
    )
    .absolute()
    .size_full()
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
            LayerCommand::New => content.can_create_layer(),
            LayerCommand::Duplicate => content.can_duplicate_layer(tab.active_layer),
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
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| {
                this.run_layer_command(command, window, cx)
            });
            return;
        }
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
            LayerCommand::MergeDown => {
                let blocked = self.active_tab().is_some_and(|tab| {
                    let layers = tab.document.content.layers();
                    let Some(index) = layers.iter().position(|layer| layer.id == id) else {
                        return false;
                    };
                    layers[index].text.is_some()
                        || index
                            .checked_sub(1)
                            .is_some_and(|lower| layers[lower].text.is_some())
                });
                if blocked {
                    self.notice("Rasterize the text layer before using pixel tools.", cx);
                    return;
                }
                self.edit_content(
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
                );
            }
            LayerCommand::ToggleVisibility => self.edit_content(
                move |content, _, _| {
                    if content.set_visible(id, !visible) {
                        content.layer_composite_dirty(id)
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

    fn select_layer(&mut self, id: LayerId, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab().is_some_and(|tab| tab.active_layer != id)
            && self.finish_text(window, cx)
        {
            self.defer_after_text(move |this, window, cx| this.select_layer(id, window, cx));
            return;
        }
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        if !tab.busy() && tab.active_layer != id && tab.document.content.layer(id).is_some() {
            self.pending_nudge = None;
            self.clone_source = None;
            tab.active_layer = id;
            tab.notice = None;
            self.clear_move_preview(window, cx);
            self.opacity_drag = None;
            self.cancel_text(window, cx);
            self.cancel_warp(window, cx);
            self.cancel_stroke(window, cx);
            self.cancel_gradient(window, cx);
            self.cancel_scale(window, cx);
            self.begin_warp(window, cx);
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
                    content.layer_composite_dirty(id)
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
    }

    fn begin_opacity_drag(
        &mut self,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_numeric_edit(window, cx);
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.busy() || self.form.is_some() || self.dialog_open || self.move_drag.is_some() {
            return;
        }
        self.opacity_drag = OpacityDrag::new(tab.id, tab.active_layer, bounds, position);
        cx.notify();
    }

    pub(super) fn extend_opacity_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(drag) = self.opacity_drag.as_mut()
            && drag.update(position)
        {
            cx.notify();
        }
    }

    // A whole drag commits as one opacity change.
    pub(super) fn end_opacity_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(mut drag) = self.opacity_drag.take() else {
            return;
        };
        drag.update(event.position);
        if self.active_tab().is_some_and(|tab| drag.belongs_to(tab)) {
            self.commit_active_opacity(drag.opacity, window, cx);
        }
        cx.notify();
    }

    fn commit_active_opacity(&mut self, opacity: u8, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            if let Some(index) = self.active {
                let tab = &mut self.tabs[index];
                tab.pending_opacity = Some((tab.active_layer, opacity));
            }
            self.defer_after_text(move |this, window, cx| {
                this.commit_active_opacity(opacity, window, cx);
            });
            return;
        }
        // A new text draft can insert a layer. Resolve its ID after that commit.
        if let Some(tab) = self.active_tab() {
            self.commit_layer_opacity(tab.active_layer, opacity, window, cx);
        }
    }

    pub(super) fn commit_layer_opacity(
        &mut self,
        id: LayerId,
        opacity: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.active else {
            return;
        };
        if self.tabs[index].busy()
            || self.move_drag.is_some()
            || self.tabs[index]
                .document
                .content
                .layer(id)
                .is_none_or(|layer| layer.opacity == opacity)
        {
            return;
        }
        self.edit_content(
            move |content, _, _| content.change_opacity(id, opacity),
            window,
            cx,
        );
        self.tabs[index].pending_opacity = Some((id, opacity));
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
            let title: SharedString = if layer.text.is_some() {
                format!("{name} · Text").into()
            } else {
                name.clone()
            };
            let thumbnail = tab.document.canvas.thumbnail(id).cloned();
            let (thumbnail_width, thumbnail_height) = thumbnail_size(layer.pixels.dimensions());
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
                    this.select_layer(id, window, cx);
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
                        .size(px(THUMBNAIL_EDGE))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .relative()
                                .w(px(thumbnail_width))
                                .h(px(thumbnail_height))
                                .rounded_sm()
                                .overflow_hidden()
                                .child(thumbnail_checker())
                                .when_some(thumbnail, |frame, image| {
                                    frame.child(
                                        img(image)
                                            .absolute()
                                            .size_full()
                                            .object_fit(ObjectFit::Fill),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(rgb(if layer.visible { TEXT } else { MUTED }))
                        .child(title),
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

        let opacity = self
            .opacity_drag
            .as_ref()
            .filter(|drag| drag.belongs_to(tab))
            .map_or_else(|| tab.layer_opacity(), |drag| drag.opacity);
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
        let (layout, down) = (bounds.clone(), bounds);
        div()
            .id("layer-opacity")
            .h(px(20.))
            .w_full()
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.begin_opacity_drag(down.get(), event.position, window, cx);
                }),
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
        if !self.geometry_enabled() {
            return root;
        }
        root.on_action(cx.listener(|this, action: &AlignLayer, window, cx| {
            this.align_active(*action, window, cx);
        }))
        .on_action(cx.listener(|this, _: &LayerToImageSize, window, cx| {
            this.layer_to_image_size(window, cx);
        }))
        .on_action(cx.listener(|this, _: &CropLayerToContent, window, cx| {
            this.crop_layer_to_content(window, cx);
        }))
        .on_action(cx.listener(|this, _: &CropImageToContent, window, cx| {
            this.crop_image_to_content(window, cx);
        }))
    }

    pub(super) fn geometry_enabled(&self) -> bool {
        self.move_drag.is_none()
            && self
                .active_tab()
                .is_some_and(|tab| !tab.busy() && self.form.is_none() && !self.dialog_open)
    }

    fn align_active(&mut self, axis: AlignLayer, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(move |this, window, cx| this.align_active(axis, window, cx));
            return;
        }
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        let alignment = match axis {
            AlignLayer::Left => Alignment::Left,
            AlignLayer::Right => Alignment::Right,
            AlignLayer::Top => Alignment::Top,
            AlignLayer::Bottom => Alignment::Bottom,
            AlignLayer::Horizontal => Alignment::HorizontalCenter,
            AlignLayer::Vertical => Alignment::VerticalCenter,
            AlignLayer::Both => Alignment::Both,
        };
        self.edit_content(
            move |content, _, _| content.align_layer(id, alignment),
            window,
            cx,
        );
    }

    fn layer_to_image_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.layer_to_image_size(window, cx));
            return;
        }
        if self.refuse_text_pixels(false, cx) {
            return;
        }
        let Some(id) = self.active_tab().map(|tab| tab.active_layer) else {
            return;
        };
        if self
            .active_document()
            .is_some_and(|document| !document.content.can_fit_layer(id))
        {
            self.notice(
                "Fitting this layer would exceed the project pixel budget.",
                cx,
            );
            return;
        }
        self.edit_content(
            move |content, _, _| content.layer_to_image_size(id),
            window,
            cx,
        );
    }

    fn crop_layer_to_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.crop_layer_to_content(window, cx));
            return;
        }
        if self.refuse_text_pixels(false, cx) {
            return;
        }
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
        if self.finish_text(window, cx) {
            self.defer_after_text(|this, window, cx| this.crop_image_to_content(window, cx));
            return;
        }
        if self.refuse_text_pixels(true, cx) {
            return;
        }
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
                    *selection = selection.clone().rebase(
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
        if self.move_drag.is_some() || self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        let Some(anchor) = self.unclamped_pointer_pixel(position, scale) else {
            return;
        };
        self.start_move_preview(MoveInput::Pointer, anchor, position, window, cx);
    }

    pub(super) fn begin_move_numeric(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tool != crate::commands::Tool::Move || self.move_drag.is_some() {
            return false;
        }
        self.start_move_preview(
            MoveInput::Numeric,
            (0, 0),
            point(px(0.), px(0.)),
            window,
            cx,
        );
        self.move_drag.is_some()
    }

    pub(super) fn move_offset(&self) -> Option<(i32, i32)> {
        self.move_drag
            .as_ref()
            .map(|drag| drag.offset)
            .or(self.move_settled)
            .or_else(|| {
                let tab = self.active_tab()?;
                Some(tab.document.content.layer(tab.active_layer)?.offset)
            })
    }

    pub(super) fn preview_move_position(
        &mut self,
        horizontal: bool,
        value: i32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self
            .move_drag
            .as_ref()
            .filter(|drag| drag.input == MoveInput::Numeric)
        else {
            return;
        };
        let offset = if horizontal {
            (value, drag.origin.1)
        } else {
            (drag.origin.0, value)
        };
        self.update_move_offset(offset, cx);
    }

    pub(super) fn restore_move_position(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.move_drag.as_ref() {
            self.update_move_offset(drag.origin, cx);
        }
    }

    pub(super) fn nudge_layer(
        &mut self,
        delta: (i32, i32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tool != crate::commands::Tool::Move
            || self.space_pan
            || self.numeric_edit.is_some()
            || self.form.is_some()
            || self.dialog_open
        {
            return;
        }
        if let Some(tab) = self.active_tab().filter(|tab| tab.busy()) {
            let context = (tab.id, tab.active_layer, tab.history.generation());
            if let Some(pending) = self.pending_nudge.as_mut() {
                pending.queue(context, delta);
            }
            return;
        }
        if self.move_drag.is_none() {
            self.start_move_preview(
                MoveInput::Keyboard,
                (0, 0),
                point(px(0.), px(0.)),
                window,
                cx,
            );
        }
        let Some(offset) = self
            .move_drag
            .as_ref()
            .and_then(|drag| drag.nudge_offset(delta))
        else {
            return;
        };
        self.update_move_offset(offset, cx);
    }

    pub(super) fn finish_nudge(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(key, "left" | "right" | "up" | "down")
            && (self.form.is_some() || self.dialog_open)
            && (self.pending_nudge.is_some()
                || self
                    .move_drag
                    .as_ref()
                    .is_some_and(|drag| drag.input == MoveInput::Keyboard))
        {
            self.cancel_move(window, cx);
            return;
        }
        if matches!(key, "left" | "right" | "up" | "down")
            && let Some(pending) = self.pending_nudge.as_mut()
        {
            pending.held = false;
        }
        if matches!(key, "left" | "right" | "up" | "down")
            && self
                .move_drag
                .as_ref()
                .is_some_and(|drag| drag.input == MoveInput::Keyboard)
        {
            self.finish_move(window, cx);
        }
    }

    pub(super) fn move_pointer_dragging(&self) -> bool {
        self.move_drag
            .as_ref()
            .is_some_and(|drag| drag.input == MoveInput::Pointer)
    }

    pub(super) fn finish_pending_nudge(
        &mut self,
        id: usize,
        generation: u64,
        published: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_nudge.take() else {
            return;
        };
        if pending.tab != id || pending.generation != generation {
            self.pending_nudge = Some(pending);
            return;
        }
        if self.tool != crate::commands::Tool::Move
            || self.form.is_some()
            || self.dialog_open
            || pending.delta == (0, 0)
            || self
                .active_tab()
                .is_none_or(|tab| !pending.accepts(tab, published))
        {
            return;
        }
        self.nudge_layer(pending.delta, window, cx);
        if !pending.held {
            self.finish_move(window, cx);
        }
    }

    fn start_move_preview(
        &mut self,
        input: MoveInput,
        anchor: (i64, i64),
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .active
            .map(|index| &mut self.tabs[index])
            .filter(|tab| !tab.busy())
        else {
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
        let tab_id = tab.id;
        let size = layer.pixels.dimensions();
        let opacity = layer.opacity;
        let pixels = layer.pixels.clone();
        let content = tab.document.content.clone();
        self.move_token = self.move_token.wrapping_add(1);
        let token = self.move_token;
        self.drop_move_overlay(window, cx);
        self.move_drag = Some(MoveDrag {
            tab: tab_id,
            layer: id,
            origin,
            offset: origin,
            anchor,
            anchor_screen: position,
            input,
        });
        let job = cx.background_executor().spawn(async move {
            let (below, above) = content.composite_around(id);
            MovePaint {
                below: below.map(|image| document::preview_render(&image)),
                sprite: document::preview_render(&with_layer_opacity(&pixels, opacity)),
                above: above.map(|image| document::preview_render(&image)),
                offset: origin,
                size,
            }
        });
        cx.spawn_in(window, async move |workspace, cx| {
            let mut overlay = job.await;
            let _ = workspace.update_in(cx, |workspace, window, cx| {
                if workspace.move_token != token {
                    super::drop_images(overlay.into_images(), window, cx);
                    return;
                }
                overlay.offset = workspace
                    .move_drag
                    .as_ref()
                    .map(|drag| drag.offset)
                    .or(workspace.move_settled)
                    .unwrap_or(origin);
                workspace.move_overlay = Some(overlay);
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
        let Some(drag) = self
            .move_drag
            .as_ref()
            .filter(|drag| drag.input == MoveInput::Pointer)
        else {
            return;
        };
        let delta = position - drag.anchor_screen;
        let offset = if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 3. {
            drag.origin
        } else {
            let Some(pointer) = self.unclamped_pointer_pixel(position, scale) else {
                return;
            };
            (
                shift_offset(drag.origin.0, pointer.0.saturating_sub(drag.anchor.0)),
                shift_offset(drag.origin.1, pointer.1.saturating_sub(drag.anchor.1)),
            )
        };
        self.update_move_offset(offset, cx);
    }

    fn update_move_offset(&mut self, offset: (i32, i32), cx: &mut Context<Self>) {
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
        if self.active_tab().is_none_or(|tab| !drag.belongs_to(tab)) || drag.offset == drag.origin {
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
        let pending = if drag.input == MoveInput::Keyboard {
            self.active_tab().map(|tab| PendingNudge {
                tab: tab.id,
                layer: id,
                generation: tab.history.generation(),
                delta: (0, 0),
                held: false,
            })
        } else {
            None
        };
        self.edit_content(
            move |content, _, _| content.set_offset(id, offset),
            window,
            cx,
        );
        self.pending_nudge = pending;
    }

    pub(super) fn cancel_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_nudge = None;
        if self.move_drag.is_none() && self.move_overlay.is_none() {
            return;
        }
        self.clear_move_preview(window, cx);
        cx.notify();
    }

    pub(super) fn clear_move_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .numeric_edit
            .as_ref()
            .is_some_and(|edit| edit.field.is_move())
        {
            self.numeric_edit = None;
        }
        self.move_token = self.move_token.wrapping_add(1);
        self.move_drag = None;
        self.drop_move_overlay(window, cx);
    }

    pub(super) fn drop_move_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_settled = None;
        if let Some(overlay) = self.move_overlay.take() {
            super::drop_images(overlay.into_images(), window, cx);
        }
    }

    pub(super) fn move_paint(&self) -> Option<MovePaint> {
        self.move_overlay
            .clone()
            .or_else(|| self.warp_overlay.clone())
            .or_else(|| self.gradient_overlay.clone())
            .or_else(|| self.text_overlay.clone())
            .or_else(|| self.scale_overlay.clone())
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

pub(super) fn with_layer_opacity(pixels: &RgbaImage, opacity: u8) -> RgbaImage {
    if opacity == 255 {
        return pixels.clone();
    }
    let mut image = pixels.clone();
    for pixel in image.pixels_mut() {
        pixel.0[3] = ((u16::from(pixel.0[3]) * u16::from(opacity) + 127) / 255) as u8;
    }
    image
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
    fn keyboard_nudges_accumulate_without_changing_a_pointer_or_numeric_session() {
        let mut drag = MoveDrag {
            tab: 0,
            layer: LayerId(0),
            origin: (0, 0),
            offset: (0, 0),
            anchor: (0, 0),
            anchor_screen: point(px(0.), px(0.)),
            input: MoveInput::Keyboard,
        };
        for _ in 0..5 {
            drag.offset = drag.nudge_offset((1, -10)).unwrap();
        }
        assert_eq!(drag.offset, (5, -50));
        assert_eq!(drag.origin, (0, 0));
        drag.offset = (i32::MAX, i32::MIN);
        assert_eq!(drag.nudge_offset((10, -10)), Some(drag.offset));
        for input in [MoveInput::Pointer, MoveInput::Numeric] {
            drag.input = input;
            assert_eq!(drag.nudge_offset((1, 0)), None);
        }
        let mut tab = super::super::tests::opacity_tab();
        drag.tab = tab.id;
        drag.layer = tab.active_layer;
        assert!(drag.belongs_to(&tab));
        tab.active_layer = LayerId(99);
        assert!(!drag.belongs_to(&tab));
        tab.active_layer = drag.layer;
        tab.id += 1;
        assert!(!drag.belongs_to(&tab));
        tab.id = drag.tab;
        tab.editing = true;
        assert!(!drag.belongs_to(&tab));
    }

    #[test]
    fn queued_nudges_resume_only_after_their_own_move_commit() {
        let mut tab = super::super::tests::opacity_tab();
        let generation = tab.history.generation();
        let context = (tab.id, tab.active_layer, generation);
        let mut pending = PendingNudge {
            tab: tab.id,
            layer: tab.active_layer,
            generation,
            delta: (0, 0),
            held: false,
        };
        pending.queue(context, (1, 0));
        pending.queue(context, (0, -10));
        pending.queue((tab.id + 1, tab.active_layer, generation), (100, 0));
        assert_eq!(pending.delta, (1, -10));
        assert!(pending.held);
        assert!(!pending.accepts(&tab, true));
        let mut state = tab.state();
        let dirty = state.content.set_offset(state.active_layer, (1, 0));
        let patch = crate::document::RenderedCanvas::prepare_patch(
            &tab.document.canvas.pixels,
            tab.document.canvas.preview.has_transparency,
            tab.document.canvas.level_caches(),
            &state.content,
            dirty,
        )
        .unwrap();
        tab.editing = true;
        assert!(!pending.accepts(&tab, true));
        assert!(
            tab.publish_change(
                generation,
                super::super::Change::Edit,
                Some((state.content, state.active_layer, state.selection, patch)),
                1
            )
            .0
        );
        assert!(pending.accepts(&tab, true));
        assert!(!pending.accepts(&tab, false));
        assert_eq!(tab.history.generation(), generation + 1);
        let (undo, _) = tab.history.undo_target().unwrap();
        assert_eq!(undo.content.layer(tab.active_layer).unwrap().offset, (0, 0));
        tab.active_layer = LayerId(99);
        assert!(!pending.accepts(&tab, true));
        tab.active_layer = context.1;
        pending.generation += 1;
        assert!(!pending.accepts(&tab, true));
    }

    #[test]
    fn opacity_drag_clamps_outside_the_track_and_samples_the_release_position() {
        let bounds = Bounds::new(point(px(20.), px(10.)), size(px(200.), px(20.)));
        let at = |x| point(px(x), px(900.));
        let mut drag = OpacityDrag::new(1, LayerId(0), bounds, at(120.)).unwrap();
        assert_eq!(drag.opacity, 128);
        assert!(!drag.update(at(120.)));
        assert!(drag.update(at(-100.)));
        assert_eq!(drag.opacity, 0);
        assert!(drag.update(at(500.)));
        assert_eq!(drag.opacity, 255);
        // Mouse-up may be the first event at the final x coordinate.
        assert!(drag.update(at(70.)));
        assert_eq!(drag.opacity, 64);
    }

    #[test]
    fn opacity_drag_rejects_invalid_layout_and_belongs_only_to_its_tab_and_layer() {
        let mut tab = super::super::tests::opacity_tab();
        let position = point(px(30.), px(10.));
        assert!(OpacityDrag::new(tab.id, tab.active_layer, Bounds::default(), position).is_none());
        let bounds = Bounds::new(Point::default(), size(px(100.), px(20.)));
        let drag = OpacityDrag::new(tab.id, tab.active_layer, bounds, position).unwrap();
        assert!(drag.belongs_to(&tab));
        tab.id += 1;
        assert!(!drag.belongs_to(&tab));
        tab.id -= 1;
        tab.active_layer = LayerId(1);
        assert!(!drag.belongs_to(&tab));
        let invalid = Bounds::new(Point::default(), size(px(f32::NAN), px(20.)));
        assert!(OpacityDrag::new(tab.id, tab.active_layer, invalid, position).is_none());
    }

    #[test]
    fn thumbnail_sizes_keep_the_layer_aspect_ratio() {
        assert_eq!(thumbnail_size((4, 4)), (34., 34.));
        assert_eq!(thumbnail_size((8, 4)), (34., 17.));
        assert_eq!(thumbnail_size((4, 8)), (17., 34.));
        assert_eq!(thumbnail_size((0, 0)), (1., 1.));
    }

    #[test]
    fn opacity_percent_round_trips_every_percent() {
        for percent in 0..=100 {
            assert_eq!(opacity_percent(opacity_from_percent(percent)), percent);
        }
        assert_eq!(opacity_from_percent(100), 255);
        assert_eq!(opacity_from_percent(0), 0);
    }
}
