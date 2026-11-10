use std::{
    cell::Cell,
    f32::consts::TAU,
    rc::Rc,
    sync::{Arc, OnceLock},
};

use gpui::{
    BorderStyle, Bounds, Context, Corners, CursorStyle, Div, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage, Stateful, Window, canvas, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, quad, rgb, size,
};
use image::{Frame, Rgba, RgbaImage};
use smallvec::smallvec;

use super::Workspace;

#[derive(Clone, Copy)]
pub(super) struct Hsv {
    hue: f32,
    saturation: f32,
    value: f32,
}

impl Hsv {
    pub fn from_rgb(color: u32) -> Self {
        let [_, r, g, b] = color.to_be_bytes();
        let [r, g, b] = [r, g, b].map(|channel| f32::from(channel) / 255.);
        let value = r.max(g).max(b);
        let delta = value - r.min(g).min(b);
        let hue = if delta == 0. {
            0.
        } else if value == r {
            (g - b) / delta
        } else if value == g {
            (b - r) / delta + 2.
        } else {
            (r - g) / delta + 4.
        };
        Self {
            hue: hue.rem_euclid(6.) / 6.,
            saturation: if value == 0. { 0. } else { delta / value },
            value,
        }
    }

    pub fn rgb(self) -> u32 {
        let channel = |offset: f32| {
            let sector = (offset + self.hue * 6.).rem_euclid(6.);
            let amount = sector.min(4. - sector).clamp(0., 1.);
            (self.value * (1. - self.saturation * amount) * 255.).round() as u8
        };
        u32::from_be_bytes([0, channel(5.), channel(3.), channel(1.)])
    }

    fn wheel_position(x: f32, y: f32, value: f32) -> Self {
        Self {
            hue: (y.atan2(x) / TAU).rem_euclid(1.),
            saturation: x.hypot(y).min(1.),
            value,
        }
    }

    fn bright(self) -> Self {
        Self { value: 1., ..self }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorControl {
    Wheel,
    Brightness,
}

impl ColorControl {
    pub fn at(
        self,
        hsv: Hsv,
        bounds: Bounds<Pixels>,
        position: Point<Pixels>,
        start: bool,
    ) -> Option<Hsv> {
        match self {
            Self::Wheel => {
                let radius = bounds.size.width / 2.;
                let offset = position - bounds.center();
                let (x, y) = (offset.x / radius, offset.y / radius);
                if start && x.hypot(y) > 1. {
                    return None;
                }
                // A hue selection from pure black should produce a visible color.
                let value = if start && hsv.value == 0. {
                    1.
                } else {
                    hsv.value
                };
                Some(Hsv::wheel_position(x, y, value))
            }
            Self::Brightness => {
                let value = ((position.x - bounds.left()) / bounds.size.width).clamp(0., 1.);
                Some(Hsv { value, ..hsv })
            }
        }
    }
}

pub(super) fn control(kind: ColorControl, hsv: Hsv, cx: &Context<Workspace>) -> Stateful<Div> {
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let layout_bounds = bounds.clone();
    let drag_bounds = bounds.clone();
    let up_bounds = bounds.clone();
    let outside_bounds = bounds.clone();
    let control = div()
        .id(match kind {
            ColorControl::Wheel => "hue-saturation-wheel",
            ColorControl::Brightness => "brightness-slider",
        })
        .cursor(CursorStyle::Crosshair)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.drag_color(kind, bounds.get(), event.position, true, cx);
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.dragging() {
                this.drag_color(kind, drag_bounds.get(), event.position, false, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                this.end_color_drag(kind, up_bounds.get(), event.position, cx);
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                this.end_color_drag(kind, outside_bounds.get(), event.position, cx);
            }),
        )
        .child(
            canvas(
                move |bounds, _, _| layout_bounds.set(bounds),
                move |bounds, _, window, _| paint_control(kind, hsv, bounds, window),
            )
            .size_full(),
        );
    match kind {
        ColorControl::Wheel => control.size(px(224.)),
        ColorControl::Brightness => control.h(px(22.)).w_full(),
    }
}

fn paint_control(kind: ColorControl, hsv: Hsv, bounds: Bounds<Pixels>, window: &mut Window) {
    let marker = match kind {
        ColorControl::Wheel => {
            let image = wheel_image();
            if window
                .paint_image(bounds, Corners::default(), image, 0, false)
                .is_err()
            {
                return;
            }
            let angle = hsv.hue * TAU;
            let radius = bounds.size.width / 2. * hsv.saturation;
            bounds.center() + point(radius * angle.cos(), radius * angle.sin())
        }
        ColorControl::Brightness => {
            let track = Bounds::centered_at(bounds.center(), size(bounds.size.width, px(12.)));
            let gradient = linear_gradient(
                90.,
                linear_color_stop(rgb(0), 0.),
                linear_color_stop(rgb(hsv.bright().rgb()), 1.),
            );
            window.paint_quad(quad(
                track,
                px(6.),
                gradient,
                px(1.),
                rgb(0x545c69),
                BorderStyle::Solid,
            ));
            point(
                bounds.left() + bounds.size.width * hsv.value,
                bounds.center().y,
            )
        }
    };
    window.paint_quad(quad(
        Bounds::centered_at(marker, size(px(14.), px(14.))),
        px(7.),
        rgb(0x171a20),
        px(1.),
        rgb(0xffffff),
        BorderStyle::Solid,
    ));
    window.paint_quad(quad(
        Bounds::centered_at(marker, size(px(8.), px(8.))),
        px(4.),
        rgb(hsv.rgb()),
        px(0.),
        rgb(0),
        BorderStyle::Solid,
    ));
}

fn wheel_image() -> Arc<RenderImage> {
    static WHEEL: OnceLock<Arc<RenderImage>> = OnceLock::new();
    WHEEL
        .get_or_init(|| {
            const EDGE: u32 = 448;
            let radius = EDGE as f32 / 2.;
            let pixels = RgbaImage::from_fn(EDGE, EDGE, |x, y| {
                let x = x as f32 + 0.5 - radius;
                let y = y as f32 + 0.5 - radius;
                let alpha = (radius - x.hypot(y) + 0.5).clamp(0., 1.);
                let [_, r, g, b] = Hsv::wheel_position(x / radius, y / radius, 1.)
                    .rgb()
                    .to_be_bytes();
                Rgba([b, g, r, (alpha * 255.).round() as u8])
            });
            Arc::new(RenderImage::new(smallvec![Frame::new(pixels)]))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_preserves_rgb_on_round_trip() {
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let color = u32::from_be_bytes([0, r, g, b]);
                    assert_eq!(Hsv::from_rgb(color).rgb(), color);
                }
            }
        }
    }

    #[test]
    fn selecting_hue_from_black_is_visible_and_brightness_preserves_hue() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.)));
        let hsv = ColorControl::Wheel
            .at(Hsv::from_rgb(0), bounds, point(px(200.), px(100.)), true)
            .unwrap();
        assert_eq!(hsv.rgb(), 0xff0000);
        let dimmed = ColorControl::Brightness
            .at(hsv, bounds, point(px(100.), px(0.)), false)
            .unwrap();
        assert_eq!(dimmed.rgb(), 0x800000);
    }
}
