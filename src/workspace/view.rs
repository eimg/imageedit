use std::{
    cell::Cell,
    rc::Rc,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui::{
    Animation, AnimationExt as _, App, Bounds, ContentMask, Context, Corners, CursorStyle, Div,
    ExternalPaths, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, Transformation,
    TransformationMatrix, Window, canvas, div, fill, percentage, point, prelude::*, px, rgb, rgba,
    size, svg,
};

use crate::commands::{
    AlignLayer, ApplyCrop, ApplyGradient, ApplyTransform, BackgroundColor, BrushSizeDown,
    BrushSizeUp, CancelCrop, CancelFreeSelect, CancelGradient, CancelMove, CancelPath,
    CancelStroke, CancelTransform, CloseFreeSelect, ClosePath, CloseTab, DeletePathAnchor,
    ExportJpeg, ExportPng, ExportWebp, FeatherSelection, ForegroundColor, GrowSelection,
    InvertSelection, MinimizeWindow, NEW_HINT, NewImage, NextTab, OPEN_HINT, OpenImage,
    PreviousTab, Redo, ResetColors, SaveImage, SaveImageAs, SaveProjectAs, SelectNone, SelectTool,
    ShrinkSelection, SwapColors, TOOL_GROUPS, ToggleGrid, ToggleGuides, ToggleLayerBoundary, Tool,
    Undo, Zoom, ZoomIn, ZoomOut, ZoomWindow,
};
use crate::document::{CropRect, DrawnTile, FileFormat};
use crate::preview;
use crate::transform::{Mirror, Resample};

use super::canvas_tools::PreviewGeometry;
use super::cursor::CanvasCursor;
use super::files::SaveMode;
use super::tooltip::{tooltip, tooltip_with_detail};
use super::{Change, DEFAULT_COLORS, Tab, Workspace};

pub(super) const CHROME: u32 = 0x20242c;
pub(super) const PANEL: u32 = 0x191d24;
pub(super) const CANVAS: u32 = 0x11141a;
pub(super) const CHECKER_DARK: u32 = 0x8d95a6;
pub(super) const CHECKER_LIGHT: u32 = 0xd7dbe3;
pub(super) const BORDER: u32 = 0x303640;
pub(super) const TEXT: u32 = 0xe9edf4;
pub(super) const MUTED: u32 = 0xa3adbd;
pub(super) const DISABLED: u32 = 0x767f8d;
pub(super) const HOVER: u32 = 0x2b323d;
pub(super) const ACCENT: u32 = 0xacc6ff;
pub(super) const SELECTED: u32 = 0x2c3a54;
const HEADER_HEIGHT: f32 = 36.;
pub(super) const TOOLBOX_DEFAULT_WIDTH: f32 = 220.;
const TOOLBOX_MIN_WIDTH: f32 = 140.;
const TOOLBOX_MAX_WIDTH: f32 = 400.;
const TOOLBOX_FOLDED_WIDTH: f32 = 56.;
const TOOLBOX_FOLDED_PADDING: f32 = 8.;
const TOOLBOX_PADDING: f32 = 12.;
const TOOLBOX_GAP: f32 = 4.;
const TOOLBOX_SCROLLBAR_ALLOWANCE: f32 = 12.;
const TOOLBOX_MIN_BUTTON_WIDTH: f32 = 88.;
const TOOLBOX_MAX_COLUMNS: usize = 4;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.request_preview_level(window, cx);
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
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                this.begin_space_pan(event, cx);
            }))
            .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, cx| {
                this.end_space_pan(event, cx);
            }))
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .on_action(|_: &ZoomWindow, window, _| window.zoom_window())
            .when(interactive, |root| {
                let root = self.bind_layer_actions(root, cx);
                let root = self.bind_geometry_actions(root, cx);
                let root = self.bind_selection_actions(root, cx);
                let root = self.bind_clipboard_actions(root, cx);
                let root = self.bind_transform_actions(root, cx);
                let root = self.bind_adjustment_actions(root, cx);
                let root = self.bind_filter_actions(root, cx);
                let root = self.bind_path_actions(root, cx);
                let root = self.bind_stroke_actions(root, cx);
                let root = self.bind_text_actions(root, cx);
                root.on_action(cx.listener(|this, action: &SelectTool, window, cx| {
                    let tool = action.0;
                    let changed = this.tool != tool;
                    let was_path = this.tool == Tool::Path;
                    if changed && this.refuse_pending_text(cx) {
                        return;
                    }
                    if changed {
                        this.tool = tool;
                        this.pan_drag = None;
                        this.clear_selection_gesture();
                        this.clear_move_preview(window, cx);
                        this.clear_crop();
                        if tool == Tool::Path {
                            this.inspector_tab = super::InspectorTab::Paths;
                        } else if was_path {
                            this.end_path_session(cx);
                        }
                    }
                    if changed {
                        this.cancel_stroke(window, cx);
                        this.cancel_gradient(window, cx);
                        this.cancel_text(window, cx);
                        this.cancel_scale(window, cx);
                        this.cancel_path_gesture(cx);
                    }
                    if changed || matches!(tool, Tool::Rotate | Tool::Flip | Tool::Perspective) {
                        this.cancel_warp(window, cx);
                        this.begin_warp(window, cx);
                    }
                    cx.notify();
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
                .on_action(cx.listener(|this, _: &SwapColors, window, cx| {
                    this.colors.swap(0, 1);
                    this.refresh_gradient(window, cx);
                }))
                .on_action(cx.listener(|this, _: &ResetColors, window, cx| {
                    this.colors = DEFAULT_COLORS;
                    this.refresh_gradient(window, cx);
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
                    .when(self.tool == Tool::Move, |root| {
                        root.key_context("Move").on_action(cx.listener(
                            |this, _: &CancelMove, window, cx| this.cancel_move(window, cx),
                        ))
                    })
                    .when(
                        matches!(
                            self.tool,
                            Tool::Pencil
                                | Tool::Brush
                                | Tool::Eraser
                                | Tool::Blur
                                | Tool::Sharpen
                                | Tool::CloneStamp
                        ),
                        |root| {
                            root.key_context("Paint").on_action(cx.listener(
                                |this, _: &CancelStroke, window, cx| this.cancel_stroke(window, cx),
                            ))
                        },
                    )
                    .when(self.tool == Tool::Gradient, |root| {
                        root.key_context("Gradient")
                            .when(self.can_apply_gradient(), |root| {
                                root.on_action(cx.listener(
                                    |this, _: &ApplyGradient, window, cx| {
                                        this.apply_gradient(window, cx)
                                    },
                                ))
                            })
                            .on_action(cx.listener(|this, _: &CancelGradient, window, cx| {
                                this.cancel_gradient(window, cx);
                                cx.notify();
                            }))
                    })
                    .when(
                        matches!(self.tool, Tool::Rotate | Tool::Flip | Tool::Perspective),
                        |root| {
                            root.key_context("Transform")
                                .when(self.can_apply_warp(), |root| {
                                    root.on_action(cx.listener(
                                        |this, _: &ApplyTransform, window, cx| {
                                            this.apply_warp(window, cx)
                                        },
                                    ))
                                })
                                .on_action(cx.listener(|this, _: &CancelTransform, window, cx| {
                                    this.cancel_warp(window, cx);
                                    cx.notify();
                                }))
                        },
                    )
                    .on_action(cx.listener(|this, _: &FeatherSelection, window, cx| {
                        this.radius_form(super::selection::MaskEdit::Feather, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &GrowSelection, window, cx| {
                        this.radius_form(super::selection::MaskEdit::Grow, window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &ShrinkSelection, window, cx| {
                        this.radius_form(super::selection::MaskEdit::Shrink, window, cx);
                    }))
                    .when(!self.free_points.is_empty(), |root| {
                        root.key_context("FreeSelect")
                            .on_action(cx.listener(|this, _: &CancelFreeSelect, _, cx| {
                                this.cancel_free(cx);
                            }))
                            .when(self.free_points.len() >= 3, |root| {
                                root.on_action(cx.listener(
                                    |this, _: &CloseFreeSelect, window, cx| {
                                        this.close_free(window, cx);
                                    },
                                ))
                            })
                    })
                    .when(
                        self.tool == Tool::Text && self.text_draft.is_some(),
                        |root| {
                            root.key_context("Text").on_key_down(cx.listener(
                                |this, event: &gpui::KeyDownEvent, window, cx| {
                                    this.type_text(event, window, cx);
                                },
                            ))
                        },
                    )
                    .when(self.tool == Tool::Path, |root| {
                        root.key_context("Path")
                            .on_action(cx.listener(|this, _: &CancelPath, _, cx| {
                                this.end_path_session(cx);
                            }))
                            .on_action(cx.listener(|this, _: &ClosePath, window, cx| {
                                this.close_active_path(window, cx);
                            }))
                            .on_action(cx.listener(|this, _: &DeletePathAnchor, window, cx| {
                                this.delete_path_anchor(window, cx);
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
            .when(
                interactive
                    && matches!(
                        self.tool,
                        Tool::Pencil
                            | Tool::Brush
                            | Tool::Eraser
                            | Tool::Blur
                            | Tool::Sharpen
                            | Tool::CloneStamp
                            | Tool::Path
                    ),
                |root| {
                    root.on_action(cx.listener(|this, _: &BrushSizeDown, _, cx| {
                        this.adjust_brush_size(-1, cx);
                    }))
                    .on_action(cx.listener(
                        |this, _: &BrushSizeUp, _, cx| {
                            this.adjust_brush_size(1, cx);
                        },
                    ))
                },
            )
            .when(interactive && active.is_some(), |root| {
                root.on_action(cx.listener(|this, zoom: &Zoom, window, cx| {
                    if let Some(index) = this.active {
                        this.tabs[index].zoom = *zoom;
                        this.tabs[index].pan = Point::default();
                        this.pan_drag = None;
                        this.crop_drag = None;
                        this.ensure_preview_level(window.scale_factor(), window, cx);
                        cx.notify();
                    }
                }))
                .on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
                    this.step_zoom(1.25, None, window.scale_factor(), window, cx)
                }))
                .on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
                    this.step_zoom(0.8, None, window.scale_factor(), window, cx)
                }))
                .on_action(cx.listener(|this, _: &ToggleGrid, _, cx| {
                    this.show_grid = !this.show_grid;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &ToggleGuides, _, cx| {
                    this.show_guides = !this.show_guides;
                    cx.notify();
                }))
                .on_action(cx.listener(
                    |this, _: &ToggleLayerBoundary, _, cx| {
                        this.show_layer_bounds = !this.show_layer_bounds;
                        cx.notify();
                    },
                ))
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
                            .child(self.canvas(
                                interactive && window.is_window_hovered(),
                                window.scale_factor(),
                                cx,
                            )),
                    )
                    .child(self.inspector(cx)),
            )
            .child(self.status_bar())
            .children(self.form_overlay(cx))
    }
}

impl Workspace {
    fn brush_cursor(&self, tab: &Tab, scale: f32) -> Option<BrushCursor> {
        if !matches!(
            self.tool,
            Tool::Pencil
                | Tool::Brush
                | Tool::Eraser
                | Tool::Blur
                | Tool::Sharpen
                | Tool::CloneStamp
        ) {
            return None;
        }
        let geometry = PreviewGeometry::for_tab(tab, self.viewport.get(), scale)?;
        let canvas_width = tab.document.width().max(1) as f32;
        let scale = f32::from(geometry.bounds.size.width) / canvas_width;
        let (diameter, hardness) = self.active_brush_settings();
        Some(BrushCursor {
            tool: self.tool,
            diameter: diameter as f32 * scale,
            hardness,
            soft: self.tool != Tool::Pencil,
        })
    }

    fn toolbox(&self, cx: &Context<Self>) -> Div {
        let folded = self.toolbox_folded;
        let width = if folded {
            TOOLBOX_FOLDED_WIDTH
        } else {
            self.toolbox_width
        };
        let available =
            (self.toolbox_width - TOOLBOX_PADDING * 2. - TOOLBOX_SCROLLBAR_ALLOWANCE).max(0.);
        let columns = (((available + TOOLBOX_GAP) / (TOOLBOX_MIN_BUTTON_WIDTH + TOOLBOX_GAP))
            .floor() as usize)
            .clamp(1, TOOLBOX_MAX_COLUMNS);
        let button_width =
            ((available - TOOLBOX_GAP * (columns - 1) as f32) / columns as f32).max(0.);
        let folded_button =
            (TOOLBOX_FOLDED_WIDTH - TOOLBOX_FOLDED_PADDING * 2. - TOOLBOX_GAP).max(28.);
        let scroll = if folded {
            div()
                .id("toolbox-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py_2()
                .px(px(TOOLBOX_FOLDED_PADDING))
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .children(TOOL_GROUPS.iter().map(|(_, tools)| {
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .pb_2()
                        .children(
                            tools
                                .iter()
                                .copied()
                                .map(|tool| self.tool_button(tool, folded_button, true)),
                        )
                }))
        } else {
            div()
                .id("toolbox-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .children(TOOL_GROUPS.iter().map(|(label, tools)| {
                    div().pb_2().child(section_label(label).pb_1()).child(
                        div().flex().flex_wrap().gap_1().w_full().children(
                            tools
                                .iter()
                                .copied()
                                .map(|tool| self.tool_button(tool, button_width, false)),
                        ),
                    )
                }))
        };
        div()
            .flex_shrink_0()
            .flex()
            .child(
                div()
                    .id("toolbox")
                    .w(px(width))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .child(self.toolbox_header(cx))
                    .child(self.color_box(folded))
                    .child(scroll),
            )
            .when(!folded, |row| row.child(self.toolbox_resize_handle(cx)))
    }

    fn toolbox_header(&self, cx: &Context<Self>) -> Div {
        let folded = self.toolbox_folded;
        let toggle = div()
            .id("fold-toolbox")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded_sm()
            .cursor_pointer()
            .text_color(rgb(MUTED))
            .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
            .tooltip(tooltip(if folded {
                "Expand toolbox"
            } else {
                "Collapse toolbox"
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toolbox_folded = !this.toolbox_folded;
                this.toolbox_drag = None;
                cx.notify();
            }))
            .child(icon(
                if folded {
                    "sidebar-open"
                } else {
                    "sidebar-close"
                },
                14.,
            ));
        if folded {
            workspace_header().justify_center().child(toggle)
        } else {
            workspace_header()
                .justify_between()
                .px_4()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Toolbox")
                .child(toggle)
        }
    }

    fn tool_button(&self, tool: Tool, width: f32, folded: bool) -> impl IntoElement {
        let info = tool.info();
        let shortcut = tool.shortcut_label();
        let tooltip = match shortcut {
            Some(shortcut) => format!("{} ({shortcut})", info.label),
            None => info.label.to_owned(),
        };
        div()
            .id(info.icon)
            .w(px(width))
            .h(px(if folded { width } else { 43. }))
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
            .tooltip(tooltip_with_detail(tooltip, info.description))
            .child(icon(info.icon, if folded { 18. } else { 16. }))
            .when(!folded, |button| {
                button.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(3.))
                        .w_full()
                        .min_w_0()
                        .text_size(px(10.))
                        .whitespace_nowrap()
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(info.label),
                        )
                        .when_some(shortcut, |label, shortcut| {
                            label
                                .child(div().flex_shrink_0().text_color(rgb(MUTED)).child(shortcut))
                        }),
                )
            })
            .on_click(move |_, window, cx| window.dispatch_action(Box::new(SelectTool(tool)), cx))
    }

    fn toolbox_resize_handle(&self, cx: &Context<Self>) -> gpui::Stateful<Div> {
        let dragging = self.toolbox_drag.is_some();
        div()
            .id("toolbox-resize")
            .w(px(6.))
            .h_full()
            .flex_shrink_0()
            .cursor(CursorStyle::ResizeLeftRight)
            .when_else(
                dragging,
                |handle| handle.bg(rgb(ACCENT)),
                |handle| handle.hover(|handle| handle.bg(rgb(HOVER))),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.begin_toolbox_resize(event, cx);
                }),
            )
            .on_drag((), |_, _, _, cx| cx.new(|_| gpui::EmptyView))
            .on_drag_move::<()>(cx.listener(|this, event: &gpui::DragMoveEvent<()>, _, cx| {
                this.drag_toolbox(event.event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_toolbox_resize(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_toolbox_resize(cx)),
            )
    }

    fn begin_toolbox_resize(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.toolbox_drag = Some((f32::from(event.position.x), self.toolbox_width));
        cx.notify();
    }

    fn drag_toolbox(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((start_x, start_width)) = self.toolbox_drag else {
            return;
        };
        let width = (start_width + f32::from(position.x) - start_x)
            .clamp(TOOLBOX_MIN_WIDTH, TOOLBOX_MAX_WIDTH);
        if (width - self.toolbox_width).abs() > 0.5 {
            self.toolbox_width = width;
            cx.notify();
        }
    }

    fn end_toolbox_resize(&mut self, cx: &mut Context<Self>) {
        if self.toolbox_drag.take().is_some() {
            cx.notify();
        }
    }

    fn color_box(&self, folded: bool) -> Div {
        let [foreground, background] = self.colors;
        let panel = div().flex_shrink_0().border_b_1().border_color(rgb(BORDER));
        if folded {
            let swatch = (TOOLBOX_FOLDED_WIDTH - TOOLBOX_FOLDED_PADDING * 2.).min(36.);
            let flip = (swatch * 0.5).clamp(14., 18.);
            panel
                .p(px(TOOLBOX_FOLDED_PADDING))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .child(color_square("Foreground", foreground, swatch))
                .child(swap_button(flip))
                .child(color_square("Background", background, swatch))
        } else {
            let swatch = (self.toolbox_width * 0.18).clamp(26., 40.);
            let offset = (swatch * 0.5).round();
            let block = swatch + offset;
            let flip = (offset - 1.).max(12.);
            panel.p_3().flex().items_center().justify_center().child(
                div()
                    .relative()
                    .size(px(block))
                    .child(
                        color_square("Background", background, swatch)
                            .absolute()
                            .right_0()
                            .bottom_0(),
                    )
                    .child(swap_button(flip).absolute().right_0().top_0())
                    .child(
                        color_square("Foreground", foreground, swatch)
                            .absolute()
                            .left_0()
                            .top_0()
                            .shadow_sm(),
                    ),
            )
        }
    }

    fn selection_numeric_options(&self, cx: &Context<Self>) -> Div {
        let mut options = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_label("SHAPE"));
        if self.tool == Tool::Select {
            options = options.child(self.selection_radius_row(cx));
        }
        if self.tool == Tool::OvalSelect {
            options = options.child(toggle_button("Circle", self.oval_circle).on_click(
                cx.listener(|this, _, window, cx| {
                    let circle = !this.oval_circle;
                    this.set_oval_circle(circle, window, cx);
                }),
            ));
        }
        if self.selection_shape_rect().is_some() {
            options = options.child(self.selection_size_row());
        }
        options
    }

    fn selection_radius_row(&self, cx: &Context<Self>) -> Div {
        option_row(
            "Radius",
            stepper(
                self.selection_radius.to_string(),
                step_button("selection-radius-down", "−").on_click(
                    cx.listener(|this, _, window, cx| this.nudge_selection_radius(-1, window, cx)),
                ),
                step_button("selection-radius-up", "+").on_click(
                    cx.listener(|this, _, window, cx| this.nudge_selection_radius(1, window, cx)),
                ),
            ),
        )
    }

    fn selection_size_row(&self) -> Div {
        let Some(rect) = self.selection_shape_rect() else {
            return div();
        };
        div()
            .flex()
            .gap_2()
            .child(readout_chip("W", rect.width))
            .child(readout_chip("H", rect.height))
    }

    fn crop_fields(&self) -> Div {
        let Some(crop) = self.crop else {
            return div();
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(readout_chip("X", crop.x))
                    .child(readout_chip("Y", crop.y)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(readout_chip("W", crop.width))
                    .child(readout_chip("H", crop.height)),
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

    fn canvas(&self, show_tool_cursor: bool, scale: f32, cx: &Context<Self>) -> impl IntoElement {
        if !show_tool_cursor || self.active.is_none() {
            self.canvas_cursor.leave();
        }
        let show_tool_cursor =
            show_tool_cursor && self.pan_drag.is_none() && !self.space_pan && !cx.has_active_drag();
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
                                        window,
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
                                    this.extend_selection(
                                        event.position,
                                        window.scale_factor(),
                                        cx,
                                    );
                                    this.extend_move(event.position, window.scale_factor(), cx);
                                    this.extend_free(event.position, window.scale_factor(), cx);
                                    this.extend_scale(event.position, window.scale_factor(), cx);
                                    this.extend_warp_drag(
                                        event.position,
                                        window.scale_factor(),
                                        window,
                                        cx,
                                    );
                                    this.extend_gradient(
                                        event.position,
                                        window.scale_factor(),
                                        window,
                                        cx,
                                    );
                                    this.extend_path(event.position, window.scale_factor(), cx);
                                    this.extend_stroke(
                                        event.position,
                                        window.scale_factor(),
                                        window,
                                        cx,
                                    );
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
                    .when(
                        matches!(self.tool, Tool::Select | Tool::OvalSelect) && !tab.busy(),
                        |area| {
                            area.cursor(self.selection_cursor(scale)).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                    this.begin_selection(
                                        event.position,
                                        window.scale_factor(),
                                        super::selection::selection_op(&event.modifiers),
                                        cx,
                                    );
                                }),
                            )
                        },
                    )
                    .when(self.tool == Tool::Move && !tab.busy(), |area| {
                        area.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_move(event.position, window.scale_factor(), window, cx);
                            }),
                        )
                    })
                    .when(
                        matches!(self.tool, Tool::Rotate | Tool::Flip | Tool::Perspective)
                            && !tab.busy(),
                        |area| {
                            area.cursor(CursorStyle::Crosshair).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                    this.begin_warp_drag(
                                        event.position,
                                        window.scale_factor(),
                                        window,
                                        cx,
                                    );
                                }),
                            )
                        },
                    )
                    .when(
                        matches!(
                            self.tool,
                            Tool::Pencil
                                | Tool::Brush
                                | Tool::Eraser
                                | Tool::Blur
                                | Tool::Sharpen
                                | Tool::CloneStamp
                        ) && !tab.busy(),
                        |area| {
                            area.cursor(CursorStyle::Crosshair).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                    if this.tool == Tool::CloneStamp && event.modifiers.alt {
                                        this.set_clone_source(
                                            event.position,
                                            window.scale_factor(),
                                            cx,
                                        );
                                    } else {
                                        this.begin_stroke(
                                            event.position,
                                            event.modifiers.shift,
                                            window.scale_factor(),
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                            )
                        },
                    )
                    .when(self.tool == Tool::ScaleLayer && !tab.busy(), |area| {
                        area.cursor(self.scale_cursor(scale)).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_scale(event.position, window.scale_factor(), cx);
                            }),
                        )
                    })
                    .when(self.tool == Tool::Crop && !tab.busy(), |area| {
                        area.cursor(self.crop_cursor(scale)).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_crop(event.position, window.scale_factor(), cx);
                            }),
                        )
                    })
                    .when(self.tool == Tool::Eyedropper, |area| {
                        area.cursor(CursorStyle::Crosshair).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                this.pick_color(
                                    event.position(),
                                    window.scale_factor(),
                                    window,
                                    cx,
                                );
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
                                    window,
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
                                        window,
                                        cx,
                                    );
                                }),
                            )
                    })
                    .when(self.tool == Tool::FreeSelect && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_free(
                                    event.position,
                                    &event.modifiers,
                                    window.scale_factor(),
                                    window,
                                    cx,
                                );
                            }),
                        )
                    })
                    .when(self.tool == Tool::MagicWand && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                this.wand_at(
                                    event.position(),
                                    &event.modifiers(),
                                    window.scale_factor(),
                                    window,
                                    cx,
                                );
                            },
                        ))
                    })
                    .when(self.tool == Tool::Text && !tab.busy(), |area| {
                        area.cursor(CursorStyle::IBeam).on_click(cx.listener(
                            |this, event: &gpui::ClickEvent, window, cx| {
                                this.place_text(
                                    event.position(),
                                    window.scale_factor(),
                                    window,
                                    cx,
                                );
                            },
                        ))
                    })
                    .when(self.tool == Tool::Path && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_path(event.position, window.scale_factor(), cx);
                            }),
                        )
                    })
                    .when(self.tool == Tool::Gradient && !tab.busy(), |area| {
                        area.cursor(CursorStyle::Crosshair).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                this.begin_gradient(
                                    event.position,
                                    window.scale_factor(),
                                    window,
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
                    .when(self.space_pan && self.pan_drag.is_none(), |area| {
                        area.cursor(CursorStyle::OpenHand)
                    })
                    .when(self.space_pan, |area| {
                        area.capture_any_mouse_down(cx.listener(
                            |this, event: &gpui::MouseDownEvent, _, cx| {
                                if event.button != MouseButton::Left {
                                    return;
                                }
                                this.pan_drag = Some((event.button, event.position));
                                cx.stop_propagation();
                                cx.notify();
                            },
                        ))
                    })
                    .child(image_preview(
                        tab,
                        self.viewport.clone(),
                        CanvasDecorations {
                            show_grid: self.show_grid,
                            show_guides: self.show_guides,
                            crop: self.crop,
                            layer_bounds: if self.show_layer_bounds {
                                tab.document.content.layer(tab.active_layer).map(|layer| {
                                    let offset = self
                                        .move_drag
                                        .as_ref()
                                        .map_or(layer.offset, |drag| drag.offset);
                                    preview::PixelRect {
                                        x: i64::from(offset.0),
                                        y: i64::from(offset.1),
                                        width: layer.pixels.width(),
                                        height: layer.pixels.height(),
                                    }
                                })
                            } else {
                                None
                            },
                            scale_frame: self.scale_frame(),
                            selection: self
                                .selection_preview
                                .clone()
                                .unwrap_or_else(|| tab.selection.clone()),
                            show_handles: matches!(self.tool, Tool::Select | Tool::OvalSelect),
                            move_layer: self.move_paint(),
                            warp: self.warp_frame(),
                            gradient: self.gradient_frame(),
                            stroke: self.stroke_paint(),
                            free_path: self.free_points.clone(),
                            polygon: self.tool == Tool::FreeSelect,
                            paths: self.displayed_paths(),
                            active_path: self.displayed_active_path(),
                            path_handles: self.tool == Tool::Path,
                        },
                    ))
                    .on_modifiers_changed(cx.listener(
                        |_this, _: &gpui::ModifiersChangedEvent, window, cx| {
                            window.refresh();
                            cx.notify();
                        },
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
                        // The painted cursor hides the system pointer. A selection that can
                        // be moved or resized needs that pointer, so the hand and arrows show.
                        let native_cursor = (matches!(self.tool, Tool::Select | Tool::OvalSelect)
                            && self.selection_cursor(scale) != CursorStyle::Crosshair)
                            || (self.tool == Tool::Crop
                                && self.crop_cursor(scale) != CursorStyle::Crosshair)
                            || (self.tool == Tool::ScaleLayer
                                && self.scale_cursor(scale) != CursorStyle::Crosshair);
                        if native_cursor {
                            self.canvas_cursor.show_native(true);
                        }
                        area.when(!native_cursor && !cfg!(target_os = "macos"), |area| {
                            area.cursor(CursorStyle::None)
                        })
                        .when(!native_cursor, |area| {
                            let brush = self.brush_cursor(tab, scale);
                            area.child(tool_cursor(self.tool, self.canvas_cursor.clone(), brush))
                        })
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
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_2()
                                .mt_1()
                                .child(
                                    button("folder", "Choose Files…")
                                        .id("empty-open")
                                        .px_4()
                                        .h(px(36.))
                                        .bg(rgb(ACCENT))
                                        .text_color(rgb(CANVAS))
                                        .hover(|button| button.bg(rgb(0xc5d7ff)))
                                        .cursor_pointer()
                                        .on_click(open_image)
                                        .child(shortcut_hint(OPEN_HINT).opacity(0.65)),
                                )
                                .child(
                                    div()
                                        .id("empty-new")
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .h(px(30.))
                                        .px_3()
                                        .rounded_md()
                                        .text_color(rgb(MUTED))
                                        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                                        .cursor_pointer()
                                        .on_click(new_image)
                                        .child("New Image")
                                        .child(shortcut_hint(NEW_HINT).text_color(rgb(DISABLED))),
                                ),
                        ),
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
                                    .child(tool.label)
                                    .when_some(self.tool.shortcut_label(), |row, shortcut| {
                                        row.child(
                                            div()
                                                .text_size(px(12.))
                                                .font_weight(gpui::FontWeight::NORMAL)
                                                .text_color(rgb(MUTED))
                                                .child(shortcut),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_color(rgb(MUTED))
                                    .line_height(px(19.))
                                    .child(tool.description),
                            )
                            .when(
                                matches!(self.tool, Tool::Select | Tool::OvalSelect),
                                |panel| panel.child(self.selection_numeric_options(cx)),
                            )
                            .when(
                                matches!(
                                    self.tool,
                                    Tool::Select
                                        | Tool::OvalSelect
                                        | Tool::FreeSelect
                                        | Tool::MagicWand
                                ),
                                |panel| {
                                    let enabled = self.selection_command_enabled(
                                        super::selection::SelectionCommand::None,
                                    );
                                    panel.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .child(section_label("SELECTION"))
                                            .child(selection_actions(enabled)),
                                    )
                                },
                            )
                            .when(
                                matches!(self.tool, Tool::MagicWand | Tool::BucketFill),
                                |panel| {
                                    panel
                                        .child(section_label("MATCH"))
                                        .child(tolerance_row(self.tolerance, cx))
                                },
                            )
                            .when(self.tool == Tool::BucketFill, |panel| {
                                let mode = self.fill_mode;
                                let sample_all = self.sample_all;
                                panel
                                    .child(section_label("FILL"))
                                    .child(segmented([
                                        fill_mode_button(
                                            "Connected",
                                            mode == crate::document::FillMode::Connected,
                                            crate::document::FillMode::Connected,
                                            cx,
                                        ),
                                        fill_mode_button(
                                            "Selection",
                                            mode == crate::document::FillMode::Selection,
                                            crate::document::FillMode::Selection,
                                            cx,
                                        ),
                                    ]))
                                    .child(
                                        toggle_button("Sample All Layers", sample_all).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.set_sample_all(!sample_all, cx);
                                            }),
                                        ),
                                    )
                            })
                            .when(self.tool == Tool::Text, |panel| self.text_options(panel, cx))
                            .when(self.tool == Tool::Path, |panel| {
                                panel.child(info_note(
                                    "Path management and Stroke are in the Paths tab.",
                                ))
                            })
                            .when(self.tool == Tool::Gradient, |panel| {
                                let shape = self.gradient_shape;
                                let blend = self.gradient_blend;
                                let can_apply = self.can_apply_gradient();
                                panel
                                    .child(section_label("GRADIENT"))
                                    .child(segmented([
                                        gradient_shape_button(
                                            "Linear",
                                            shape == crate::paint::GradientShape::Linear,
                                            crate::paint::GradientShape::Linear,
                                            cx,
                                        ),
                                        gradient_shape_button(
                                            "Radial",
                                            shape == crate::paint::GradientShape::Radial,
                                            crate::paint::GradientShape::Radial,
                                            cx,
                                        ),
                                    ]))
                                    .child(segmented([
                                        gradient_blend_button(
                                            "FG → BG",
                                            blend
                                                == crate::paint::GradientBlend::ForegroundToBackground,
                                            crate::paint::GradientBlend::ForegroundToBackground,
                                            cx,
                                        ),
                                        gradient_blend_button(
                                            "FG → Clear",
                                            blend
                                                == crate::paint::GradientBlend::ForegroundToTransparent,
                                            crate::paint::GradientBlend::ForegroundToTransparent,
                                            cx,
                                        )
                                        .tooltip(tooltip("Foreground to Transparent")),
                                    ]))
                                    .when(self.gradient.is_some(), |panel| {
                                        panel.child(action_row([
                                            option_action("Apply", true)
                                                .when(can_apply, |button| {
                                                    button.on_click(|_, window, cx| {
                                                        window.dispatch_action(
                                                            Box::new(ApplyGradient),
                                                            cx,
                                                        )
                                                    })
                                                })
                                                .when(!can_apply, |button| {
                                                    button
                                                        .opacity(0.45)
                                                        .cursor(CursorStyle::Arrow)
                                                }),
                                            option_action("Cancel", false).on_click(|_, window, cx| {
                                                window.dispatch_action(
                                                    Box::new(CancelGradient),
                                                    cx,
                                                )
                                            }),
                                        ]))
                                    })
                            })
                            .when_some(self.crop, |panel, _| {
                                panel
                                    .child(section_label("CROP"))
                                    .child(self.crop_fields())
                                    .child(info_note(
                                        "Drag inside to move. Drag a handle to resize.",
                                    ))
                                    .child(action_row([
                                        option_action("Apply Crop", true).on_click(
                                            |_, window, cx| {
                                                window.dispatch_action(Box::new(ApplyCrop), cx)
                                            },
                                        ),
                                        option_action("Cancel", false).on_click(
                                            |_, window, cx| {
                                                window.dispatch_action(Box::new(CancelCrop), cx)
                                            },
                                        ),
                                    ]))
                            })
                            .when(self.tool == Tool::Move, |panel| {
                                let enabled = self.geometry_enabled();
                                let offset =
                                    self.move_drag.as_ref().map(|drag| drag.offset).or_else(|| {
                                        self.active_tab().and_then(|tab| {
                                            tab.document
                                                .content
                                                .layer(tab.active_layer)
                                                .map(|layer| layer.offset)
                                        })
                                    });
                                panel
                                    .when_some(offset, |panel, offset| {
                                        panel
                                            .child(section_label("POSITION"))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap_2()
                                                    .child(readout_chip("X", offset.0))
                                                    .child(readout_chip("Y", offset.1)),
                                            )
                                            .child(section_label("ALIGN"))
                                            .child(align_controls(enabled, cx))
                                    })
                                    .when(self.move_drag.is_some(), |panel| {
                                        panel.child(
                                            option_action("Cancel Move", false).w_full().on_click(
                                                |_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(CancelMove),
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
                                    })
                            })
                            .when(self.tool == Tool::ScaleLayer, |panel| {
                                let size = self.scale_size();
                                panel
                                    .child(info_note(
                                        "Drag a border or corner to scale the active layer.",
                                    ))
                                    .when_some(size, |panel, (width, height)| {
                                        panel
                                            .child(section_label("SIZE"))
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap_2()
                                                    .child(readout_chip("W", width))
                                                    .child(readout_chip("H", height)),
                                            )
                                    })
                            })
                            .when_some(self.warp_status(), |panel, status| {
                                let horizontal = status.horizontal;
                                let resample = status.resample;
                                let show_resample = self.tool != Tool::Flip;
                                let can_apply = self.can_apply_warp();
                                panel
                                    .child(if status.applying {
                                        info_note("Applying…")
                                    } else {
                                        info_note(status.summary)
                                    })
                                    .when_some(status.error, |panel, error| {
                                        panel.child(div().text_color(rgb(0xffb4ab)).child(error))
                                    })
                                    .when(self.tool == Tool::Flip, |panel| {
                                        panel
                                            .child(section_label("MIRROR"))
                                            .child(segmented([
                                                warp_choice(
                                                    "Horizontal",
                                                    horizontal,
                                                    Mirror::Horizontal,
                                                    cx,
                                                ),
                                                warp_choice(
                                                    "Vertical",
                                                    !horizontal,
                                                    Mirror::Vertical,
                                                    cx,
                                                ),
                                            ]))
                                    })
                                    .when(show_resample, |panel| {
                                        panel
                                            .child(section_label("RESAMPLE"))
                                            .child(segmented([
                                                resample_choice(
                                                    "Nearest",
                                                    Resample::Nearest,
                                                    resample,
                                                    cx,
                                                ),
                                                resample_choice(
                                                    "Bilinear",
                                                    Resample::Bilinear,
                                                    resample,
                                                    cx,
                                                ),
                                                resample_choice(
                                                    "Bicubic",
                                                    Resample::Bicubic,
                                                    resample,
                                                    cx,
                                                ),
                                            ]))
                                    })
                                    .child(action_row([
                                        option_action("Apply", true)
                                            .when(can_apply, |button| {
                                                button.on_click(|_, window, cx| {
                                                    window.dispatch_action(
                                                        Box::new(ApplyTransform),
                                                        cx,
                                                    );
                                                })
                                            })
                                            .when(!can_apply, |button| {
                                                button
                                                    .opacity(0.45)
                                                    .cursor(CursorStyle::Arrow)
                                            }),
                                        option_action("Cancel", false).on_click(|_, window, cx| {
                                            window.dispatch_action(Box::new(CancelTransform), cx)
                                        }),
                                    ]))
                            })
                            .when(
                                matches!(
                                    self.tool,
                                    Tool::Pencil
                                        | Tool::Brush
                                        | Tool::Eraser
                                        | Tool::Blur
                                        | Tool::Sharpen
                                        | Tool::CloneStamp
                                ),
                                |panel| {
                                    panel
                                        .when(self.tool == Tool::CloneStamp, |panel| {
                                            panel.child(if self.clone_source.is_some() {
                                                info_note("Clone source set. Alt-click to change it.")
                                            } else {
                                                info_note("Alt-click to set the clone source.")
                                            })
                                        })
                                        .child(section_label("BRUSH"))
                                        .child(brush_row(
                                            "Size",
                                            self.brush_value(super::paint_ui::BrushField::Size),
                                            super::paint_ui::BrushField::Size,
                                            cx,
                                        ))
                                        .child(brush_row(
                                            "Spacing",
                                            self.brush_value(super::paint_ui::BrushField::Spacing),
                                            super::paint_ui::BrushField::Spacing,
                                            cx,
                                        ))
                                        .when(self.tool != Tool::Pencil, |panel| {
                                            panel.child(brush_row(
                                                "Hardness",
                                                self.brush_value(
                                                    super::paint_ui::BrushField::Hardness,
                                                ),
                                                super::paint_ui::BrushField::Hardness,
                                                cx,
                                            ))
                                        })
                                        .child(brush_row(
                                            "Opacity",
                                            self.brush_value(super::paint_ui::BrushField::Opacity),
                                            super::paint_ui::BrushField::Opacity,
                                            cx,
                                        ))
                                },
                            )
                            .when(
                                !matches!(
                                    self.tool,
                                    Tool::Pan
                                        | Tool::Zoom
                                        | Tool::Eyedropper
                                        | Tool::Crop
                                        | Tool::BucketFill
                                        | Tool::Select
                                        | Tool::OvalSelect
                                        | Tool::FreeSelect
                                        | Tool::MagicWand
                                        | Tool::Move
                                        | Tool::Rotate
                                        | Tool::Flip
                                        | Tool::Perspective
                                        | Tool::Pencil
                                        | Tool::Brush
                                        | Tool::Eraser
                                        | Tool::Blur
                                        | Tool::Sharpen
                                        | Tool::Gradient
                                        | Tool::Path
                                        | Tool::Text
                                ),
                                |panel| panel.child(info_note("Planned tool")),
                            ),
                    )
                    .child(self.inspector_tabs(cx)),
            )
    }

    fn inspector_tabs(&self, cx: &Context<Self>) -> Div {
        let selected = self.inspector_tab;
        let tab = |label: &'static str, target: super::InspectorTab, cx: &Context<Self>| {
            let active = selected == target;
            div()
                .id(label)
                .h(px(28.))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_size(px(12.))
                .when(active, |tab| {
                    tab.bg(rgb(SELECTED))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT))
                })
                .when(!active, |tab| {
                    tab.text_color(rgb(MUTED))
                        .hover(|tab| tab.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.inspector_tab != target {
                        this.inspector_tab = target;
                        cx.notify();
                    }
                }))
                .child(label)
        };
        let content = match selected {
            super::InspectorTab::Layers => self.layers_panel(cx),
            super::InspectorTab::Paths => self.path_panel(cx),
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .p_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(tab("Layers", super::InspectorTab::Layers, cx))
                    .child(tab("Paths", super::InspectorTab::Paths, cx)),
            )
            .child(
                div()
                    .id(("inspector-tab-scroll", selected as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
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
                    let selection = match &tab.selection {
                        crate::selection::Selection::Unrestricted => String::new(),
                        crate::selection::Selection::Empty => "  ·  Empty selection".to_owned(),
                        crate::selection::Selection::Full => "  ·  All pixels".to_owned(),
                        crate::selection::Selection::Rect(rect) => {
                            format!("  ·  Selection {} × {}", rect.width, rect.height)
                        }
                        crate::selection::Selection::Ellipse(ellipse) => {
                            let rect = ellipse.rect();
                            format!("  ·  Selection {} × {}", rect.width, rect.height)
                        }
                        crate::selection::Selection::RoundedRect(shape) => {
                            let rect = shape.rect();
                            format!("  ·  Selection {} × {}", rect.width, rect.height)
                        }
                        crate::selection::Selection::Inverse(_) => {
                            "  ·  Inverted selection".to_owned()
                        }
                        crate::selection::Selection::Mask(_) => "  ·  Custom selection".to_owned(),
                    };
                    format!(
                        "{}  ·  {} × {} px{selection}{state}",
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

fn new_image(_: &gpui::ClickEvent, window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(NewImage), cx);
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

fn color_square(label: &'static str, color: u32, size: f32) -> gpui::Stateful<Div> {
    let foreground = label == "Foreground";
    div()
        .id(label)
        .size(px(size))
        .rounded_sm()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(color))
        .cursor_pointer()
        .hover(|swatch| swatch.border_color(rgb(ACCENT)))
        .tooltip(tooltip(if foreground {
            "Foreground color"
        } else {
            "Background color"
        }))
        .on_click(move |_, window, cx| {
            if foreground {
                window.dispatch_action(Box::new(ForegroundColor), cx);
            } else {
                window.dispatch_action(Box::new(BackgroundColor), cx);
            }
        })
}

fn swap_button(size: f32) -> gpui::Stateful<Div> {
    div()
        .id("swap-colors")
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .text_color(rgb(MUTED))
        .tooltip(tooltip("Swap foreground and background"))
        .hover(|button| button.text_color(rgb(ACCENT)))
        .cursor_pointer()
        .on_click(|_, window, cx| {
            window.dispatch_action(Box::new(SwapColors), cx);
        })
        .child(icon("swap", (size * 0.65).clamp(9., 14.)))
}

fn shortcut_hint(shortcut: &'static str) -> Div {
    div().text_size(px(12.)).child(shortcut)
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

const OPTION_HEIGHT: f32 = 28.;
const STEP_HEIGHT: f32 = 26.;

pub(super) fn info_note(text: impl Into<gpui::SharedString>) -> Div {
    div()
        .flex()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CANVAS))
        .p_2()
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(rgb(MUTED))
        .child(icon("info", 14.))
        .child(div().flex_1().min_w_0().child(text.into()))
}

pub(super) fn option_row(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(12.))
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(control)
}

pub(super) fn segmented(children: impl IntoIterator<Item = gpui::Stateful<Div>>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .p_1()
        .rounded_md()
        .bg(rgb(CANVAS))
        .border_1()
        .border_color(rgb(BORDER))
        .children(children)
}

pub(super) fn choice_button(label: &'static str, selected: bool) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .h(px(24.))
        .flex_1()
        .min_w_0()
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .text_size(px(11.))
        .font_weight(if selected {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::NORMAL
        })
        .when(selected, |button| {
            button.bg(rgb(SELECTED)).text_color(rgb(TEXT))
        })
        .when(!selected, |button| {
            button
                .text_color(rgb(MUTED))
                .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        })
        .cursor_pointer()
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(label),
        )
}

pub(super) fn toggle_button(label: &'static str, selected: bool) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .h(px(OPTION_HEIGHT))
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if selected { ACCENT } else { BORDER }))
        .bg(rgb(if selected { SELECTED } else { CANVAS }))
        .text_size(px(12.))
        .text_color(rgb(if selected { TEXT } else { MUTED }))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(
            div()
                .size(px(14.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .border_1()
                .border_color(rgb(if selected { ACCENT } else { BORDER }))
                .when(selected, |mark| mark.bg(rgb(ACCENT)).text_color(rgb(PANEL)))
                .child(if selected { "✓" } else { "" }),
        )
        .child(label)
}

pub(super) fn step_button(
    id: impl Into<gpui::ElementId>,
    symbol: &'static str,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .size(px(STEP_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .rounded_sm()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(CHROME))
        .text_color(rgb(MUTED))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(symbol)
}

pub(super) fn stepper(
    value: impl Into<gpui::SharedString>,
    down: gpui::Stateful<Div>,
    up: gpui::Stateful<Div>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(down)
        .child(
            div()
                .min_w(px(48.))
                .h(px(STEP_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .bg(rgb(CANVAS))
                .border_1()
                .border_color(rgb(BORDER))
                .text_size(px(12.))
                .child(value.into()),
        )
        .child(up)
}

pub(super) fn readout_chip(label: &'static str, value: impl ToString) -> Div {
    div()
        .h(px(STEP_HEIGHT))
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .justify_between()
        .gap_1()
        .px_2()
        .rounded_md()
        .bg(rgb(CANVAS))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(div().text_size(px(12.)).child(value.to_string()))
}

pub(super) fn option_action(label: &'static str, primary: bool) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .h(px(OPTION_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .px_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if primary { ACCENT } else { BORDER }))
        .bg(rgb(if primary { SELECTED } else { CHROME }))
        .text_size(px(12.))
        .text_color(rgb(if primary { TEXT } else { MUTED }))
        .cursor_pointer()
        .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
        .child(label)
}

pub(super) fn action_row(children: impl IntoIterator<Item = gpui::Stateful<Div>>) -> Div {
    div()
        .flex()
        .gap_2()
        .children(children.into_iter().map(|child| child.flex_1()))
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

#[derive(Clone, Copy)]
struct BrushCursor {
    tool: Tool,
    diameter: f32,
    hardness: u8,
    soft: bool,
}

fn tool_cursor(
    tool: Tool,
    cursor: Rc<CanvasCursor>,
    brush: Option<BrushCursor>,
) -> impl IntoElement {
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
            if let Some(brush) = brush {
                paint_brush_cursor(window, hotspot, brush);
                return;
            }

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
            let (shadow_bounds, shadow_path) = if tool == Tool::Move {
                // A small shadow keeps the arrowheads readable at badge size.
                (
                    Bounds::new(icon_bounds.origin + point(px(1.), px(1.)), icon_bounds.size),
                    icon.into(),
                )
            } else {
                (icon_bounds, outline.clone())
            };
            for (bounds, path, color) in [
                (shadow_bounds, shadow_path, rgb(0x12151c)),
                (icon_bounds, icon.into(), rgb(0xffffff)),
            ] {
                let _ =
                    window.paint_svg(bounds, path, TransformationMatrix::unit(), color.into(), cx);
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

fn paint_brush_cursor(window: &mut Window, center: Point<Pixels>, brush: BrushCursor) {
    let radius = (brush.diameter / 2.).max(1.5);
    let ring = |window: &mut Window, radius: f32, weight: f32, color: gpui::Rgba, dashed: bool| {
        let segments = (radius * 3.).clamp(24., 160.) as usize;
        let mut path = PathBuilder::stroke(px(weight));
        if dashed {
            path = path.dash_array(&[px(2.), px(2.)]);
        }
        let mut points = Vec::with_capacity(segments);
        for index in 0..segments {
            let angle = index as f32 / segments as f32 * std::f32::consts::TAU;
            points.push(center + point(px(angle.cos() * radius), px(angle.sin() * radius)));
        }
        path.add_polygon(&points, true);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    if brush.tool == Tool::CloneStamp && window.modifiers().alt {
        ring(window, radius, 2., rgb(0x12151c), true);
        ring(window, radius, 1., rgba(0xacc6ffff), true);
        for (length, weight, color) in [(7., 2., rgb(0x12151c)), (5., 1., rgba(0xacc6ffff))] {
            window.paint_quad(fill(
                Bounds::centered_at(center, size(px(length), px(weight))),
                color,
            ));
            window.paint_quad(fill(
                Bounds::centered_at(center, size(px(weight), px(length))),
                color,
            ));
        }
        return;
    }
    ring(window, radius, 2., rgb(0x12151c), false);
    ring(window, radius, 1., rgb(0xffffff), false);
    if brush.soft && brush.hardness < 100 {
        let core = radius * f32::from(brush.hardness) / 100.;
        if core >= 2. {
            ring(window, core, 2., rgba(0x12151caa), true);
            ring(window, core, 1., rgba(0xffffffaa), true);
        }
    }
}

struct CanvasDecorations {
    show_grid: bool,
    show_guides: bool,
    crop: Option<CropRect>,
    layer_bounds: Option<preview::PixelRect>,
    scale_frame: Option<preview::PixelRect>,
    selection: crate::selection::Selection,
    show_handles: bool,
    move_layer: Option<super::layers::MovePaint>,
    warp: Option<super::transform_ui::WarpFrame>,
    gradient: Option<super::gradient_ui::GradientFrame>,
    stroke: Vec<super::paint_ui::StrokePaint>,
    free_path: Vec<Point<f32>>,
    polygon: bool,
    paths: Vec<crate::paths::VectorPath>,
    active_path: Option<crate::paths::PathId>,
    path_handles: bool,
}

fn image_preview(
    tab: &Tab,
    viewport: Rc<Cell<Bounds<Pixels>>>,
    decorations: CanvasDecorations,
) -> impl IntoElement {
    let CanvasDecorations {
        show_grid,
        show_guides,
        crop,
        layer_bounds,
        scale_frame,
        selection,
        show_handles,
        move_layer,
        warp,
        gradient,
        stroke,
        free_path,
        polygon,
        paths,
        active_path,
        path_handles,
    } = decorations;
    let image = tab.document.canvas.preview.image.clone();
    let tile_sets = tab.document.canvas.tile_sets();
    let pixels = tab.document.canvas.pixels.clone();
    let has_transparency = tab.document.canvas.preview.has_transparency;
    let width = tab.document.width();
    let height = tab.document.height();
    let zoom = tab.zoom;
    let pan = tab.pan;
    let contours = selection.contours(width, height);
    let handle_rect = show_handles
        .then(|| super::selection::adjustable_rect(selection.clone()))
        .flatten();

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
            if has_transparency || move_layer.is_some() {
                paint_checkerboard(window, image_bounds, visible);
            }
            if let Some(move_layer) = move_layer.clone() {
                paint_moved_layer(window, &geometry, width, height, &move_layer);
            } else {
                let pixel_width = image_bounds.size.width / width as f32;
                let pixel_height = image_bounds.size.height / height as f32;
                let show_pixels = pixel_width.min(pixel_height) >= px(8.);
                if show_pixels {
                    paint_source_pixels(window, &pixels, image_bounds, visible);
                } else if !paint_tiles(
                    window,
                    &geometry,
                    width,
                    height,
                    window.scale_factor(),
                    visible,
                    &tile_sets,
                ) && let Err(error) =
                    window.paint_image(image_bounds, Corners::default(), image.clone(), 0, false)
                {
                    eprintln!("Could not paint image preview: {error}");
                }
                for patch in &stroke {
                    paint_stroke_patch(
                        window,
                        &geometry,
                        (width, height),
                        visible,
                        patch,
                        show_pixels,
                    );
                }
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
            if let Some(bounds) = layer_bounds {
                paint_layer_bounds(window, &geometry, width, height, visible, bounds);
            }
            paint_paths(
                window,
                &geometry,
                viewport,
                (width, height),
                &paths,
                active_path,
            );
            paint_contours(window, &geometry, width, height, visible, &contours);
            paint_segments(
                window,
                &geometry,
                width,
                height,
                visible,
                selection.outline_edges(),
            );
            paint_free_outline(window, &geometry, viewport, &free_path, polygon);
            if path_handles {
                paint_path_handles(
                    window,
                    &geometry,
                    viewport,
                    (width, height),
                    &paths,
                    active_path,
                );
            }
            if let Some(rect) = handle_rect {
                paint_handles(window, &geometry, width, height, rect);
            }
            if let Some(frame) = scale_frame {
                paint_scale_frame(window, &geometry, width, height, visible, frame);
            }
            if let Some(frame) = warp {
                paint_warp_frame(window, &geometry, viewport, width, height, frame);
            }
            if let Some(frame) = gradient {
                paint_gradient_frame(window, &geometry, viewport, width, height, frame);
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
                paint_handles(
                    window,
                    &geometry,
                    width,
                    height,
                    preview::PixelRect {
                        x: i64::from(crop.x),
                        y: i64::from(crop.y),
                        width: crop.width,
                        height: crop.height,
                    },
                );
            }
        },
    )
    .size_full()
}

fn paint_tiles(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    scale: f32,
    visible: Bounds<Pixels>,
    tile_sets: &[(u32, Vec<DrawnTile>)],
) -> bool {
    let display = f32::from(geometry.bounds.size.width)
        .max(f32::from(geometry.bounds.size.height))
        .max(1.)
        * scale;
    let wanted = preview::fit_level_shift(width.max(height), display as u32);
    let Some((_, tiles)) = tile_sets
        .iter()
        .find(|(shift, _)| *shift == wanted)
        .or_else(|| tile_sets.first())
    else {
        return false;
    };
    if tiles.is_empty() {
        return false;
    }
    let canvas_bounds = |rect: preview::PixelRect| {
        let start = geometry.project(point(
            rect.x as f32 / width as f32,
            rect.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            rect.right() as f32 / width as f32,
            rect.bottom() as f32 / height as f32,
        ));
        Bounds::from_corners(start, end)
    };
    for tile in tiles {
        let content = canvas_bounds(tile.content);
        if content.intersect(&visible).is_empty() {
            continue;
        }
        let stored = canvas_bounds(tile.stored);
        window.with_content_mask(Some(ContentMask { bounds: content }), |window| {
            if let Err(error) =
                window.paint_image(stored, Corners::default(), tile.image.clone(), 0, false)
            {
                eprintln!("Could not paint image tile: {error}");
            }
        });
    }
    true
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
    window.paint_quad(fill(visible, rgb(CHECKER_DARK)));
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
                    window.paint_quad(fill(tile, rgb(CHECKER_LIGHT)));
                }
            }
        }
    }
}

fn selection_actions(enabled: bool) -> Div {
    action_row([
        selection_button("Deselect", enabled, |window, cx| {
            window.dispatch_action(Box::new(SelectNone), cx);
        }),
        selection_button("Invert", enabled, |window, cx| {
            window.dispatch_action(Box::new(InvertSelection), cx);
        }),
    ])
}

fn fill_mode_button(
    label: &'static str,
    selected: bool,
    mode: crate::document::FillMode,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    choice_button(label, selected).on_click(cx.listener(move |this, _, _, cx| {
        this.set_fill_mode(mode, cx);
    }))
}

fn gradient_shape_button(
    label: &'static str,
    selected: bool,
    shape: crate::paint::GradientShape,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    choice_button(label, selected).on_click(cx.listener(move |this, _, window, cx| {
        this.set_gradient_shape(shape, window, cx);
    }))
}

fn gradient_blend_button(
    label: &'static str,
    selected: bool,
    blend: crate::paint::GradientBlend,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    choice_button(label, selected).on_click(cx.listener(move |this, _, window, cx| {
        this.set_gradient_blend(blend, window, cx);
    }))
}

fn paint_gradient_frame(
    window: &mut Window,
    geometry: &PreviewGeometry,
    viewport: Bounds<Pixels>,
    width: u32,
    height: u32,
    frame: super::gradient_ui::GradientFrame,
) {
    if width == 0 || height == 0 {
        return;
    }
    let project = |corner: (f64, f64)| {
        geometry.project(point(
            corner.0 as f32 / width as f32,
            corner.1 as f32 / height as f32,
        ))
    };
    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        let start = project(frame.start);
        let end = project(frame.end);
        for (weight, color) in [(3., rgb(0x12151c)), (1., rgb(0xffffff))] {
            let mut path = PathBuilder::stroke(px(weight));
            path.move_to(start);
            path.line_to(end);
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        let scale = window.scale_factor();
        let snap = |value: f32| (value * scale).round() / scale;
        for center in [start, end] {
            let origin = point(
                px(snap(f32::from(center.x) - 3.5)),
                px(snap(f32::from(center.y) - 3.5)),
            );
            window.paint_quad(fill(
                Bounds::new(origin, size(px(7.), px(7.))),
                rgb(0xffffff),
            ));
        }
    });
}

fn warp_choice(
    label: &'static str,
    selected: bool,
    mirror: Mirror,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    choice_button(label, selected).on_click(cx.listener(move |this, _, window, cx| {
        this.set_warp_mirror(mirror, window, cx);
    }))
}

fn resample_choice(
    label: &'static str,
    resample: Resample,
    selected: Resample,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    choice_button(label, resample == selected).on_click(cx.listener(move |this, _, window, cx| {
        this.set_warp_resample(resample, window, cx);
    }))
}

fn tolerance_row(value: u8, cx: &Context<super::Workspace>) -> Div {
    option_row(
        "Tolerance",
        stepper(
            value.to_string(),
            step_button("tolerance-down", "−").on_click(cx.listener(|this, _, _, cx| {
                this.tolerance = this.tolerance.saturating_sub(1);
                cx.notify();
            })),
            step_button("tolerance-up", "+").on_click(cx.listener(|this, _, _, cx| {
                this.tolerance = this.tolerance.saturating_add(1);
                cx.notify();
            })),
        ),
    )
}

fn align_controls(enabled: bool, cx: &Context<super::Workspace>) -> Div {
    div().flex().flex_wrap().gap_1().children([
        align_button("align-top", "Align Top", AlignLayer::Top, enabled, cx),
        align_button(
            "align-bottom",
            "Align Bottom",
            AlignLayer::Bottom,
            enabled,
            cx,
        ),
        align_button("align-left", "Align Left", AlignLayer::Left, enabled, cx),
        align_button("align-right", "Align Right", AlignLayer::Right, enabled, cx),
        align_button(
            "align-h-center",
            "Center Horizontally",
            AlignLayer::Horizontal,
            enabled,
            cx,
        ),
        align_button(
            "align-v-center",
            "Center Vertically",
            AlignLayer::Vertical,
            enabled,
            cx,
        ),
    ])
}

fn align_button(
    symbol: &'static str,
    label: &'static str,
    action: AlignLayer,
    enabled: bool,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    div()
        .id(label)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .flex_shrink_0()
        .rounded_md()
        .bg(rgb(CHROME))
        .text_color(rgb(if enabled { MUTED } else { DISABLED }))
        .tooltip(tooltip(label))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                .on_click(cx.listener(move |_, _, window, cx| {
                    window.dispatch_action(Box::new(action), cx);
                }))
        })
        .when(!enabled, |button| button.cursor(CursorStyle::Arrow))
        .child(icon(symbol, 15.))
}

fn selection_button(
    label: &'static str,
    enabled: bool,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    option_action(label, false)
        .when(enabled, |button| {
            button.on_click(move |_, window, cx| action(window, cx))
        })
        .when(!enabled, |button| {
            button.opacity(0.45).cursor(CursorStyle::Arrow)
        })
}

fn paint_paths(
    window: &mut Window,
    geometry: &PreviewGeometry,
    viewport: Bounds<Pixels>,
    canvas: (u32, u32),
    paths: &[crate::paths::VectorPath],
    active: Option<crate::paths::PathId>,
) {
    let (width, height) = canvas;
    if paths.is_empty() || width == 0 || height == 0 {
        return;
    }
    let project = |at: (f64, f64)| {
        geometry.project(point(
            (at.0 / f64::from(width)) as f32,
            (at.1 / f64::from(height)) as f32,
        ))
    };
    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        for path in paths {
            let points = crate::paths::stroke(path);
            let Some(first) = points.first().copied() else {
                continue;
            };
            let current = Some(path.id) == active;
            for (weight, color) in [
                (3., rgb(0x12151c)),
                (1., rgb(if current { ACCENT } else { 0xffffff })),
            ] {
                let mut outline = PathBuilder::stroke(px(weight));
                outline.move_to(project(first));
                for point in &points[1..] {
                    outline.line_to(project(*point));
                }
                if let Ok(outline) = outline.build() {
                    window.paint_path(outline, color);
                }
            }
        }
    });
}

fn paint_path_handles(
    window: &mut Window,
    geometry: &PreviewGeometry,
    viewport: Bounds<Pixels>,
    canvas: (u32, u32),
    paths: &[crate::paths::VectorPath],
    active: Option<crate::paths::PathId>,
) {
    let (width, height) = canvas;
    if paths.is_empty() || width == 0 || height == 0 {
        return;
    }
    let project = |at: (f64, f64)| {
        geometry.project(point(
            (at.0 / f64::from(width)) as f32,
            (at.1 / f64::from(height)) as f32,
        ))
    };
    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        for path in paths {
            let current = Some(path.id) == active;
            for anchor in &path.anchors {
                let center = project(anchor.point);
                if anchor.handle_in != anchor.point {
                    paint_handle_line(window, center, project(anchor.handle_in));
                }
                if anchor.handle_out != anchor.point {
                    paint_handle_line(window, center, project(anchor.handle_out));
                }
                window.paint_quad(fill(
                    Bounds::centered_at(center, size(px(7.), px(7.))),
                    rgb(if current { ACCENT } else { 0xffffff }),
                ));
            }
        }
    });
}

fn paint_handle_line(window: &mut Window, from: Point<Pixels>, to: Point<Pixels>) {
    for (weight, color) in [(3., rgb(0x12151c)), (1., rgb(0xffffff))] {
        let mut line = PathBuilder::stroke(px(weight));
        line.move_to(from);
        line.line_to(to);
        if let Ok(line) = line.build() {
            window.paint_path(line, color);
        }
    }
    window.paint_quad(fill(
        Bounds::centered_at(to, size(px(5.), px(5.))),
        rgb(0xffffff),
    ));
}

fn paint_free_outline(
    window: &mut Window,
    geometry: &PreviewGeometry,
    viewport: Bounds<Pixels>,
    points: &[Point<f32>],
    polygon: bool,
) {
    let Some(first) = points.first().map(|point| geometry.project(*point)) else {
        return;
    };
    let pointer = window.mouse_position();
    let can_close = polygon && points.len() >= 3;
    let near_start = can_close && super::selection::near_polygon_start(first, pointer);
    let tail = (polygon && viewport.contains(&pointer)).then_some(if near_start {
        first
    } else {
        pointer
    });

    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        for (weight, color) in [(3., rgb(0x12151c)), (1., rgb(0xffffff))] {
            let mut path = PathBuilder::stroke(px(weight));
            path.move_to(first);
            for point in &points[1..] {
                path.line_to(geometry.project(*point));
            }
            if let Some(tail) = tail {
                path.line_to(tail);
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        if polygon {
            window.paint_quad(fill(
                Bounds::centered_at(first, size(px(8.), px(8.))),
                rgb(0x12151c),
            ));
            window.paint_quad(fill(
                Bounds::centered_at(first, size(px(6.), px(6.))),
                rgb(if near_start { ACCENT } else { 0xffffff }),
            ));
        }
    });
}

fn paint_segments(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    visible: Bounds<Pixels>,
    segments: &[(i32, i32, i32, i32)],
) {
    if segments.is_empty() || width == 0 || height == 0 {
        return;
    }
    static START: OnceLock<Instant> = OnceLock::new();
    let phase = marching_phase(START.get_or_init(Instant::now).elapsed());
    let mut distance = 0.0;
    for &(x0, y0, x1, y1) in segments {
        let from = geometry.project(point(x0 as f32 / width as f32, y0 as f32 / height as f32));
        let to = geometry.project(point(x1 as f32 / width as f32, y1 as f32 / height as f32));
        paint_marching_span(window, from, to, phase, distance, visible);
        distance += f32::from((to.x - from.x).abs() + (to.y - from.y).abs());
    }
    window.request_animation_frame();
}

fn paint_contours(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    visible: Bounds<Pixels>,
    contours: &[preview::PixelRect],
) {
    if contours.is_empty() || width == 0 || height == 0 {
        return;
    }
    static START: OnceLock<Instant> = OnceLock::new();
    let phase = marching_phase(START.get_or_init(Instant::now).elapsed());
    let mut painted = false;
    for rect in contours {
        if rect.width == 0 || rect.height == 0 {
            continue;
        }
        let start = geometry.project(point(
            rect.x as f32 / width as f32,
            rect.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            rect.right() as f32 / width as f32,
            rect.bottom() as f32 / height as f32,
        ));
        let bounds = Bounds::from_corners(start, end);
        if bounds.intersect(&visible).is_empty() {
            continue;
        }
        paint_marching_rect(window, bounds, visible, phase);
        painted = true;
    }
    if painted {
        window.request_animation_frame();
    }
}

fn brush_row(
    label: &'static str,
    value: u32,
    field: super::paint_ui::BrushField,
    cx: &Context<super::Workspace>,
) -> Div {
    option_row(
        label,
        stepper(
            value.to_string(),
            brush_step(label, false, field, cx),
            brush_step(label, true, field, cx),
        ),
    )
}

fn brush_step(
    label: &'static str,
    up: bool,
    field: super::paint_ui::BrushField,
    cx: &Context<super::Workspace>,
) -> gpui::Stateful<Div> {
    let name = if up { "up" } else { "down" };
    step_button(
        gpui::SharedString::from(format!("{label} {name}")),
        if up { "+" } else { "−" },
    )
    .on_click(cx.listener(move |this, _, _, cx| {
        this.nudge_brush(field, if up { 1 } else { -1 }, cx);
    }))
}

fn paint_stroke_patch(
    window: &mut Window,
    geometry: &PreviewGeometry,
    (width, height): (u32, u32),
    visible: Bounds<Pixels>,
    patch: &super::paint_ui::StrokePaint,
    show_pixels: bool,
) {
    let project = |rect: preview::PixelRect| {
        let start = geometry.project(point(
            rect.x as f32 / width as f32,
            rect.y as f32 / height as f32,
        ));
        let end = geometry.project(point(
            rect.right() as f32 / width as f32,
            rect.bottom() as f32 / height as f32,
        ));
        Bounds::from_corners(start, end)
    };
    let mask = project(patch.content).intersect(&visible);
    if mask.is_empty() {
        return;
    }
    let stored = project(patch.stored);
    window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
        // Repaint the background so erased pixels can reveal transparency.
        paint_checkerboard(window, geometry.bounds, mask);
        if show_pixels {
            paint_source_pixels(window, &patch.pixels, stored, mask);
        } else if let Err(error) =
            window.paint_image(stored, Corners::default(), patch.image.clone(), 0, false)
        {
            eprintln!("Could not paint the stroke: {error}");
        }
    });
}

fn paint_moved_layer(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    move_layer: &super::layers::MovePaint,
) {
    if let Some(below) = &move_layer.below
        && let Err(error) =
            window.paint_image(geometry.bounds, Corners::default(), below.clone(), 0, false)
    {
        eprintln!("Could not paint layers below the moved layer: {error}");
    }
    let (offset_x, offset_y) = move_layer.offset;
    let (layer_width, layer_height) = move_layer.size;
    let start = geometry.project(point(
        offset_x as f32 / width as f32,
        offset_y as f32 / height as f32,
    ));
    let end = geometry.project(point(
        (offset_x as f32 + layer_width as f32) / width as f32,
        (offset_y as f32 + layer_height as f32) / height as f32,
    ));
    if let Err(error) = window.paint_image(
        Bounds::from_corners(start, end),
        Corners::default(),
        move_layer.sprite.clone(),
        0,
        false,
    ) {
        eprintln!("Could not paint the moved layer: {error}");
    }
    if let Some(above) = &move_layer.above
        && let Err(error) =
            window.paint_image(geometry.bounds, Corners::default(), above.clone(), 0, false)
    {
        eprintln!("Could not paint layers above the moved layer: {error}");
    }
}

fn paint_warp_frame(
    window: &mut Window,
    geometry: &PreviewGeometry,
    viewport: Bounds<Pixels>,
    width: u32,
    height: u32,
    frame: super::transform_ui::WarpFrame,
) {
    if width == 0 || height == 0 {
        return;
    }
    let project = |corner: (f64, f64)| {
        geometry.project(point(
            corner.0 as f32 / width as f32,
            corner.1 as f32 / height as f32,
        ))
    };
    window.with_content_mask(Some(ContentMask { bounds: viewport }), |window| {
        let outline = frame.outline.map(project);
        let foreground = rgb(if frame.invalid { 0xffb4ab } else { 0xffffff });
        for (weight, color) in [(3., rgb(0x12151c)), (1., foreground)] {
            let mut path = PathBuilder::stroke(px(weight));
            path.move_to(outline[0]);
            for corner in outline.iter().skip(1) {
                path.line_to(*corner);
            }
            path.line_to(outline[0]);
            if let Some(axis) = frame.axis {
                let axis = axis.map(project);
                path.move_to(axis[0]);
                path.line_to(axis[1]);
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        let scale = window.scale_factor();
        for grip in frame.grips.iter().take(frame.grip_count) {
            let center = project(*grip);
            paint_contrast_square(window, f32::from(center.x), f32::from(center.y), 7., scale);
        }
    });
}

fn paint_handles(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    rect: preview::PixelRect,
) {
    if width == 0 || height == 0 || rect.width == 0 || rect.height == 0 {
        return;
    }
    let project =
        |x: i64, y: i64| geometry.project(point(x as f32 / width as f32, y as f32 / height as f32));
    let top_left = project(rect.x, rect.y);
    let bottom_right = project(rect.right(), rect.bottom());
    let left = f32::from(top_left.x.min(bottom_right.x));
    let right = f32::from(top_left.x.max(bottom_right.x));
    let top = f32::from(top_left.y.min(bottom_right.y));
    let bottom = f32::from(top_left.y.max(bottom_right.y));
    let mid_x = (left + right) / 2.;
    let mid_y = (top + bottom) / 2.;
    let scale = window.scale_factor();
    let handle = 5.;
    for (x, y) in [
        (left, top),
        (mid_x, top),
        (right, top),
        (left, mid_y),
        (right, mid_y),
        (left, bottom),
        (mid_x, bottom),
        (right, bottom),
    ] {
        paint_contrast_square(window, x, y, handle, scale);
    }
}

fn paint_contrast_square(window: &mut Window, x: f32, y: f32, edge: f32, scale: f32) {
    let snap = |value: f32| (value * scale).round() / scale;
    let origin = point(px(snap(x - edge / 2.)), px(snap(y - edge / 2.)));
    window.paint_quad(fill(
        Bounds::new(origin, size(px(edge), px(edge))),
        rgb(0x12151c),
    ));
    let inset = (edge - 2.).max(1.);
    window.paint_quad(fill(
        Bounds::new(origin + point(px(1.), px(1.)), size(px(inset), px(inset))),
        rgb(0xffffff),
    ));
}

fn paint_marching_rect(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
    phase: f32,
) {
    let top = bounds.top();
    let right = bounds.right();
    let bottom = bounds.bottom();
    let left = bounds.left();
    let width = f32::from(bounds.size.width).max(0.);
    let height = f32::from(bounds.size.height).max(0.);
    paint_marching_span(
        window,
        point(left, top),
        point(right, top),
        phase,
        0.,
        visible,
    );
    paint_marching_span(
        window,
        point(right, top),
        point(right, bottom),
        phase,
        width,
        visible,
    );
    paint_marching_span(
        window,
        point(right, bottom),
        point(left, bottom),
        phase,
        width + height,
        visible,
    );
    paint_marching_span(
        window,
        point(left, bottom),
        point(left, top),
        phase,
        width + height + width,
        visible,
    );
}

// `distance` is how far `from` sits along the whole outline, so the dash continues around corners.
fn paint_marching_span(
    window: &mut Window,
    from: Point<Pixels>,
    to: Point<Pixels>,
    phase: f32,
    distance: f32,
    visible: Bounds<Pixels>,
) {
    let horizontal = (f32::from(from.y) - f32::from(to.y)).abs() < f32::from(to.x - from.x).abs();
    let (start, end, cross) = if horizontal {
        (f32::from(from.x), f32::from(to.x), f32::from(from.y))
    } else {
        (f32::from(from.y), f32::from(to.y), f32::from(from.x))
    };
    let length = (end - start).abs();
    if length < 0.5 {
        return;
    }
    let cross_visible = if horizontal {
        cross + 1. >= f32::from(visible.top()) && cross - 1. <= f32::from(visible.bottom())
    } else {
        cross + 1. >= f32::from(visible.left()) && cross - 1. <= f32::from(visible.right())
    };
    if !cross_visible {
        return;
    }
    let (view_min, view_max) = if horizontal {
        (f32::from(visible.left()), f32::from(visible.right()))
    } else {
        (f32::from(visible.top()), f32::from(visible.bottom()))
    };
    let draw_min = start.min(end).max(view_min);
    let draw_max = start.max(end).min(view_max);
    if draw_max - draw_min < 0.5 {
        return;
    }
    let forward = end >= start;
    // Bottom and left edges run back toward the origin. Their visible interval is
    // still a forward distance range, or those two sides never get a dash.
    let (first, last) = span_draw_range(start, end, draw_min, draw_max);
    let scale = window.scale_factor();
    let snap = |value: f32| (value * scale).round() / scale;
    let axis = |travel: f32| {
        if forward {
            start + travel
        } else {
            start - travel
        }
    };
    let mut cursor = first;
    while cursor < last {
        let (white, remaining) = dash_at(cursor, distance, phase);
        let dash_end = (cursor + remaining).min(last);
        if dash_end <= cursor {
            break;
        }
        if dash_end - cursor >= 0.5 {
            let color = if white { rgb(0xffffff) } else { rgb(0x12151c) };
            let left_edge = snap(axis(cursor).min(axis(dash_end)));
            let right_edge = snap(axis(cursor).max(axis(dash_end)));
            let cross = snap(cross);
            let segment = if horizontal {
                Bounds::new(
                    point(px(left_edge), px(cross)),
                    size(px((right_edge - left_edge).max(1.)), px(1.)),
                )
            } else {
                Bounds::new(
                    point(px(cross), px(left_edge)),
                    size(px(1.), px((right_edge - left_edge).max(1.))),
                )
            };
            let segment = segment.intersect(&visible);
            if !segment.is_empty() {
                window.paint_quad(fill(segment, color));
            }
        }
        cursor = dash_end;
    }
}

const SELECTION_DASH: f32 = 4.;
const SELECTION_DASH_PERIOD: f32 = SELECTION_DASH * 2.;

fn marching_phase(elapsed: Duration) -> f32 {
    // Three cycles per second keeps the calculation within the fractional second.
    let seconds = elapsed.subsec_micros() as f32 / 1_000_000.;
    (seconds * (SELECTION_DASH_PERIOD * 3.)) % SELECTION_DASH_PERIOD
}

/// Whether the dash at this point on the outline is white, and how far that dash continues.
/// `distance` carries the phase around corners. Snapping to the dash period would cancel `phase`.
fn dash_at(travel: f32, distance: f32, phase: f32) -> (bool, f32) {
    let cycle = (travel + distance - phase).rem_euclid(SELECTION_DASH_PERIOD);
    if cycle < SELECTION_DASH {
        (true, SELECTION_DASH - cycle)
    } else {
        (false, SELECTION_DASH_PERIOD - cycle)
    }
}

fn span_draw_range(start: f32, end: f32, draw_min: f32, draw_max: f32) -> (f32, f32) {
    let along = |position: f32| {
        if end >= start {
            position - start
        } else {
            start - position
        }
    };
    let near = along(draw_min);
    let far = along(draw_max);
    (near.min(far), near.max(far))
}

fn paint_scale_frame(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    visible: Bounds<Pixels>,
    rect: preview::PixelRect,
) {
    if width == 0 || height == 0 || rect.width == 0 || rect.height == 0 {
        return;
    }
    let start = geometry.project(point(
        rect.x as f32 / width as f32,
        rect.y as f32 / height as f32,
    ));
    let end = geometry.project(point(
        (rect.x + i64::from(rect.width)) as f32 / width as f32,
        (rect.y + i64::from(rect.height)) as f32 / height as f32,
    ));
    let bounds = Bounds::from_corners(start, end);
    if bounds.intersect(&visible).is_empty() {
        return;
    }
    window.with_content_mask(Some(ContentMask { bounds: visible }), |window| {
        window.paint_quad(gpui::outline(
            bounds,
            rgba(0xacc6ffb0),
            gpui::BorderStyle::Solid,
        ));
        paint_handles(window, geometry, width, height, rect);
    });
}

fn paint_layer_bounds(
    window: &mut Window,
    geometry: &PreviewGeometry,
    width: u32,
    height: u32,
    visible: Bounds<Pixels>,
    rect: preview::PixelRect,
) {
    if width == 0 || height == 0 || rect.width == 0 || rect.height == 0 {
        return;
    }
    let start = geometry.project(point(
        rect.x as f32 / width as f32,
        rect.y as f32 / height as f32,
    ));
    let end = geometry.project(point(
        (rect.x + i64::from(rect.width)) as f32 / width as f32,
        (rect.y + i64::from(rect.height)) as f32 / height as f32,
    ));
    let bounds = Bounds::from_corners(start, end);
    if bounds.intersect(&visible).is_empty() {
        return;
    }
    window.with_content_mask(Some(ContentMask { bounds: visible }), |window| {
        window.paint_quad(gpui::outline(
            bounds,
            rgba(0xacc6ff80),
            gpui::BorderStyle::Solid,
        ));
    });
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

#[cfg(test)]
mod tests {
    use super::{Duration, dash_at, marching_phase, span_draw_range};

    #[test]
    fn selection_phase_changes_between_frames_even_at_large_clock_values() {
        let elapsed = Duration::from_secs(2_000_000_000);
        assert_eq!(marching_phase(elapsed + Duration::from_millis(125)), 3.);
        assert_eq!(marching_phase(elapsed + Duration::from_millis(250)), 6.);
    }

    #[test]
    fn a_backward_edge_still_covers_its_visible_span() {
        assert_eq!(span_draw_range(0., 100., 10., 90.), (10., 90.));
        assert_eq!(span_draw_range(100., 0., 10., 90.), (10., 90.));
    }

    #[test]
    fn the_dash_phase_shifts_the_pattern_around_corners() {
        assert_eq!(dash_at(0., 0., 0.), (true, 4.));
        assert_eq!(dash_at(0., 0., 2.), (false, 2.));
        assert_eq!(dash_at(0., 10., 2.), dash_at(10., 0., 2.));
    }
}
