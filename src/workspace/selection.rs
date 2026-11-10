use gpui::{Action, Context, CursorStyle, Div, Pixels, Point, Window, point, prelude::*};

use super::Workspace;
use super::canvas_tools::PreviewGeometry;
use crate::commands::{
    ClearToTransparency, CropToSelection, InvertSelection, SelectAll, SelectNone,
};
use crate::document::CropRect;
use crate::preview::{Dirty, PixelRect};
use crate::selection::Selection;

const HANDLE_SLOP: f32 = 6.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionGesture {
    Create {
        origin: Point<Pixels>,
        anchor: Option<(u32, u32)>,
    },
    Move {
        rect: PixelRect,
        grab: (i64, i64),
    },
    Resize {
        rect: PixelRect,
        edges: SelectionEdges,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionHit {
    Outside,
    Inside,
    Edge(SelectionEdges),
}

impl Workspace {
    pub(super) fn clear_selection_gesture(&mut self) {
        self.selection_drag = None;
        self.selection_preview = None;
    }

    pub(super) fn begin_selection(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab().is_none_or(|tab| tab.busy()) {
            return;
        }
        self.clear_selection_gesture();
        let Some(selection) = self.active_tab().map(|tab| tab.selection) else {
            return;
        };
        let gesture = if let Some(rect) = adjustable_rect(selection) {
            let hit = self
                .selection_screen_rect(rect, scale)
                .map(|(left, top, right, bottom)| {
                    hit_selection(
                        left,
                        top,
                        right,
                        bottom,
                        f32::from(position.x),
                        f32::from(position.y),
                        HANDLE_SLOP,
                    )
                })
                .unwrap_or(SelectionHit::Outside);
            match hit {
                SelectionHit::Edge(edges) => SelectionGesture::Resize { rect, edges },
                SelectionHit::Inside => {
                    let pixel = self
                        .pointer_pixel(position, scale)
                        .unwrap_or((rect.x, rect.y));
                    SelectionGesture::Move {
                        rect,
                        grab: (pixel.0 - rect.x, pixel.1 - rect.y),
                    }
                }
                SelectionHit::Outside => SelectionGesture::Create {
                    origin: position,
                    anchor: self.canvas_pixel(position, scale, false),
                },
            }
        } else if let Some(anchor) = self.canvas_pixel(position, scale, false) {
            SelectionGesture::Create {
                origin: position,
                anchor: Some(anchor),
            }
        } else {
            return;
        };
        self.selection_drag = Some(gesture);
        cx.notify();
    }

    pub(super) fn extend_selection(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.selection_drag else {
            return;
        };
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (width, height) = tab.document.content.size();
        let previous = tab.selection;
        let pointer = self.pointer_pixel(position, scale);
        let preview = match drag {
            SelectionGesture::Create {
                origin,
                anchor: Some(start),
            } => {
                let delta = position - origin;
                if f32::from(delta.x).abs().max(f32::from(delta.y).abs()) < 3. {
                    None
                } else {
                    pointer.map(|end| {
                        Selection::rectangle(start, (end.0 as u32, end.1 as u32), width, height)
                    })
                }
            }
            SelectionGesture::Create { anchor: None, .. } => None,
            SelectionGesture::Move { rect, grab } => pointer.map(|pixel| {
                placed_selection(
                    previous,
                    move_rect(rect, pixel, grab, width, height),
                    width,
                    height,
                )
            }),
            SelectionGesture::Resize { rect, edges } => pointer.map(|pixel| {
                placed_selection(
                    previous,
                    resize_rect(rect, edges, pixel, width, height),
                    width,
                    height,
                )
            }),
        };
        if self.selection_preview != preview {
            self.selection_preview = preview;
            cx.notify();
        }
    }

    pub(super) fn finish_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let gesture = self.selection_drag.take();
        let preview = self.selection_preview.take();
        match gesture {
            // A click that misses the box clears it. A drag that never crosses the
            // threshold does the same, which is how a click outside deselects.
            Some(SelectionGesture::Create { .. }) if preview.is_none() => {
                self.select_none(window, cx);
            }
            Some(_) => {
                if let Some(selection) = preview {
                    self.commit_selection(selection, window, cx);
                } else {
                    cx.notify();
                }
            }
            None => {}
        }
    }

    pub(super) fn selection_cursor(&self, scale: f32) -> CursorStyle {
        if let Some(gesture) = self.selection_drag {
            return match gesture {
                SelectionGesture::Create { .. } => CursorStyle::Crosshair,
                SelectionGesture::Move { .. } => CursorStyle::ClosedHand,
                SelectionGesture::Resize { edges, .. } => edges.cursor(),
            };
        }
        let Some(position) = self.canvas_cursor.position.get() else {
            return CursorStyle::Crosshair;
        };
        let Some(selection) = self.active_tab().map(|tab| tab.selection) else {
            return CursorStyle::Crosshair;
        };
        let Some(rect) = adjustable_rect(selection) else {
            return CursorStyle::Crosshair;
        };
        let Some((left, top, right, bottom)) = self.selection_screen_rect(rect, scale) else {
            return CursorStyle::Crosshair;
        };
        match hit_selection(
            left,
            top,
            right,
            bottom,
            f32::from(position.x),
            f32::from(position.y),
            HANDLE_SLOP,
        ) {
            SelectionHit::Inside => CursorStyle::OpenHand,
            SelectionHit::Edge(edges) => edges.cursor(),
            SelectionHit::Outside => CursorStyle::Crosshair,
        }
    }

    fn pointer_pixel(&self, position: Point<Pixels>, scale: f32) -> Option<(i64, i64)> {
        let tab = self.active_tab()?;
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let fraction = geometry.fraction(position);
        Some((
            fraction_pixel(fraction.x, tab.document.width()),
            fraction_pixel(fraction.y, tab.document.height()),
        ))
    }

    fn selection_screen_rect(&self, rect: PixelRect, scale: f32) -> Option<(f32, f32, f32, f32)> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        if width == 0 || height == 0 {
            return None;
        }
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let start = geometry.project(point(
            rect.x as f32 / width as f32,
            rect.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            rect.right() as f32 / width as f32,
            rect.bottom() as f32 / height as f32,
        ));
        Some((
            f32::from(start.x.min(end.x)),
            f32::from(start.y.min(end.y)),
            f32::from(start.x.max(end.x)),
            f32::from(start.y.max(end.y)),
        ))
    }

    pub(super) fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_selection(Selection::Full, window, cx);
    }

    pub(super) fn select_none(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_selection(Selection::Unrestricted, window, cx);
    }

    pub(super) fn invert_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (width, height) = tab.document.content.size();
        let selection = tab.selection.invert(width, height);
        self.commit_selection(selection, window, cx);
    }

    pub(super) fn clear_to_transparency(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active.map(|index| &mut self.tabs[index]) else {
            return;
        };
        if tab.selection == Selection::Empty {
            tab.notice = Some("Nothing is selected.".into());
            cx.notify();
            return;
        }
        let layer = tab.active_layer;
        if tab
            .document
            .content
            .layer(layer)
            .is_some_and(|layer| !layer.visible)
        {
            tab.notice = Some("The active layer is hidden. Show it before clearing.".into());
            cx.notify();
            return;
        }
        self.edit_content(
            move |content, _, selection| {
                content
                    .clear(layer, selection)
                    .map(Dirty::Region)
                    .unwrap_or(Dirty::None)
            },
            window,
            cx,
        );
    }

    pub(super) fn crop_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bounds) = self.selection_crop() else {
            return;
        };
        let crop = CropRect {
            x: bounds.x as u32,
            y: bounds.y as u32,
            width: bounds.width,
            height: bounds.height,
        };
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
    }

    pub(super) fn bind_selection_actions(&self, root: Div, cx: &Context<Self>) -> Div {
        let root = self.bind_selection::<SelectAll>(root, SelectionCommand::All, cx);
        let root = self.bind_selection::<SelectNone>(root, SelectionCommand::None, cx);
        let root = self.bind_selection::<InvertSelection>(root, SelectionCommand::Invert, cx);
        let root = self.bind_selection::<ClearToTransparency>(root, SelectionCommand::Clear, cx);
        self.bind_selection::<CropToSelection>(root, SelectionCommand::Crop, cx)
    }

    fn bind_selection<A: Action>(
        &self,
        root: Div,
        command: SelectionCommand,
        cx: &Context<Self>,
    ) -> Div {
        if !self.selection_command_enabled(command) {
            return root;
        }
        root.on_action(cx.listener(move |this, _: &A, window, cx| match command {
            SelectionCommand::All => this.select_all(window, cx),
            SelectionCommand::None => this.select_none(window, cx),
            SelectionCommand::Invert => this.invert_selection(window, cx),
            SelectionCommand::Clear => this.clear_to_transparency(window, cx),
            SelectionCommand::Crop => this.crop_to_selection(window, cx),
        }))
    }

    pub(super) fn selection_command_enabled(&self, command: SelectionCommand) -> bool {
        let Some(tab) = self.active_tab() else {
            return false;
        };
        if tab.busy() {
            return false;
        }
        match command {
            SelectionCommand::All | SelectionCommand::None | SelectionCommand::Invert => true,
            SelectionCommand::Clear => tab.selection != Selection::Empty,
            SelectionCommand::Crop => self.selection_crop().is_some(),
        }
    }

    fn selection_crop(&self) -> Option<PixelRect> {
        let tab = self.active_tab()?;
        let (width, height) = tab.document.content.size();
        tab.selection.crop_bounds(width, height)
    }

    fn commit_selection(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_tab()
            .is_some_and(|tab| tab.selection == selection)
        {
            cx.notify();
            return;
        }
        self.edit_content(
            move |_, _, current| {
                *current = selection;
                Dirty::Unpainted
            },
            window,
            cx,
        );
    }
}

pub(super) fn adjustable_rect(selection: Selection) -> Option<PixelRect> {
    match selection {
        Selection::Rect(rect) | Selection::Inverse(rect) if rect.width > 0 && rect.height > 0 => {
            Some(rect)
        }
        _ => None,
    }
}

fn placed_selection(previous: Selection, rect: PixelRect, width: u32, height: u32) -> Selection {
    let covers = width > 0
        && height > 0
        && rect.x <= 0
        && rect.y <= 0
        && rect.right() >= i64::from(width)
        && rect.bottom() >= i64::from(height);
    match previous {
        Selection::Inverse(_) => {
            if covers {
                Selection::Empty
            } else {
                Selection::Inverse(rect)
            }
        }
        _ => {
            if covers {
                Selection::Full
            } else {
                Selection::Rect(rect)
            }
        }
    }
}

fn move_rect(
    rect: PixelRect,
    pointer: (i64, i64),
    grab: (i64, i64),
    width: u32,
    height: u32,
) -> PixelRect {
    let max_x = i64::from(width)
        .saturating_sub(i64::from(rect.width))
        .max(0);
    let max_y = i64::from(height)
        .saturating_sub(i64::from(rect.height))
        .max(0);
    PixelRect::new(
        (pointer.0 - grab.0).clamp(0, max_x),
        (pointer.1 - grab.1).clamp(0, max_y),
        rect.width,
        rect.height,
    )
}

fn resize_rect(
    rect: PixelRect,
    edges: SelectionEdges,
    pointer: (i64, i64),
    width: u32,
    height: u32,
) -> PixelRect {
    let clamp_x = |value: i64| value.clamp(0, i64::from(width).saturating_sub(1).max(0));
    let clamp_y = |value: i64| value.clamp(0, i64::from(height).saturating_sub(1).max(0));
    let mut x0 = rect.x;
    let mut y0 = rect.y;
    let mut x1 = rect.right().saturating_sub(1);
    let mut y1 = rect.bottom().saturating_sub(1);
    if edges.left {
        x0 = clamp_x(pointer.0);
    }
    if edges.right {
        x1 = clamp_x(pointer.0);
    }
    if edges.top {
        y0 = clamp_y(pointer.1);
    }
    if edges.bottom {
        y1 = clamp_y(pointer.1);
    }
    let left = x0.min(x1);
    let right = x0.max(x1);
    let top = y0.min(y1);
    let bottom = y0.max(y1);
    PixelRect::new(
        left,
        top,
        (right - left + 1) as u32,
        (bottom - top + 1) as u32,
    )
}

fn fraction_pixel(fraction: f32, edge: u32) -> i64 {
    if edge == 0 {
        return 0;
    }
    let scaled = fraction * edge as f32;
    if scaled <= 0. {
        0
    } else {
        (scaled.floor() as i64).min(i64::from(edge) - 1)
    }
}

fn hit_selection(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    x: f32,
    y: f32,
    slop: f32,
) -> SelectionHit {
    if x < left - slop || x > right + slop || y < top - slop || y > bottom + slop {
        return SelectionHit::Outside;
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
        return SelectionHit::Edge(SelectionEdges {
            left: near_left,
            right: near_right,
            top: near_top,
            bottom: near_bottom,
        });
    }
    SelectionHit::Inside
}

impl SelectionEdges {
    fn cursor(self) -> CursorStyle {
        match (self.left || self.right, self.top || self.bottom) {
            (true, true) if (self.left && self.top) || (self.right && self.bottom) => {
                CursorStyle::ResizeUpRightDownLeft
            }
            (true, true) => CursorStyle::ResizeUpLeftDownRight,
            (true, false) => CursorStyle::ResizeLeftRight,
            (false, true) => CursorStyle::ResizeUpDown,
            (false, false) => CursorStyle::Crosshair,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SelectionCommand {
    All,
    None,
    Invert,
    Clear,
    Crop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_win_over_edges_and_a_click_inside_is_not_an_edge() {
        assert_eq!(
            hit_selection(0., 0., 100., 80., 1., 1., 6.),
            SelectionHit::Edge(SelectionEdges {
                left: true,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 50., 2., 6.),
            SelectionHit::Edge(SelectionEdges {
                left: false,
                right: false,
                top: true,
                bottom: false,
            })
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 50., 40., 6.),
            SelectionHit::Inside
        );
        assert_eq!(
            hit_selection(0., 0., 100., 80., 120., 40., 6.),
            SelectionHit::Outside
        );
    }

    #[test]
    fn moving_stays_on_the_canvas_and_resizing_can_cross_the_opposite_edge() {
        let rect = PixelRect::new(10, 20, 30, 40);
        assert_eq!(
            move_rect(rect, (22, 33), (2, 3), 100, 100),
            PixelRect::new(20, 30, 30, 40)
        );
        assert_eq!(
            move_rect(rect, (0, 0), (2, 3), 100, 100),
            PixelRect::new(0, 0, 30, 40)
        );
        assert_eq!(
            move_rect(rect, (200, 200), (0, 0), 100, 100),
            PixelRect::new(70, 60, 30, 40)
        );
        let right = SelectionEdges {
            left: false,
            right: true,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_rect(rect, right, (50, 20), 100, 100),
            PixelRect::new(10, 20, 41, 40)
        );
        let left = SelectionEdges {
            left: true,
            right: false,
            top: false,
            bottom: false,
        };
        assert_eq!(
            resize_rect(rect, left, (50, 20), 100, 100),
            PixelRect::new(39, 20, 12, 40)
        );
    }

    #[test]
    fn an_adjusted_inverse_stays_inverse_until_it_covers_the_canvas() {
        let hole = PixelRect::new(1, 1, 2, 2);
        assert_eq!(
            placed_selection(Selection::Inverse(hole), hole, 8, 8),
            Selection::Inverse(hole)
        );
        assert_eq!(
            placed_selection(Selection::Inverse(hole), PixelRect::new(0, 0, 8, 8), 8, 8),
            Selection::Empty
        );
        assert_eq!(
            placed_selection(Selection::Rect(hole), PixelRect::new(0, 0, 8, 8), 8, 8),
            Selection::Full
        );
    }

    #[test]
    fn a_fraction_outside_the_image_clamps_to_the_near_edge() {
        assert_eq!(fraction_pixel(-0.2, 10), 0);
        assert_eq!(fraction_pixel(0.15, 10), 1);
        assert_eq!(fraction_pixel(3., 10), 9);
    }
}
