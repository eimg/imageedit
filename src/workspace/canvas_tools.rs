use gpui::{Bounds, Context, MouseButton, MouseUpEvent, Pixels, Point, Window, point, px, size};

use super::{CropDrag, Tab, Workspace};
use crate::commands::Zoom;
use crate::document::{CropRect, solid_color};
use crate::preview::Dirty;

impl Workspace {
    pub(super) fn step_zoom(
        &mut self,
        multiplier: f32,
        anchor: Option<Point<Pixels>>,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = self.viewport.get();
        let Some(index) = self.active else {
            return;
        };
        let tab = &mut self.tabs[index];
        let Some(before) = PreviewGeometry::for_tab(tab, viewport, scale) else {
            return;
        };
        let current = before.percent;
        // Let small images zoom past Fit, where their magnification can already exceed 3200%.
        let fit = PreviewGeometry::new(
            tab.document.width(),
            tab.document.height(),
            Zoom::Fit,
            viewport,
            scale,
        );
        let limit = fit.map_or(3200., |fit| (fit.percent * 4.).max(3200.));
        let percent = (current * multiplier).clamp(current.min(0.0001), current.max(limit));
        if (percent - current).abs() <= current * f32::EPSILON {
            return;
        }
        let zoom = Zoom::Percent(percent);

        let anchor = anchor.unwrap_or(viewport.center());
        let coordinates = before.fraction(anchor);
        tab.zoom = zoom;
        if let Some(after) = PreviewGeometry::new(
            tab.document.width(),
            tab.document.height(),
            zoom,
            viewport,
            scale,
        ) {
            let point = after.project(coordinates);
            tab.pan = constrain_pan(anchor - point, after.pan_limit);
        }
        self.pan_drag = None;
        self.crop_drag = None;
        self.ensure_preview_level(scale, window, cx);
        cx.notify();
    }

    pub(super) fn drag_preview(
        &mut self,
        button: MouseButton,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some((drag_button, last)) = self.pan_drag else {
            return;
        };
        if button != drag_button {
            return;
        }
        self.pan_drag = Some((button, position));
        self.pan_preview(position - last, scale, cx);
    }

    pub(super) fn end_canvas_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .pan_drag
            .is_some_and(|(button, _)| button == event.button)
        {
            self.drag_preview(event.button, event.position, window.scale_factor(), cx);
            self.pan_drag = None;
            cx.notify();
        }
        if event.button == MouseButton::Left && self.crop_drag.is_some() {
            self.extend_crop(event.position, window.scale_factor(), cx);
            self.crop_drag = None;
            cx.notify();
        }
        if event.button == MouseButton::Left && self.selection_drag.is_some() {
            self.extend_selection(event.position, window.scale_factor(), cx);
            self.finish_selection(window, cx);
        }
    }

    pub(super) fn pan_preview(&mut self, delta: Point<Pixels>, scale: f32, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        let tab = &mut self.tabs[index];
        if let Some(geometry) = PreviewGeometry::for_tab(tab, self.viewport.get(), scale) {
            let limit = geometry.pan_limit;
            let pan = constrain_pan(constrain_pan(tab.pan, limit) + delta, limit);
            if pan != tab.pan {
                tab.pan = pan;
                cx.notify();
            }
        }
    }

    pub(super) fn begin_crop(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(Tab::busy) {
            return;
        }
        self.clear_crop();
        self.crop_drag = self
            .canvas_pixel(position, scale, false)
            .map(|start| CropDrag {
                origin: position,
                start,
            });
        cx.notify();
    }

    pub(super) fn extend_crop(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.crop_drag else {
            return;
        };
        let end = self.canvas_pixel(position, scale, true);
        let delta = position - drag.origin;
        let crop = end
            .filter(|_| f32::from(delta.x).abs().max(f32::from(delta.y).abs()) >= 3.)
            .map(|end| CropRect::between(drag.start, end));
        if self.crop != crop {
            self.crop = crop;
            cx.notify();
        }
    }

    pub(super) fn apply_crop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(crop) = self.crop.take() else {
            return;
        };
        self.crop_drag = None;
        self.edit_content(
            move |content, _, selection| {
                if content.crop(crop) {
                    *selection = selection.rebase(
                        i64::from(crop.x),
                        i64::from(crop.y),
                        crop.width,
                        crop.height,
                    );
                    Dirty::Canvas
                } else {
                    Dirty::None
                }
            },
            window,
            cx,
        );
        cx.notify();
    }

    pub(super) fn fill_at(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.canvas_pixel(position, window.scale_factor(), false) else {
            return;
        };
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        let layer = tab.active_layer;
        let selection = tab.selection;
        let (width, height) = tab.document.content.size();
        if !selection.covers(i64::from(x), i64::from(y), width, height) {
            tab.notice = Some(if selection == crate::selection::Selection::Empty {
                "Nothing is selected.".into()
            } else {
                "That pixel is outside the selection.".into()
            });
            cx.notify();
            return;
        }
        if tab
            .document
            .content
            .layer(layer)
            .is_some_and(|layer| !layer.visible)
        {
            tab.notice = Some("The active layer is hidden. Show it before filling.".into());
            cx.notify();
            return;
        }
        let color = solid_color(self.colors[0]);
        self.edit_content(
            move |content, _, selection| {
                content
                    .fill(layer, x, y, color, selection)
                    .map(Dirty::Region)
                    .unwrap_or(Dirty::None)
            },
            window,
            cx,
        );
    }

    pub(super) fn pick_color(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.canvas_pixel(position, scale, false) else {
            return;
        };
        if let Some(document) = self.active_document() {
            let [r, g, b, alpha] = document.canvas.pixels.get_pixel(x, y).0;
            if alpha > 0 {
                self.set_color(0, u32::from_be_bytes([0, r, g, b]));
                cx.notify();
            }
        }
    }

    pub(super) fn canvas_pixel(
        &self,
        position: Point<Pixels>,
        scale: f32,
        clamp: bool,
    ) -> Option<(u32, u32)> {
        let tab = self.active_tab()?;
        let viewport = self.viewport.get();
        let geometry = PreviewGeometry::for_tab(tab, viewport, scale)?;
        if !clamp && (!viewport.contains(&position) || !geometry.bounds.contains(&position)) {
            return None;
        }
        let position = point(
            position.x.clamp(viewport.left(), viewport.right()),
            position.y.clamp(viewport.top(), viewport.bottom()),
        );
        let fraction = geometry.fraction(position);
        let pixel = |value: f32, edge: u32| ((value * edge as f32) as u32).min(edge - 1);
        Some((
            pixel(fraction.x, tab.document.width()),
            pixel(fraction.y, tab.document.height()),
        ))
    }
}

pub(super) struct PreviewGeometry {
    pub bounds: Bounds<Pixels>,
    pan_limit: Point<Pixels>,
    percent: f32,
}

impl PreviewGeometry {
    pub(super) fn new(
        width: u32,
        height: u32,
        zoom: Zoom,
        viewport: Bounds<Pixels>,
        scale: f32,
    ) -> Option<Self> {
        let width = px(width as f32);
        let height = px(height as f32);
        let factor = match zoom {
            Zoom::Fit => {
                let padded = viewport.inset(px(24.));
                (padded.size.width / width).min(padded.size.height / height)
            }
            Zoom::Percent(percent) => percent / 100. / scale,
        };
        if factor <= 0. || viewport.size.width <= px(0.) || viewport.size.height <= px(0.) {
            return None;
        }
        let bounds = Bounds::centered_at(viewport.center(), size(width * factor, height * factor));
        let pan_limit = point(
            ((bounds.size.width - viewport.size.width) / 2.).max(px(0.)),
            ((bounds.size.height - viewport.size.height) / 2.).max(px(0.)),
        );
        Some(Self {
            bounds,
            pan_limit,
            percent: match zoom {
                Zoom::Fit => factor * scale * 100.,
                Zoom::Percent(percent) => percent,
            },
        })
    }

    pub(super) fn for_tab(tab: &Tab, viewport: Bounds<Pixels>, scale: f32) -> Option<Self> {
        Self::new(
            tab.document.width(),
            tab.document.height(),
            tab.zoom,
            viewport,
            scale,
        )
        .map(|geometry| geometry.panned(tab.pan))
    }

    pub(super) fn panned(mut self, pan: Point<Pixels>) -> Self {
        self.bounds.origin += constrain_pan(pan, self.pan_limit);
        self
    }

    pub(super) fn fraction(&self, position: Point<Pixels>) -> Point<f32> {
        point(
            (position.x - self.bounds.left()) / self.bounds.size.width,
            (position.y - self.bounds.top()) / self.bounds.size.height,
        )
    }

    pub(super) fn project(&self, fraction: Point<f32>) -> Point<Pixels> {
        self.bounds.origin
            + point(
                self.bounds.size.width * fraction.x,
                self.bounds.size.height * fraction.y,
            )
    }
}

fn constrain_pan(pan: Point<Pixels>, limit: Point<Pixels>) -> Point<Pixels> {
    point(
        pan.x.clamp(-limit.x, limit.x),
        pan.y.clamp(-limit.y, limit.y),
    )
}
