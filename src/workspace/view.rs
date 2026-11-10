use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, Corners, CursorStyle, Div, ExternalPaths, MouseButton, ObjectFit, Pixels,
    Point, Window, canvas, div, fill, img, point, prelude::*, px, rgb, rgba, size, svg,
};

use crate::commands::{
    ADD_LAYER, CloseTab, MinimizeWindow, NextTab, OPEN_HINT, OpenImage, PreviousTab, SelectTool,
    TOOL_GROUPS, ToggleGrid, ToggleGuides, Tool, Zoom, ZoomWindow,
};

use super::{DEFAULT_COLORS, Tab, Workspace};

const CHROME: u32 = 0x20242c;
const PANEL: u32 = 0x191d24;
const CANVAS: u32 = 0x11141a;
const BORDER: u32 = 0x303640;
const TEXT: u32 = 0xe9edf4;
const MUTED: u32 = 0xa3adbd;
const DISABLED: u32 = 0x767f8d;
const HOVER: u32 = 0x2b323d;
const ACCENT: u32 = 0xacc6ff;
const SELECTED: u32 = 0x2c3a54;

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, action: &SelectTool, _, cx| {
                this.tool = action.0;
                this.pan_drag = None;
                cx.notify();
            }))
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .when(self.active.is_some(), |root| {
                root.on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                    if let Some(index) = this.active {
                        this.close_tab(index, window, cx);
                    }
                }))
                .on_action(cx.listener(|this, zoom: &Zoom, _, cx| {
                    if let Some(index) = this.active {
                        this.tabs[index].zoom = *zoom;
                        this.tabs[index].pan = Point::default();
                        this.pan_drag = None;
                        cx.notify();
                    }
                }))
                .on_action(cx.listener(|this, _: &ToggleGrid, _, cx| {
                    this.show_grid = !this.show_grid;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &ToggleGuides, _, cx| {
                    this.show_guides = !this.show_guides;
                    cx.notify();
                }))
            })
            .when(self.tabs.len() > 1, |root| {
                root.on_action(cx.listener(|this, _: &NextTab, window, cx| {
                    this.cycle_tabs(false, window, cx);
                }))
                .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                    this.cycle_tabs(true, window, cx);
                }))
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.toolbox(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(self.tab_strip(cx))
                            .when(!self.errors.is_empty(), |area| {
                                area.child(self.error_banner(cx))
                            })
                            .child(self.canvas(cx)),
                    )
                    .child(self.inspector()),
            )
            .child(self.status_bar())
    }
}

impl Workspace {
    fn toolbox(&self, cx: &Context<Self>) -> Div {
        div()
            .w(px(220.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Toolbox"),
            )
            .child(
                div()
                    .id("toolbox-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .children(TOOL_GROUPS.iter().map(|(label, tools)| {
                        div().pb_2().child(section_label(label).pb_1()).child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .children(tools.iter().copied().map(|tool| self.tool_button(tool))),
                        )
                    })),
            )
            .child(self.color_box(cx))
    }

    fn tool_button(&self, tool: Tool) -> impl IntoElement {
        let info = tool.info();
        div()
            .id(info.icon)
            .w(px(62.))
            .h(px(43.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .rounded_md()
            .bg(rgb(CHROME))
            .cursor_pointer()
            .when_else(
                self.tool == tool,
                |button| button.bg(rgb(SELECTED)).text_color(rgb(ACCENT)),
                |button| button.hover(|button| button.bg(rgb(HOVER))),
            )
            .child(icon(info.icon, 17.))
            .child(
                div()
                    .text_size(px(10.))
                    .whitespace_nowrap()
                    .child(info.label),
            )
            .on_click(move |_, window, cx| window.dispatch_action(Box::new(SelectTool(tool)), cx))
    }

    fn color_box(&self, cx: &Context<Self>) -> Div {
        let [foreground, background] = self.colors;
        div()
            .flex_shrink_0()
            .p_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(color_swatch("Foreground", foreground))
                    .child(color_swatch("Background", background)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .mt_1()
                    .child(color_button("Swap").on_click(cx.listener(|this, _, _, cx| {
                        this.colors.swap(0, 1);
                        cx.notify();
                    })))
                    .child(
                        color_button("Reset").on_click(cx.listener(|this, _, _, cx| {
                            this.colors = DEFAULT_COLORS;
                            cx.notify();
                        })),
                    ),
            )
    }

    fn tab_strip(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("tabs")
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .bg(rgb(PANEL))
            .border_b_1()
            .border_color(rgb(BORDER))
            .when(self.tabs.is_empty(), |strip| {
                strip.child(
                    div()
                        .px_4()
                        .flex()
                        .items_center()
                        .text_color(rgb(MUTED))
                        .child("No open images"),
                )
            })
            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                let selected = self.active == Some(index);
                let id = tab.id;
                div()
                    .id(("tab", id))
                    .w(px(188.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pr_2()
                    .border_r_1()
                    .border_t_2()
                    .border_color(rgb(if selected { ACCENT } else { PANEL }))
                    .bg(rgb(if selected { CHROME } else { PANEL }))
                    .child(
                        div()
                            .id("select")
                            .h_full()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .pl_3()
                            .pr_1()
                            .cursor_pointer()
                            .text_color(rgb(if selected { TEXT } else { MUTED }))
                            .child(icon("image", 14.))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(tab.document.name.clone()),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(index) = this.tab_index(id) {
                                    this.select_tab(index, window, cx);
                                }
                            })),
                    )
                    .child(
                        icon_button("close")
                            .id("close")
                            .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(index) = this.tab_index(id) {
                                    this.close_tab(index, window, cx);
                                }
                            })),
                    )
            }))
    }

    fn canvas(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("image-canvas")
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(rgb(CANVAS))
            .on_drop(cx.listener(|this, files: &ExternalPaths, _, cx| {
                this.open_paths(files.paths().to_vec(), cx);
            }))
            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(SELECTED)))
            .map(|area| match self.active_tab() {
                Some(tab) => area
                    .when(self.tool == Tool::Pan, |area| {
                        area.cursor(if self.pan_drag.is_some() {
                            CursorStyle::ClosedHand
                        } else {
                            CursorStyle::OpenHand
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                this.pan_drag = Some(event.position);
                                cx.notify();
                            }),
                        )
                        .on_mouse_move(cx.listener(
                            |this, event: &gpui::MouseMoveEvent, window, cx| {
                                if event.pressed_button == Some(MouseButton::Left) {
                                    this.drag_preview(event.position, window.scale_factor(), cx);
                                }
                            },
                        ))
                        .capture_any_mouse_up(cx.listener(
                            |this, event: &gpui::MouseUpEvent, _, cx| {
                                if event.button == MouseButton::Left
                                    && this.pan_drag.take().is_some()
                                {
                                    cx.notify();
                                }
                            },
                        ))
                    })
                    .when(self.tool == Tool::Eyedropper, |area| {
                        area.cursor(CursorStyle::Crosshair).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                if let Some(color) = this.active_tab().and_then(|tab| {
                                    sample_color(
                                        tab,
                                        this.viewport.get(),
                                        window.scale_factor(),
                                        event.position(),
                                    )
                                }) {
                                    this.colors[0] = color;
                                    cx.notify();
                                }
                            },
                        ))
                    })
                    .child(image_preview(
                        tab,
                        self.viewport.clone(),
                        self.show_grid,
                        self.show_guides,
                    )),
                None => area.child(dot_grid()).child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .p_4()
                        .text_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(58.))
                                .rounded_lg()
                                .bg(rgb(SELECTED))
                                .text_color(rgb(ACCENT))
                                .border_1()
                                .border_color(rgb(0x405477))
                                .child(icon("image", 26.)),
                        )
                        .child(
                            div()
                                .text_size(px(24.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Open an image"),
                        )
                        .child(
                            div()
                                .text_color(rgb(MUTED))
                                .child("Drop a PNG, JPEG, or WebP here to preview it."),
                        )
                        .child(
                            button("folder", "Choose Files…")
                                .id("empty-open")
                                .mt_1()
                                .px_4()
                                .h(px(36.))
                                .bg(rgb(ACCENT))
                                .text_color(rgb(CANVAS))
                                .hover(|button| button.bg(rgb(0xc5d7ff)))
                                .cursor_pointer()
                                .on_click(open_image),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(OPEN_HINT)),
                ),
            })
    }

    fn drag_preview(&mut self, position: Point<Pixels>, scale: f32, cx: &mut Context<Self>) {
        let (Some(last), Some(index)) = (self.pan_drag, self.active) else {
            return;
        };
        self.pan_drag = Some(position);
        let tab = &mut self.tabs[index];
        if let Some(geometry) = PreviewGeometry::new(
            tab.document.width,
            tab.document.height,
            tab.zoom,
            self.viewport.get(),
            scale,
        ) {
            let limit = geometry.pan_limit;
            let pan = constrain_pan(constrain_pan(tab.pan, limit) + position - last, limit);
            if pan != tab.pan {
                tab.pan = pan;
                cx.notify();
            }
        }
    }

    fn inspector(&self) -> Div {
        let tool = self.tool.info();
        div()
            .w(px(232.))
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .id("inspector-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .child(section_label("TOOL OPTIONS"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(icon(tool.icon, 16.))
                                    .child(tool.label),
                            )
                            .child(
                                div()
                                    .text_color(rgb(MUTED))
                                    .line_height(px(19.))
                                    .child(tool.description),
                            )
                            .when(
                                !matches!(self.tool, Tool::Pan | Tool::Eyedropper),
                                |panel| {
                                    panel.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .rounded_md()
                                            .p_2()
                                            .bg(rgb(CHROME))
                                            .text_color(rgb(MUTED))
                                            .child(icon("info", 14.))
                                            .child("Planned tool"),
                                    )
                                },
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("LAYERS"))
                                    .child(icon_button("layers")),
                            )
                            .when_else(
                                self.active_document().is_some(),
                                |panel| {
                                    let document = self.active_document().unwrap();
                                    panel
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .rounded_md()
                                                .p_2()
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .bg(rgb(CHROME))
                                                .child(
                                                    img(document.preview.clone())
                                                        .size(px(34.))
                                                        .flex_shrink_0()
                                                        .rounded_sm()
                                                        .bg(rgb(CANVAS))
                                                        .object_fit(ObjectFit::Contain),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_ellipsis()
                                                        .child("Source image"),
                                                )
                                                .child(
                                                    icon_button("eye").text_color(rgb(DISABLED)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .justify_between()
                                                .text_color(rgb(DISABLED))
                                                .child("Opacity")
                                                .child("100%"),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .gap_1()
                                                .text_xs()
                                                .text_color(rgb(DISABLED))
                                                .children(["Rename…", "Raise", "Lower"].map(
                                                    |label| {
                                                        div()
                                                            .flex()
                                                            .flex_1()
                                                            .justify_center()
                                                            .py_1()
                                                            .rounded_sm()
                                                            .bg(rgb(CHROME))
                                                            .child(label)
                                                    },
                                                )),
                                        )
                                },
                                |panel| {
                                    panel.child(
                                        div()
                                            .rounded_md()
                                            .p_3()
                                            .border_1()
                                            .border_color(rgb(BORDER))
                                            .text_color(rgb(MUTED))
                                            .child("Open an image to see its source here."),
                                    )
                                },
                            )
                            .child(button("plus", ADD_LAYER).text_color(rgb(DISABLED))),
                    ),
            )
    }

    fn status_bar(&self) -> Div {
        let summary = if self.pending > 0 {
            format!("Opening {}…", self.pending)
        } else {
            self.active_tab().map_or_else(
                || "Ready".to_owned(),
                |tab| {
                    let document = &tab.document;
                    let zoom = match tab.zoom {
                        Zoom::Fit => "Fit".to_owned(),
                        Zoom::Percent(percent) => format!("{percent}%"),
                    };
                    format!(
                        "{}  ·  {} × {} px  ·  {zoom}",
                        document.format, document.width, document.height
                    )
                },
            )
        };
        let count = self.tabs.len();
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(28.))
            .flex_shrink_0()
            .px_4()
            .bg(rgb(CHROME))
            .border_t_1()
            .border_color(rgb(BORDER))
            .text_xs()
            .text_color(rgb(MUTED))
            .child(summary)
            .child(if count == 1 {
                "1 image open".to_owned()
            } else {
                format!("{count} images open")
            })
    }

    fn error_banner(&self, cx: &Context<Self>) -> Div {
        let message = &self.errors[self.errors.len() - 1];
        let previous = self.errors.len() - 1;
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(rgb(0x402c2d))
            .text_color(rgb(0xffc5b9))
            .child(icon("info", 16.))
            .child(div().min_w_0().flex_1().child(message.clone()))
            .when(previous > 0, |banner| {
                banner.child(format!("+{previous} more"))
            })
            .child(
                icon_button("close")
                    .id("dismiss-error")
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(HOVER)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.errors.clear();
                        cx.notify();
                    })),
            )
    }
}

fn open_image(_: &gpui::ClickEvent, window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(OpenImage), cx);
}

fn color_swatch(label: &'static str, color: u32) -> Div {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .gap_1()
        .child(
            div()
                .h(px(22.))
                .rounded_sm()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(color)),
        )
        .child(section_label(label))
}

fn color_button(label: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .flex_1()
        .flex()
        .justify_center()
        .py_1()
        .rounded_sm()
        .text_size(px(11.))
        .text_color(rgb(MUTED))
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .cursor_pointer()
        .child(label)
}

fn button(symbol: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(30.))
        .px_2()
        .rounded_md()
        .child(icon(symbol, 16.))
        .child(label)
}

fn icon_button(symbol: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(22.))
        .flex_shrink_0()
        .rounded_sm()
        .text_color(rgb(MUTED))
        .child(icon(symbol, 14.))
}

fn section_label(text: &'static str) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text)
}

#[derive(IntoElement)]
struct Icon {
    path: &'static str,
    edge: f32,
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        svg()
            .path(self.path)
            .size(px(self.edge))
            .flex_shrink_0()
            .text_color(window.text_style().color)
    }
}

fn icon(path: &'static str, edge: f32) -> Icon {
    Icon { path, edge }
}

fn image_preview(
    tab: &Tab,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    show_grid: bool,
    show_guides: bool,
) -> impl IntoElement {
    let image = tab.document.preview.clone();
    let width = tab.document.width;
    let height = tab.document.height;
    let zoom = tab.zoom;
    let pan = tab.pan;

    canvas(
        move |bounds, window, _| {
            viewport.set(bounds);
            let mut geometry =
                PreviewGeometry::new(width, height, zoom, bounds, window.scale_factor())?;
            geometry.bounds.origin += constrain_pan(pan, geometry.pan_limit);
            Some(geometry.bounds)
        },
        move |viewport, bounds, window, _| {
            let Some(image_bounds) = bounds else {
                return;
            };
            if let Err(error) =
                window.paint_image(image_bounds, Corners::default(), image, 0, false)
            {
                eprintln!("Could not paint image preview: {error}");
                return;
            }

            let visible = image_bounds.intersect(&viewport);
            if show_grid {
                paint_grid(window, image_bounds, visible);
            }
            if show_guides {
                let center = image_bounds.center();
                let color = rgba(0x69d4fff2);
                if center.x >= visible.left() && center.x < visible.right() {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(center.x, visible.top()),
                            size(px(1.), visible.size.height),
                        ),
                        color,
                    ));
                }
                if center.y >= visible.top() && center.y < visible.bottom() {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(visible.left(), center.y),
                            size(visible.size.width, px(1.)),
                        ),
                        color,
                    ));
                }
            }
        },
    )
    .size_full()
}

struct PreviewGeometry {
    bounds: Bounds<Pixels>,
    pan_limit: Point<Pixels>,
}

impl PreviewGeometry {
    fn new(
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
            Zoom::Percent(percent) => f32::from(percent) / 100. / scale,
        };
        if factor <= 0. || viewport.size.width <= px(0.) || viewport.size.height <= px(0.) {
            return None;
        }
        let bounds = Bounds::centered_at(viewport.center(), size(width * factor, height * factor));
        let pan_limit = point(
            ((bounds.size.width - viewport.size.width) / 2.).max(px(0.)),
            ((bounds.size.height - viewport.size.height) / 2.).max(px(0.)),
        );
        Some(Self { bounds, pan_limit })
    }
}

fn constrain_pan(pan: Point<Pixels>, limit: Point<Pixels>) -> Point<Pixels> {
    point(
        pan.x.clamp(-limit.x, limit.x),
        pan.y.clamp(-limit.y, limit.y),
    )
}

fn sample_color(
    tab: &Tab,
    viewport: Bounds<Pixels>,
    scale: f32,
    position: Point<Pixels>,
) -> Option<u32> {
    let geometry = PreviewGeometry::new(
        tab.document.width,
        tab.document.height,
        tab.zoom,
        viewport,
        scale,
    )?;
    let mut bounds = geometry.bounds;
    bounds.origin += constrain_pan(tab.pan, geometry.pan_limit);
    if !bounds.contains(&position) {
        return None;
    }

    let preview = &tab.document.preview;
    let dimensions = preview.size(0);
    let width = u32::from(dimensions.width) as usize;
    let height = u32::from(dimensions.height) as usize;
    let x =
        (((position.x - bounds.left()) / bounds.size.width * width as f32) as usize).min(width - 1);
    let y = (((position.y - bounds.top()) / bounds.size.height * height as f32) as usize)
        .min(height - 1);
    let offset = (y * width + x) * 4;
    let [blue, green, red, alpha] = preview.as_bytes(0)?.get(offset..offset + 4)? else {
        return None;
    };
    (*alpha != 0).then(|| u32::from_be_bytes([0, *red, *green, *blue]))
}

fn paint_grid(window: &mut Window, image: Bounds<Pixels>, visible: Bounds<Pixels>) {
    let spacing = px(32.);
    let color = rgba(0xd9e6ff55);
    let first_line = |origin: Pixels, edge: Pixels| {
        edge + px(f32::from(origin - edge).rem_euclid(f32::from(spacing)))
    };

    let mut x = first_line(image.left(), visible.left());
    while x < visible.right() {
        window.paint_quad(fill(
            Bounds::new(point(x, visible.top()), size(px(1.), visible.size.height)),
            color,
        ));
        x += spacing;
    }
    let mut y = first_line(image.top(), visible.top());
    while y < visible.bottom() {
        window.paint_quad(fill(
            Bounds::new(point(visible.left(), y), size(visible.size.width, px(1.))),
            color,
        ));
        y += spacing;
    }
}

fn dot_grid() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            let spacing = 24;
            let width = f32::from(bounds.size.width) as usize;
            let height = f32::from(bounds.size.height) as usize;
            for x in (spacing / 2..width).step_by(spacing) {
                for y in (spacing / 2..height).step_by(spacing) {
                    let origin = bounds.origin + point(px(x as f32), px(y as f32));
                    window.paint_quad(fill(
                        Bounds::new(origin, size(px(1.), px(1.))),
                        rgb(0x2b323d),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full()
}
