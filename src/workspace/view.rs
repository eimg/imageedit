use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, Corners, CursorStyle, Div, ExternalPaths,
    MouseButton, Pixels, Point, Transformation, TransformationMatrix, Window, canvas, div, fill,
    percentage, point, prelude::*, px, rgb, rgba, size, svg,
};

use crate::commands::{
    ApplyCrop, BackgroundColor, CancelCrop, CloseTab, ExportJpeg, ExportPng, ExportWebp,
    ForegroundColor, MinimizeWindow, NewImage, NextTab, OPEN_HINT, OpenImage, PreviousTab, Redo,
    ResetColors, SaveImage, SaveImageAs, SaveProjectAs, SelectTool, SwapColors, TOOL_GROUPS,
    ToggleGrid, ToggleGuides, Tool, Undo, Zoom, ZoomIn, ZoomOut, ZoomWindow,
};
use crate::document::{CropRect, FileFormat};

use super::canvas_tools::PreviewGeometry;
use super::cursor::CanvasCursor;
use super::dialogs::form_button;
use super::files::SaveMode;
use super::{Change, DEFAULT_COLORS, Tab, Workspace};

pub(super) const CHROME: u32 = 0x20242c;
pub(super) const PANEL: u32 = 0x191d24;
pub(super) const CANVAS: u32 = 0x11141a;
pub(super) const BORDER: u32 = 0x303640;
pub(super) const TEXT: u32 = 0xe9edf4;
pub(super) const MUTED: u32 = 0xa3adbd;
pub(super) const DISABLED: u32 = 0x767f8d;
pub(super) const HOVER: u32 = 0x2b323d;
pub(super) const ACCENT: u32 = 0xacc6ff;
pub(super) const SELECTED: u32 = 0x2c3a54;
const HEADER_HEIGHT: f32 = 36.;
const SWATCH_HEIGHT: f32 = 28.;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let interactive = self.form.is_none() && !self.dialog_open;
        let active = self.active_tab();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .track_focus(&self.focus)
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .when(interactive, |root| {
                let root = self.bind_layer_actions(root, cx);
                root.on_action(cx.listener(|this, action: &SelectTool, _, cx| {
                    if this.tool != action.0 {
                        this.tool = action.0;
                        this.pan_drag = None;
                        this.clear_crop();
                        cx.notify();
                    }
                }))
                .when(!self.creating, |root| {
                    root.on_action(
                        cx.listener(|this, _: &NewImage, window, cx| this.new_form(window, cx)),
                    )
                })
                .on_action(cx.listener(|this, _: &ForegroundColor, window, cx| {
                    this.color_form(0, window, cx)
                }))
                .on_action(cx.listener(|this, _: &BackgroundColor, window, cx| {
                    this.color_form(1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &SwapColors, _, cx| {
                    this.colors.swap(0, 1);
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &ResetColors, _, cx| {
                    this.colors = DEFAULT_COLORS;
                    cx.notify();
                }))
            })
            .when(
                interactive && active.is_some_and(|tab| !tab.busy()),
                |root| {
                    root.on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                        if let Some(index) = this.active {
                            this.request_close_tab(index, window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &SaveImage, window, cx| {
                        this.save(SaveMode::Current, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &SaveImageAs, window, cx| {
                        this.save(SaveMode::As, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &SaveProjectAs, window, cx| {
                        this.save(SaveMode::ProjectAs, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &ExportPng, window, cx| {
                        this.export(FileFormat::Png, 0, window, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &ExportJpeg, window, cx| this.jpeg_form(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &ExportWebp, window, cx| {
                        this.export(FileFormat::WebP, 0, window, cx)
                    }))
                    .when(self.tool == Tool::Crop, |root| {
                        root.key_context("Crop")
                            .when(self.crop.is_some() && self.crop_drag.is_none(), |root| {
                                root.on_action(cx.listener(|this, _: &ApplyCrop, window, cx| {
                                    this.apply_crop(window, cx)
                                }))
                            })
                            .on_action(cx.listener(|this, _: &CancelCrop, _, cx| {
                                this.clear_crop();
                                cx.notify();
                            }))
                    })
                },
            )
            .when(interactive && self.can_step_history(Change::Undo), |root| {
                root.on_action(cx.listener(|this, _: &Undo, window, cx| {
                    this.step_history(Change::Undo, window, cx)
                }))
            })
            .when(interactive && self.can_step_history(Change::Redo), |root| {
                root.on_action(cx.listener(|this, _: &Redo, window, cx| {
                    this.step_history(Change::Redo, window, cx)
                }))
            })
            .when(interactive && active.is_some(), |root| {
                root.on_action(cx.listener(|this, zoom: &Zoom, _, cx| {
                    if let Some(index) = this.active {
                        this.tabs[index].zoom = *zoom;
                        this.tabs[index].pan = Point::default();
                        this.pan_drag = None;
                        this.crop_drag = None;
                        cx.notify();
                    }
                }))
                .on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
                    this.step_zoom(1.25, None, window.scale_factor(), cx)
                }))
                .on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
                    this.step_zoom(0.8, None, window.scale_factor(), cx)
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
            .when(interactive && self.tabs.len() > 1, |root| {
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
                    .child(self.toolbox())
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
                            .child(self.canvas(interactive && window.is_window_hovered(), cx)),
                    )
                    .child(self.inspector(cx)),
            )
            .child(self.status_bar())
            .children(self.form_overlay(cx))
    }
}

impl Workspace {
    fn toolbox(&self) -> Div {
        div()
            .w(px(220.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(panel_header("Toolbox"))
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
            .child(self.color_box())
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

    fn color_box(&self) -> Div {
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
                    .child(
                        color_swatch("Foreground", foreground).on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ForegroundColor), cx);
                        }),
                    )
                    .child(
                        icon_button("swap")
                            .id("swap-colors")
                            .h(px(SWATCH_HEIGHT))
                            .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                            .cursor_pointer()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SwapColors), cx);
                            }),
                    )
                    .child(
                        color_swatch("Background", background).on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(BackgroundColor), cx);
                        }),
                    ),
            )
    }

    fn tab_strip(&self, cx: &Context<Self>) -> impl IntoElement {
        workspace_header()
            .id("tabs")
            .gap_1()
            .px_1()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .when(self.tabs.is_empty(), |strip| {
                strip.child(
                    div()
                        .px_2()
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
                    .h(px(28.))
                    .max_w(px(172.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .pr_1()
                    .rounded_md()
                    .when_else(
                        selected,
                        |tab| tab.bg(rgb(CHROME)),
                        |tab| tab.hover(|tab| tab.bg(rgb(CHROME))),
                    )
                    .child(
                        div()
                            .id("select")
                            .h_full()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .pl_2()
                            .pr_1()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(rgb(if selected { TEXT } else { MUTED }))
                            .child(icon("image", 12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .line_clamp(1)
                                    .child(tab.document.name.clone()),
                            )
                            .when(tab.dirty(), |label| {
                                label
                                    .child(div().flex_shrink_0().text_color(rgb(ACCENT)).child("•"))
                            })
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
                                    this.request_close_tab(index, window, cx);
                                }
                            })),
                    )
            }))
    }

    fn canvas(&self, show_tool_cursor: bool, cx: &Context<Self>) -> impl IntoElement {
        if !show_tool_cursor || self.active.is_none() {
            self.canvas_cursor.leave();
        }
        let show_tool_cursor = show_tool_cursor && self.pan_drag.is_none() && !cx.has_active_drag();
        if !show_tool_cursor {
            self.canvas_cursor.show_native(true);
        }
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
                    .on_scroll_wheel(cx.listener(
                        |this, event: &gpui::ScrollWheelEvent, window, cx| {
                            let delta = event.delta.pixel_delta(px(36.));
                            if event.modifiers.control || event.modifiers.secondary() {
                                let multiplier =
                                    1.25_f32.powf((f32::from(delta.y) / 36.).clamp(-3., 3.));
                                if delta.y != px(0.) {
                                    this.step_zoom(
                                        multiplier,
                                        Some(event.position),
                                        window.scale_factor(),
                                        cx,
                                    );
                                }
                            } else {
                                let delta = if event.modifiers.shift {
                                    point(delta.x + delta.y, px(0.))
                                } else {
                                    delta
                                };
                                this.pan_preview(delta, window.scale_factor(), cx);
                            }
                        },
                    ))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                            this.pan_drag = Some((event.button, event.position));
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(
                        cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                            this.canvas_cursor.position.set(Some(event.position));
                            window.refresh();
                            if let Some(button) = event.pressed_button {
                                this.drag_preview(
                                    button,
                                    event.position,
                                    window.scale_factor(),
                                    cx,
                                );
                                if button == MouseButton::Left {
                                    this.extend_crop(event.position, window.scale_factor(), cx);
                                }
                            }
                        }),
                    )
                    .capture_any_mouse_up(cx.listener(Self::end_canvas_drag))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::end_canvas_drag))
                    .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::end_canvas_drag))
                    .when(self.tool == Tool::Pan, |area| {
                        area.cursor(CursorStyle::OpenHand).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                this.pan_drag = Some((event.button, event.position));
                                cx.notify();
                            }),
                        )
                    })
                    .when(self.tool == Tool::Crop && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_crop(event.position, window.scale_factor(), cx);
                            }),
                        )
                    })
                    .when(self.tool == Tool::Eyedropper, |area| {
                        area.cursor(CursorStyle::Crosshair).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                this.pick_color(event.position(), window.scale_factor(), cx);
                            },
                        ))
                    })
                    .when(self.tool == Tool::Zoom, |area| {
                        area.cursor(CursorStyle::Crosshair)
                            .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                                let multiplier =
                                    if event.modifiers().control || event.modifiers().secondary() {
                                        0.8
                                    } else {
                                        1.25
                                    };
                                this.step_zoom(
                                    multiplier,
                                    Some(event.position()),
                                    window.scale_factor(),
                                    cx,
                                );
                            }))
                            .on_mouse_up(
                                MouseButton::Right,
                                cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
                                    this.step_zoom(
                                        0.8,
                                        Some(event.position),
                                        window.scale_factor(),
                                        cx,
                                    );
                                }),
                            )
                    })
                    .when(self.tool == Tool::BucketFill && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                this.fill_at(event.position(), window, cx);
                            },
                        ))
                    })
                    .when(self.pan_drag.is_some(), |area| {
                        area.cursor(CursorStyle::ClosedHand)
                    })
                    .child(image_preview(
                        tab,
                        self.viewport.clone(),
                        self.show_grid,
                        self.show_guides,
                        self.crop,
                    ))
                    .on_hover(cx.listener(|this, hovered, window, _| {
                        if *hovered {
                            this.canvas_cursor
                                .position
                                .set(Some(window.mouse_position()));
                        } else {
                            this.canvas_cursor.leave();
                        }
                        window.refresh();
                    }))
                    .when(show_tool_cursor, |area| {
                        area.when(!cfg!(target_os = "macos"), |area| {
                            area.cursor(CursorStyle::None)
                        })
                        .child(tool_cursor(self.tool, self.canvas_cursor.clone()))
                    }),
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
                                .child("Open or create an image"),
                        )
                        .child(
                            div()
                                .text_color(rgb(MUTED))
                                .child("Drop an ImageEdit project, PNG, JPEG, or WebP here."),
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
            .when(self.pending > 0, |area| {
                area.child(opening_indicator(self.pending))
            })
    }

    fn inspector(&self, cx: &Context<Self>) -> Div {
        let tool = self.tool.info();
        div()
            .w(px(232.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(BORDER))
            .child(panel_header("Tool Options"))
            .child(
                div()
                    .id("inspector-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_4()
                            .border_b_1()
                            .border_color(rgb(BORDER))
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
                            .when_some(self.crop, |panel, crop| {
                                panel
                                    .child(format!("{} × {} px", crop.width, crop.height))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(form_button("Apply Crop", true).on_click(
                                                |_, window, cx| {
                                                    window.dispatch_action(Box::new(ApplyCrop), cx)
                                                },
                                            ))
                                            .child(form_button("Cancel", false).on_click(
                                                |_, window, cx| {
                                                    window.dispatch_action(Box::new(CancelCrop), cx)
                                                },
                                            )),
                                    )
                            })
                            .when(
                                !matches!(
                                    self.tool,
                                    Tool::Pan
                                        | Tool::Zoom
                                        | Tool::Eyedropper
                                        | Tool::Crop
                                        | Tool::BucketFill
                                ),
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
                    .child(self.layers_panel(cx)),
            )
    }

    fn status_bar(&self) -> Div {
        let summary = if self.creating {
            "Creating image…".to_owned()
        } else {
            self.active_tab().map_or_else(
                || "Ready".to_owned(),
                |tab| {
                    let document = &tab.document;
                    let state = if tab.editing {
                        "  ·  Editing…".to_owned()
                    } else if tab.exporting {
                        "  ·  Exporting…".to_owned()
                    } else if tab.saving.is_some() {
                        "  ·  Saving…".to_owned()
                    } else if let Some(notice) = &tab.notice {
                        format!("  ·  {notice}")
                    } else {
                        String::new()
                    };
                    format!(
                        "{}  ·  {} × {} px{state}",
                        document.format.label(),
                        document.width(),
                        document.height()
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
            .child(div().flex_1().child(summary))
            .when_some(self.active_tab(), |bar, tab| {
                let zoom = match tab.zoom {
                    Zoom::Fit => "Fit".to_owned(),
                    Zoom::Percent(percent) if percent >= 10. => format!("{percent:.0}%"),
                    Zoom::Percent(percent) if percent >= 1. => format!("{percent:.1}%"),
                    Zoom::Percent(percent) if percent >= 0.005 => format!("{percent:.2}%"),
                    Zoom::Percent(_) => "<0.01%".to_owned(),
                };
                bar.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pr_4()
                        .child(zoom_button("zoom-out", "−").on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ZoomOut), cx)
                        }))
                        .child(zoom_button("zoom-fit", zoom).w(px(58.)).on_click(
                            |_, window, cx| window.dispatch_action(Box::new(Zoom::Fit), cx),
                        ))
                        .child(zoom_button("zoom-in", "+").on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ZoomIn), cx)
                        })),
                )
            })
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

fn workspace_header() -> Div {
    div()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(PANEL))
        .border_b_1()
        .border_color(rgb(BORDER))
}

fn panel_header(title: &'static str) -> Div {
    workspace_header()
        .px_4()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(title)
}

fn opening_indicator(pending: usize) -> Div {
    let label = if pending == 1 {
        "Opening image…".to_owned()
    } else {
        format!("Opening {pending} images…")
    };
    div()
        .absolute()
        .top_3()
        .right_3()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(CHROME))
        .border_1()
        .border_color(rgb(BORDER))
        .shadow_md()
        .text_size(px(12.))
        .child(
            svg()
                .path("loading")
                .size(px(14.))
                .text_color(rgb(ACCENT))
                .with_animation(
                    "opening-image",
                    Animation::new(Duration::from_millis(900)).repeat(),
                    |spinner, progress| {
                        spinner.with_transformation(Transformation::rotate(percentage(progress)))
                    },
                ),
        )
        .child(label)
}

fn open_image(_: &gpui::ClickEvent, window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(OpenImage), cx);
}

fn zoom_button(id: &'static str, label: impl Into<gpui::SharedString>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.))
        .h(px(22.))
        .rounded_sm()
        .cursor_pointer()
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(label.into())
}

fn color_swatch(label: &'static str, color: u32) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .cursor_pointer()
        .hover(|swatch| swatch.text_color(rgb(ACCENT)))
        .flex()
        .flex_1()
        .flex_col()
        .gap_1()
        .child(
            div()
                .h(px(SWATCH_HEIGHT))
                .rounded_sm()
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(color)),
        )
        .child(div().text_size(px(11.)).child(label))
        .child(
            div()
                .text_size(px(10.))
                .text_color(rgb(MUTED))
                .child(format!("#{color:06X}")),
        )
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

pub(super) fn section_label(text: &'static str) -> Div {
    div()
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(text)
}

#[derive(IntoElement)]
pub(super) struct Icon {
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

pub(super) fn icon(path: &'static str, edge: f32) -> Icon {
    Icon { path, edge }
}

fn tool_cursor(tool: Tool, cursor: Rc<CanvasCursor>) -> impl IntoElement {
    let icon = tool.info().icon;
    let outline = gpui::SharedString::from(format!("cursor-outline/{icon}"));
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let exit_cursor = cursor.clone();
            window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, window, _| {
                if phase == gpui::DispatchPhase::Capture {
                    exit_cursor.leave();
                    window.refresh();
                }
            });
            let hotspot = cursor.position.get().filter(|position| {
                bounds.contains(position) && window.is_window_hovered() && !cx.has_active_drag()
            });
            cursor.show_native(hotspot.is_none());
            let Some(hotspot) = hotspot else {
                return;
            };

            let horizontal = if hotspot.x + px(27.) < bounds.right() {
                7.
            } else {
                -27.
            };
            let vertical = if hotspot.y + px(27.) < bounds.bottom() {
                7.
            } else {
                -27.
            };
            let icon_bounds = Bounds::new(
                hotspot + point(px(horizontal), px(vertical)),
                size(px(20.), px(20.)),
            );
            for (path, color) in [
                (outline.clone(), rgb(0x12151c)),
                (icon.into(), rgb(0xffffff)),
            ] {
                let _ = window.paint_svg(
                    icon_bounds,
                    path,
                    TransformationMatrix::unit(),
                    color.into(),
                    cx,
                );
            }
            for (length, weight, color) in [(11., 3., rgb(0x12151c)), (9., 1., rgb(0xffffff))] {
                window.paint_quad(fill(
                    Bounds::centered_at(hotspot, size(px(length), px(weight))),
                    color,
                ));
                window.paint_quad(fill(
                    Bounds::centered_at(hotspot, size(px(weight), px(length))),
                    color,
                ));
            }
        },
    )
    .absolute()
    .size_full()
}

fn image_preview(
    tab: &Tab,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    show_grid: bool,
    show_guides: bool,
    crop: Option<CropRect>,
) -> impl IntoElement {
    let image = tab.document.canvas.preview.image.clone();
    let pixels = tab.document.canvas.pixels.clone();
    let has_transparency = tab.document.canvas.preview.has_transparency;
    let width = tab.document.width();
    let height = tab.document.height();
    let zoom = tab.zoom;
    let pan = tab.pan;

    canvas(
        move |bounds, window, _| {
            viewport.set(bounds);
            PreviewGeometry::new(width, height, zoom, bounds, window.scale_factor())
                .map(|geometry| geometry.panned(pan))
        },
        move |viewport, geometry, window, _| {
            let Some(geometry) = geometry else {
                return;
            };
            let image_bounds = geometry.bounds;
            let visible = image_bounds.intersect(&viewport);
            if visible.is_empty() {
                return;
            }
            if has_transparency {
                paint_checkerboard(window, image_bounds, visible);
            }
            let pixel_width = image_bounds.size.width / width as f32;
            let pixel_height = image_bounds.size.height / height as f32;
            if pixel_width.min(pixel_height) >= px(8.) {
                paint_source_pixels(window, &pixels, image_bounds, visible);
            } else if let Err(error) =
                window.paint_image(image_bounds, Corners::default(), image, 0, false)
            {
                eprintln!("Could not paint image preview: {error}");
                return;
            }

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
            if let Some(crop) = crop {
                let start = geometry.project(point(
                    crop.x as f32 / width as f32,
                    crop.y as f32 / height as f32,
                ));
                let end = geometry.project(point(
                    (crop.x + crop.width) as f32 / width as f32,
                    (crop.y + crop.height) as f32 / height as f32,
                ));
                paint_crop(window, image_bounds, Bounds::from_corners(start, end));
            }
        },
    )
    .size_full()
}

fn paint_source_pixels(
    window: &mut Window,
    pixels: &image::RgbaImage,
    image: Bounds<Pixels>,
    visible: Bounds<Pixels>,
) {
    // At high zoom, draw only the visible source pixels. This keeps the image
    // sharp and avoids sampling past the edges of the GPU preview texture.
    let step_x = image.size.width / pixels.width() as f32;
    let step_y = image.size.height / pixels.height() as f32;
    let x_start = ((visible.left() - image.left()) / step_x).floor() as u32;
    let y_start = ((visible.top() - image.top()) / step_y).floor() as u32;
    let x_end = (((visible.right() - image.left()) / step_x).ceil() as u32).min(pixels.width());
    let y_end = (((visible.bottom() - image.top()) / step_y).ceil() as u32).min(pixels.height());

    let scale = window.scale_factor();
    let snap = |edge: Pixels| px((f32::from(edge) * scale).round() / scale);
    for y in y_start..y_end {
        let top = snap(image.top() + step_y * y as f32);
        let bottom = snap(image.top() + step_y * (y + 1) as f32);
        for x in x_start..x_end {
            let pixel = pixels.get_pixel(x, y).0;
            if pixel[3] > 0 {
                let left = snap(image.left() + step_x * x as f32);
                let right = snap(image.left() + step_x * (x + 1) as f32);
                window.paint_quad(fill(
                    Bounds::from_corners(point(left, top), point(right, bottom)),
                    rgba(u32::from_be_bytes(pixel)),
                ));
            }
        }
    }
}

fn paint_checkerboard(window: &mut Window, image: Bounds<Pixels>, visible: Bounds<Pixels>) {
    const TILE: f32 = 12.;
    window.paint_quad(fill(visible, rgb(0x343944)));
    let tile_index = |offset: Pixels| (f32::from(offset) / TILE).floor() as i32;
    let left = tile_index(visible.left() - image.left());
    let top = tile_index(visible.top() - image.top());
    let right = tile_index(visible.right() - image.left());
    let bottom = tile_index(visible.bottom() - image.top());
    for y in top..=bottom {
        for x in left..=right {
            if (x + y) % 2 == 0 {
                let tile = Bounds::new(
                    image.origin + point(px(x as f32 * TILE), px(y as f32 * TILE)),
                    size(px(TILE), px(TILE)),
                )
                .intersect(&visible);
                if !tile.is_empty() {
                    window.paint_quad(fill(tile, rgb(0x474d5a)));
                }
            }
        }
    }
}

fn paint_crop(window: &mut Window, image: Bounds<Pixels>, crop: Bounds<Pixels>) {
    let shade = rgba(0x0b0d12a6);
    let bands = [
        Bounds::from_corners(image.origin, point(image.right(), crop.top())),
        Bounds::from_corners(point(image.left(), crop.bottom()), image.bottom_right()),
        Bounds::from_corners(point(image.left(), crop.top()), crop.bottom_left()),
        Bounds::from_corners(crop.top_right(), point(image.right(), crop.bottom())),
    ];
    for band in bands {
        if !band.is_empty() {
            window.paint_quad(fill(band, shade));
        }
    }
    window.paint_quad(gpui::outline(crop, rgb(ACCENT), gpui::BorderStyle::Solid));
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
