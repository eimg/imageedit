//! Finish the two details Win32 paints outside WM_DRAWITEM: submenu arrows and
//! popup frames. Subclass only the popup obtained from our own menu's drawing
//! DC, on the owner's thread. No global hooks, class patches, or leaked DC clips.

use std::{cell::Cell, collections::HashSet, rc::Rc};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{
        HiDpi::GetDpiForWindow,
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

use super::{GdiObject, SavedDc, brush, colorref, item_info, scaled};
use crate::theme;

const SUBCLASS_ID: usize = 0x4c494d46;
const FINISH_PAINT: u32 = WM_APP + 0x4c1;

#[derive(Default)]
pub(super) struct Popups(HashSet<isize>);

struct Popup {
    menu: Cell<HMENU>,
    pending: Cell<bool>,
    panel: GdiObject,
    border: GdiObject,
    selected: GdiObject,
}

impl Popups {
    pub(super) fn remember(&mut self, dc: HDC, owner: HWND, menu: HMENU) {
        let hwnd = unsafe { WindowFromDC(dc) };
        if !is_menu_window(hwnd, owner) {
            return;
        }
        let popup = match get_popup(hwnd).or_else(|| install(hwnd, menu)) {
            Some(popup) => popup,
            None => return,
        };
        popup.menu.set(menu);
        self.0.insert(hwnd as isize);
        // Selection changes can draw directly, without WM_PAINT. Finish after
        // Win32 draws its arrow on return from WM_DRAWITEM. Coalesce one message
        // per popup; there is no timer or repaint polling while idle.
        if !popup.pending.replace(true) && unsafe { PostMessageW(hwnd, FINISH_PAINT, 0, 0) } == 0 {
            popup.pending.set(false);
        }
    }

    pub(super) fn clear(&mut self) {
        for hwnd in self.0.drain() {
            detach(hwnd as HWND);
        }
    }
}

impl Drop for Popups {
    fn drop(&mut self) {
        self.clear();
    }
}

fn is_menu_window(hwnd: HWND, owner: HWND) -> bool {
    if hwnd.is_null() || hwnd == owner {
        return false;
    }
    let mut class = [0u16; 16];
    unsafe {
        GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
            == GetWindowThreadProcessId(owner, std::ptr::null_mut())
            && GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) == 6
            && class[..6]
                == [
                    b'#' as u16,
                    b'3' as u16,
                    b'2' as u16,
                    b'7' as u16,
                    b'6' as u16,
                    b'8' as u16,
                ]
    }
}

fn install(hwnd: HWND, menu: HMENU) -> Option<Rc<Popup>> {
    let popup = Rc::new(Popup {
        menu: Cell::new(menu),
        pending: Cell::new(false),
        panel: brush(theme::PANEL).ok()?,
        border: brush(theme::BORDER).ok()?,
        selected: brush(theme::SELECTED).ok()?,
    });
    // One strong reference belongs to the subclass until detach/WM_NCDESTROY.
    let data = Rc::into_raw(popup.clone()) as usize;
    if unsafe { SetWindowSubclass(hwnd, Some(popup_proc), SUBCLASS_ID, data) } == 0 {
        unsafe { drop(Rc::from_raw(data as *const Popup)) };
        return None;
    }
    Some(popup)
}

fn get_popup(hwnd: HWND) -> Option<Rc<Popup>> {
    let mut data = 0;
    if unsafe { GetWindowSubclass(hwnd, Some(popup_proc), SUBCLASS_ID, &mut data) } == 0
        || data == 0
    {
        return None;
    }
    // SAFETY: only our unique callback/ID can own this data, on this UI thread.
    unsafe { Rc::increment_strong_count(data as *const Popup) };
    Some(unsafe { Rc::from_raw(data as *const Popup) })
}

fn detach(hwnd: HWND) {
    let mut data = 0;
    if unsafe { GetWindowSubclass(hwnd, Some(popup_proc), SUBCLASS_ID, &mut data) } != 0
        && unsafe { RemoveWindowSubclass(hwnd, Some(popup_proc), SUBCLASS_ID) } != 0
    {
        unsafe { drop(Rc::from_raw(data as *const Popup)) };
    }
}

unsafe extern "system" fn popup_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // Keep state alive across native calls, including reentrant destruction.
    unsafe { Rc::increment_strong_count(data as *const Popup) };
    let popup = unsafe { Rc::from_raw(data as *const Popup) };
    if message == WM_NCDESTROY {
        detach(hwnd);
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    }
    if message == FINISH_PAINT {
        popup.pending.set(false);
        popup.paint(hwnd);
        return 0;
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if matches!(message, WM_PAINT | WM_NCPAINT)
        && get_popup(hwnd).is_some_and(|current| Rc::ptr_eq(&current, &popup))
    {
        popup.paint(hwnd);
    }
    result
}

impl Popup {
    fn paint(&self, hwnd: HWND) {
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        // Read current native state rather than caching the last hovered row.
        // This covers keyboard navigation, deselection, and disabled submenus.
        let dc = unsafe { GetDC(hwnd) };
        if !dc.is_null() {
            if let Some(_saved) = SavedDc::new(dc) {
                for position in 0..unsafe { GetMenuItemCount(self.menu.get()) }.max(0) as u32 {
                    let Ok(info) = item_info(self.menu.get(), position, MIIM_SUBMENU | MIIM_STATE)
                    else {
                        continue;
                    };
                    if info.hSubMenu.is_null() {
                        continue;
                    }
                    let mut rect = RECT::default();
                    if unsafe {
                        GetMenuItemRect(std::ptr::null_mut(), self.menu.get(), position, &mut rect)
                    } == 0
                    {
                        continue;
                    }
                    unsafe {
                        MapWindowPoints(
                            std::ptr::null_mut(),
                            hwnd,
                            (&mut rect as *mut RECT).cast(),
                            2,
                        )
                    };
                    let selected = info.fState & MFS_HILITE != 0;
                    let color = if info.fState & (MFS_DISABLED | MFS_GRAYED) != 0 {
                        theme::DISABLED
                    } else {
                        theme::TEXT
                    };
                    paint_arrow(
                        dc,
                        rect,
                        dpi,
                        color,
                        if selected {
                            self.selected.0
                        } else {
                            self.panel.0
                        },
                    );
                }
            }
            unsafe { ReleaseDC(hwnd, dc) };
        }
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut bounds) } == 0 {
            return;
        }
        let rect = RECT {
            left: 0,
            top: 0,
            right: bounds.right - bounds.left,
            bottom: bounds.bottom - bounds.top,
        };
        let dc = unsafe { GetWindowDC(hwnd) };
        if !dc.is_null() {
            paint_border(dc, rect, dpi, self.panel.0, self.border.0);
            unsafe { ReleaseDC(hwnd, dc) };
        }
    }
}

fn paint_arrow(dc: HDC, mut rect: RECT, dpi: u32, color: u32, background: HBRUSH) {
    rect.left = rect.right - scaled(20, dpi);
    // Erase Windows' black/white triangle before drawing Lime's chevron.
    unsafe { FillRect(dc, &rect, background) };
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let dx = scaled(2, dpi).max(1);
    let dy = scaled(4, dpi).max(2);
    let points = [
        POINT {
            x: cx - dx,
            y: cy - dy,
        },
        POINT { x: cx + dx, y: cy },
        POINT {
            x: cx - dx,
            y: cy + dy,
        },
    ];
    let pen = unsafe { CreatePen(PS_SOLID, scaled(1, dpi).max(1), colorref(color)) };
    if pen.is_null() {
        return;
    }
    let pen = GdiObject(pen);
    unsafe {
        let old = SelectObject(dc, pen.0);
        Polyline(dc, points.as_ptr(), points.len() as i32);
        SelectObject(dc, old);
    }
}

fn paint_border(dc: HDC, mut rect: RECT, dpi: u32, panel: HBRUSH, border: HBRUSH) {
    // Cover both native bevel edges, then draw a flat one-logical-pixel border.
    // Do not change popup styles, dimensions, placement, or shadows.
    for inset in 0..scaled(2, dpi).max(2) {
        let brush = if inset < scaled(1, dpi).max(1) {
            border
        } else {
            panel
        };
        unsafe { FrameRect(dc, &rect, brush) };
        rect.left += 1;
        rect.top += 1;
        rect.right -= 1;
        rect.bottom -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_details_use_lime_colors_and_preserve_the_drawing_context() {
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        assert!(!dc.is_null());
        let surface = super::super::bitmap("undo", 128, 0).unwrap();
        let old = unsafe { SelectObject(dc, surface.0) };
        let panel = brush(theme::PANEL).unwrap();
        let border = brush(theme::BORDER).unwrap();
        let selected = brush(theme::SELECTED).unwrap();
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 128,
            bottom: 128,
        };
        for dpi in [96, 144, 192] {
            unsafe { FillRect(dc, &bounds, panel.0) };
            paint_border(dc, bounds, dpi, panel.0, border.0);
            assert_eq!(unsafe { GetPixel(dc, 0, 0) }, colorref(theme::BORDER));
            assert_eq!(
                unsafe { GetPixel(dc, scaled(1, dpi), scaled(1, dpi)) },
                colorref(theme::PANEL)
            );
            for (color, background) in [
                (theme::TEXT, panel.0),
                (theme::TEXT, selected.0),
                (theme::DISABLED, panel.0),
            ] {
                let row = RECT {
                    left: 4,
                    top: 8,
                    right: 120,
                    bottom: 8 + scaled(26, dpi),
                };
                let pen = unsafe { GetCurrentObject(dc, OBJ_PEN as u32) };
                let mut clip = RECT::default();
                unsafe { GetClipBox(dc, &mut clip) };
                paint_arrow(dc, row, dpi, color, background);
                let cx = row.right - scaled(10, dpi);
                let cy = (row.top + row.bottom) / 2;
                assert_eq!(
                    unsafe { GetPixel(dc, cx + scaled(2, dpi), cy) },
                    colorref(color)
                );
                assert_eq!(unsafe { GetCurrentObject(dc, OBJ_PEN as u32) }, pen);
                let mut after = RECT::default();
                unsafe { GetClipBox(dc, &mut after) };
                assert_eq!(
                    (after.left, after.top, after.right, after.bottom),
                    (clip.left, clip.top, clip.right, clip.bottom)
                );
            }
        }
        unsafe {
            SelectObject(dc, old);
            DeleteDC(dc);
        }
    }

    #[test]
    fn popup_state_is_released_on_detach_and_window_destruction() {
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Lime popup test"),
                WS_POPUP,
                0,
                0,
                120,
                60,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let menu = unsafe { CreatePopupMenu() };
        assert!(!menu.is_null());
        let popup = install(hwnd, menu).unwrap();
        let weak = Rc::downgrade(&popup);
        drop(popup);
        assert!(get_popup(hwnd).is_some());
        let mut popups = Popups::default();
        popups.0.insert(hwnd as isize);
        popups.clear();
        assert!(get_popup(hwnd).is_none());
        assert!(weak.upgrade().is_none());
        let popup = install(hwnd, menu).unwrap();
        let weak = Rc::downgrade(&popup);
        drop(popup);
        unsafe {
            DestroyWindow(hwnd);
            DestroyMenu(menu);
        }
        assert!(weak.upgrade().is_none());
    }
}
