use gpui::{
    Bounds, Context, CursorStyle, KeyDownEvent, KeyUpEvent, MouseButton, MouseUpEvent, Pixels,
    Point, Window, point, px, size,
};

use super::{CropDrag, CropEdges, Tab, Workspace};
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
        if event.button == MouseButton::Left && self.scale_drag.is_some() {
            self.finish_scale(window, cx);
        }
        if event.button == MouseButton::Left && !self.free_points.is_empty() {
            self.finish_free(event.position, window, cx);
        }
        if event.button == MouseButton::Left && self.move_drag.is_some() {
            self.extend_move(event.position, window.scale_factor(), cx);
            self.finish_move(window, cx);
        }
        if event.button == MouseButton::Left
            && self.warp.as_ref().is_some_and(|session| session.dragging())
        {
            self.extend_warp_drag(event.position, window.scale_factor(), window, cx);
            self.end_warp_drag(cx);
        }
        if event.button == MouseButton::Left
            && self
                .gradient
                .as_ref()
                .is_some_and(|session| session.dragging())
        {
            self.extend_gradient(event.position, window.scale_factor(), window, cx);
            self.end_gradient_drag(cx);
        }
        if event.button == MouseButton::Left && self.stroke.is_some() {
            self.finish_stroke(event.position, window, cx);
        }
        if event.button == MouseButton::Left && self.path_gesture.is_some() {
            self.finish_path(event.position, window.scale_factor(), window, cx);
        }
    }

    pub(super) fn begin_space_pan(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if event.is_held
            || key.key != "space"
            || key.modifiers.modified()
            || self.space_pan
            || self.form.is_some()
            || self.dialog_open
            || self.text_draft.is_some()
            || self.active_tab().is_none()
        {
            return;
        }
        self.space_pan = true;
        cx.notify();
    }

    pub(super) fn end_space_pan(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if event.keystroke.key != "space" || !self.space_pan {
            return;
        }
        self.space_pan = false;
        if self
            .pan_drag
            .is_some_and(|(button, _)| button == MouseButton::Left)
        {
            self.pan_drag = None;
        }
        cx.notify();
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
        let hit = self.crop.and_then(|crop| {
            self.crop_screen_rect(crop, scale)
                .map(|(left, top, right, bottom)| {
                    (
                        crop,
                        hit_crop(
                            left,
                            top,
                            right,
                            bottom,
                            f32::from(position.x),
                            f32::from(position.y),
                            6.,
                        ),
                    )
                })
        });
        self.crop_drag = match hit {
            Some((crop, CropHit::Edge(edges))) => self
                .unclamped_pointer_pixel(position, scale)
                .map(|anchor| CropDrag::Resize {
                    crop,
                    edges,
                    anchor,
                }),
            Some((crop, CropHit::Inside)) => {
                self.canvas_pixel(position, scale, true)
                    .map(|pixel| CropDrag::Move {
                        crop,
                        grab: (
                            i64::from(pixel.0) - i64::from(crop.x),
                            i64::from(pixel.1) - i64::from(crop.y),
                        ),
                    })
            }
            Some((_, CropHit::Outside)) | None => {
                self.clear_crop();
                self.canvas_pixel(position, scale, false)
                    .map(|start| CropDrag::Create {
                        origin: position,
                        start,
                    })
            }
        };
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
        let (canvas_width, canvas_height) = match self.active_tab() {
            Some(tab) => tab.document.content.size(),
            None => return,
        };
        let crop = match drag {
            CropDrag::Create { origin, start } => {
                let delta = position - origin;
                self.canvas_pixel(position, scale, true)
                    .filter(|_| f32::from(delta.x).abs().max(f32::from(delta.y).abs()) >= 3.)
                    .map(|end| CropRect::between(start, end))
            }
            CropDrag::Move { crop, grab } => self
                .canvas_pixel(position, scale, true)
                .map(|pixel| move_crop(crop, pixel, grab, canvas_width, canvas_height)),
            CropDrag::Resize {
                crop,
                edges,
                anchor,
            } => self.unclamped_pointer_pixel(position, scale).map(|pixel| {
                resize_crop(
                    crop,
                    edges,
                    (pixel.0 - anchor.0, pixel.1 - anchor.1),
                    canvas_width,
                    canvas_height,
                )
            }),
        };
        if self.crop != crop {
            self.crop = crop;
            cx.notify();
        }
    }

    pub(super) fn crop_cursor(&self, scale: f32) -> CursorStyle {
        if let Some(drag) = self.crop_drag {
            return match drag {
                CropDrag::Create { .. } => CursorStyle::Crosshair,
                CropDrag::Move { .. } => CursorStyle::ClosedHand,
                CropDrag::Resize { edges, .. } => edges.cursor(),
            };
        }
        let Some(crop) = self.crop else {
            return CursorStyle::Crosshair;
        };
        let Some(position) = self.canvas_cursor.position.get() else {
            return CursorStyle::Crosshair;
        };
        let Some((left, top, right, bottom)) = self.crop_screen_rect(crop, scale) else {
            return CursorStyle::Crosshair;
        };
        match hit_crop(
            left,
            top,
            right,
            bottom,
            f32::from(position.x),
            f32::from(position.y),
            6.,
        ) {
            CropHit::Inside => CursorStyle::OpenHand,
            CropHit::Edge(edges) => edges.cursor(),
            CropHit::Outside => CursorStyle::Crosshair,
        }
    }

    fn crop_screen_rect(&self, crop: CropRect, scale: f32) -> Option<(f32, f32, f32, f32)> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        if width == 0 || height == 0 || crop.width == 0 || crop.height == 0 {
            return None;
        }
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let start = geometry.project(point(
            crop.x as f32 / width as f32,
            crop.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            (crop.x + crop.width) as f32 / width as f32,
            (crop.y + crop.height) as f32 / height as f32,
        ));
        Some((
            f32::from(start.x.min(end.x)),
            f32::from(start.y.min(end.y)),
            f32::from(start.x.max(end.x)),
            f32::from(start.y.max(end.y)),
        ))
    }

    pub(super) fn apply_crop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_text_pixels(true, cx) {
            return;
        }
        let Some(crop) = self.crop.take() else {
            return;
        };
        self.crop_drag = None;
        self.edit_content(
            move |content, _, selection| {
                if content.crop(crop) {
                    *selection = selection.clone().rebase(
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
        if self.refuse_text_pixels(false, cx) {
            return;
        }
        let Some((x, y)) = self.canvas_pixel(position, window.scale_factor(), false) else {
            return;
        };
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        let layer = tab.active_layer;
        let selection = tab.selection.clone();
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
        let request = crate::document::FillRequest {
            mode: self.fill_mode,
            sample_all: self.sample_all,
            tolerance: self.tolerance,
        };
        self.edit_content(
            move |content, _, selection| {
                content
                    .fill_bucket(layer, x, y, color, selection, request)
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((x, y)) = self.canvas_pixel(position, scale, false) else {
            return;
        };
        if let Some(document) = self.active_document() {
            let [r, g, b, alpha] = document.canvas.pixels.get_pixel(x, y).0;
            if alpha > 0 {
                self.set_color(0, u32::from_be_bytes([0, r, g, b]));
                self.refresh_gradient(window, cx);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CropHit {
    Outside,
    Inside,
    Edge(CropEdges),
}

fn hit_crop(left: f32, top: f32, right: f32, bottom: f32, x: f32, y: f32, slop: f32) -> CropHit {
    if x < left - slop || x > right + slop || y < top - slop || y > bottom + slop {
        return CropHit::Outside;
    }
    let mut near_left = (x - left).abs() <= slop;
    let mut near_right = (x - right).abs() <= slop;
    let mut near_top = (y - top).abs() <= slop;
    let mut near_bottom = (y - bottom).abs() <= slop;
    if near_left && near_right {
        near_left = x - left <= right - x;
        near_right = !near_left;
    }
    if near_top && near_bottom {
        near_top = y - top <= bottom - y;
        near_bottom = !near_top;
    }
    if near_left || near_right || near_top || near_bottom {
        return CropHit::Edge(CropEdges {
            left: near_left,
            right: near_right,
            top: near_top,
            bottom: near_bottom,
        });
    }
    CropHit::Inside
}

impl CropEdges {
    fn cursor(self) -> CursorStyle {
        match (self.left || self.right, self.top || self.bottom) {
            (true, true) if (self.left && self.top) || (self.right && self.bottom) => {
                CursorStyle::ResizeUpLeftDownRight
            }
            (true, true) => CursorStyle::ResizeUpRightDownLeft,
            (true, false) => CursorStyle::ResizeLeftRight,
            (false, true) => CursorStyle::ResizeUpDown,
            (false, false) => CursorStyle::Crosshair,
        }
    }
}

fn move_crop(
    crop: CropRect,
    pointer: (u32, u32),
    grab: (i64, i64),
    canvas_width: u32,
    canvas_height: u32,
) -> CropRect {
    let max_x = canvas_width.saturating_sub(crop.width) as i64;
    let max_y = canvas_height.saturating_sub(crop.height) as i64;
    CropRect {
        x: (i64::from(pointer.0) - grab.0).clamp(0, max_x) as u32,
        y: (i64::from(pointer.1) - grab.1).clamp(0, max_y) as u32,
        width: crop.width,
        height: crop.height,
    }
}

fn resize_crop(
    crop: CropRect,
    edges: CropEdges,
    delta: (i64, i64),
    canvas_width: u32,
    canvas_height: u32,
) -> CropRect {
    let clamp_x = |value: i64| value.clamp(0, i64::from(canvas_width).saturating_sub(1).max(0));
    let clamp_y = |value: i64| value.clamp(0, i64::from(canvas_height).saturating_sub(1).max(0));
    let mut x0 = i64::from(crop.x);
    let mut y0 = i64::from(crop.y);
    let mut x1 = i64::from(crop.x + crop.width).saturating_sub(1);
    let mut y1 = i64::from(crop.y + crop.height).saturating_sub(1);
    if edges.left {
        x0 = clamp_x(x0.saturating_add(delta.0));
    }
    if edges.right {
        x1 = clamp_x(x1.saturating_add(delta.0));
    }
    if edges.top {
        y0 = clamp_y(y0.saturating_add(delta.1));
    }
    if edges.bottom {
        y1 = clamp_y(y1.saturating_add(delta.1));
    }
    let left = x0.min(x1);
    let right = x0.max(x1);
    let top = y0.min(y1);
    let bottom = y0.max(y1);
    CropRect {
        x: left as u32,
        y: top as u32,
        width: (right - left + 1) as u32,
        height: (bottom - top + 1) as u32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_moves_and_resizes_inside_the_canvas() {
        let crop = CropRect {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        };
        assert_eq!(
            move_crop(crop, (15, 25), (2, 3), 100, 100),
            CropRect {
                x: 13,
                y: 22,
                width: 30,
                height: 40,
            }
        );
        assert_eq!(
            move_crop(crop, (200, 200), (0, 0), 100, 100),
            CropRect {
                x: 70,
                y: 60,
                width: 30,
                height: 40,
            }
        );
        let right = CropEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_crop(crop, right, (11, 0), 100, 100),
            CropRect {
                x: 10,
                y: 20,
                width: 41,
                height: 40,
            }
        );
        let left = CropEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_crop(crop, left, (40, 0), 100, 100),
            CropRect {
                x: 39,
                y: 20,
                width: 12,
                height: 40,
            }
        );
    }

    #[test]
    fn crop_corner_cursors_follow_their_diagonals() {
        assert_eq!(
            CropEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            }
            .cursor(),
            CursorStyle::ResizeUpLeftDownRight
        );
        assert_eq!(
            CropEdges {
                left: false,
                right: true,
                top: true,
                bottom: false,
            }
            .cursor(),
            CursorStyle::ResizeUpRightDownLeft
        );
    }

    #[test]
    fn crop_hit_edges_win_over_the_inside() {
        assert_eq!(
            hit_crop(0., 0., 100., 80., 1., 1., 6.),
            CropHit::Edge(CropEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(hit_crop(0., 0., 100., 80., 50., 40., 6.), CropHit::Inside);
        assert_eq!(hit_crop(0., 0., 100., 80., 120., 40., 6.), CropHit::Outside);
    }
}
