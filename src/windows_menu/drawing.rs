//! Owner drawing changes native menu presentation, not Win32 navigation or
//! activation. Keep the HMENU tree and muda's command IDs intact. Item data starts
//! with MSAAMENUINFO so Windows accessibility can still read the menu names.
//!
//! https://learn.microsoft.com/windows/win32/menurc/using-menus#creating-owner-drawn-menu-items
//! https://learn.microsoft.com/windows/win32/winauto/exposing-owner-drawn-menu-items

use std::{collections::HashMap, mem, ptr};

use anyhow::{Context as _, Result};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{
        Accessibility::{MSAA_MENU_SIG, MSAAMENUINFO},
        Controls::{
            DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_GRAYED, ODS_HOTLIGHT, ODS_NOACCEL,
            ODS_SELECTED, ODT_MENU,
        },
        HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi, SystemParametersInfoForDpi},
        WindowsAndMessaging::*,
    },
};

use super::{MenuNode, MenuPlan};
use crate::{menu_icons, theme};

struct GdiObject(HGDIOBJ);

impl Drop for GdiObject {
    fn drop(&mut self) {
        // SAFETY: every object is created by this module and deselected before
        // drop. Menu background brushes are restored before these are dropped.
        unsafe { DeleteObject(self.0) };
    }
}

struct SavedDc(HDC, i32);

impl SavedDc {
    fn new(dc: HDC) -> Option<Self> {
        let saved = unsafe { SaveDC(dc) };
        (saved != 0).then_some(Self(dc, saved))
    }
}

impl Drop for SavedDc {
    fn drop(&mut self) {
        unsafe { RestoreDC(self.0, self.1) };
    }
}

#[repr(C)]
struct Entry {
    // Must remain first; Win32 reads this without calling our code.
    msaa: MSAAMENUINFO,
    accessible: Vec<u16>,
    menu: HMENU,
    position: u32,
    original_type: u32,
    original_data: usize,
    original_bitmap: HBITMAP,
    label: Vec<u16>,
    accelerator: Vec<u16>,
    mnemonic: Option<char>,
    icon: Option<&'static str>,
    bar: bool,
    separator: bool,
    width: u32,
    height: u32,
}

struct MenuBackground {
    menu: HMENU,
    original: MENUINFO,
    bar: bool,
}

pub(super) struct Menus {
    hwnd: HWND,
    // Boxes keep native item-data pointers stable as this vector grows.
    #[allow(clippy::vec_box)]
    entries: Vec<Box<Entry>>,
    backgrounds: Vec<MenuBackground>,
    font: GdiObject,
    panel_brush: GdiObject,
    chrome_brush: GdiObject,
    border_brush: GdiObject,
    hover_brush: GdiObject,
    selected_brush: GdiObject,
    icons: HashMap<(&'static str, u32), GdiObject>,
    dpi: u32,
    active: bool,
}

impl Menus {
    pub(super) fn new(hwnd: isize, plan: &MenuPlan) -> Result<Self> {
        let hwnd = hwnd as HWND;
        let dpi = window_dpi(hwnd);
        let mut menus = Self {
            hwnd,
            entries: Vec::new(),
            backgrounds: Vec::new(),
            font: menu_font(dpi)?,
            panel_brush: brush(theme::PANEL)?,
            chrome_brush: brush(theme::CHROME)?,
            border_brush: brush(theme::BORDER)?,
            hover_brush: brush(theme::HOVER)?,
            selected_brush: brush(theme::SELECTED)?,
            icons: HashMap::new(),
            dpi,
            active: false,
        };
        let root = unsafe { GetMenu(hwnd) };
        anyhow::ensure!(!root.is_null(), "the native menu bar is missing");
        // Read the installed menu strings (including muda's accelerator labels)
        // and associate icons by action, never by ambiguous display text.
        let nodes: Vec<_> = plan.menus.iter().cloned().map(MenuNode::Submenu).collect();
        menus.collect(root, &nodes, plan, true)?;
        menus.measure()?;
        Ok(menus)
    }

    fn collect(
        &mut self,
        menu: HMENU,
        nodes: &[MenuNode],
        plan: &MenuPlan,
        bar: bool,
    ) -> Result<()> {
        let mut original = MENUINFO {
            cbSize: mem::size_of::<MENUINFO>() as u32,
            fMask: MIM_BACKGROUND | MIM_STYLE,
            ..Default::default()
        };
        anyhow::ensure!(
            unsafe { GetMenuInfo(menu, &mut original) } != 0,
            "could not read menu background"
        );
        anyhow::ensure!(
            unsafe { GetMenuItemCount(menu) } == nodes.len() as i32,
            "native menu tree differs from command tree"
        );
        self.backgrounds.push(MenuBackground {
            menu,
            original,
            bar,
        });
        for (position, node) in nodes.iter().enumerate() {
            let mut info = item_info(
                menu,
                position as u32,
                MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP | MIIM_SUBMENU | MIIM_STRING,
            )?;
            let mut text = vec![0u16; info.cch as usize + 1];
            info.dwTypeData = text.as_mut_ptr();
            info.cch = text.len() as u32;
            anyhow::ensure!(
                unsafe { GetMenuItemInfoW(menu, position as u32, 1, &mut info) } != 0,
                "could not read menu label"
            );
            let text = String::from_utf16_lossy(&text[..info.cch as usize]);
            let (label, accelerator) = text.split_once('\t').unwrap_or((&text, ""));
            let (accessible, mnemonic) = plain_label(label);
            let mut accessible: Vec<_> = accessible.encode_utf16().chain(Some(0)).collect();
            let msaa = MSAAMENUINFO {
                dwMSAASignature: MSAA_MENU_SIG as u32,
                cchWText: accessible.len() as u32 - 1,
                pszWText: accessible.as_mut_ptr(),
            };
            let icon = match node {
                MenuNode::Command(index) => {
                    menu_icons::for_action(plan.commands[*index].action.as_ref())
                }
                _ => None,
            };
            self.entries.push(Box::new(Entry {
                msaa,
                accessible,
                menu,
                position: position as u32,
                original_type: info.fType,
                original_data: info.dwItemData,
                original_bitmap: info.hbmpItem,
                label: label.encode_utf16().collect(),
                accelerator: accelerator.encode_utf16().collect(),
                mnemonic,
                icon,
                bar,
                separator: matches!(node, MenuNode::Separator),
                width: 0,
                height: 0,
            }));
            if let MenuNode::Submenu(section) = node {
                anyhow::ensure!(!info.hSubMenu.is_null(), "native submenu is missing");
                self.collect(info.hSubMenu, &section.items, plan, false)?;
            }
        }
        Ok(())
    }

    /// Caller must redraw the menu bar after releasing its borrow of Menus:
    /// DrawMenuBar synchronously sends our measurement/drawing messages.
    pub(super) fn refresh(&mut self, active: bool) -> Result<()> {
        if self.active {
            self.restore();
            self.active = false;
        }
        if !active {
            return Ok(());
        }
        let dpi = window_dpi(self.hwnd);
        let font = menu_font(dpi)?;
        self.font = font;
        self.dpi = dpi;
        self.icons.clear();
        self.measure()?;
        // Set before mutation so a partially applied tree is restored on error.
        self.active = true;
        for entry in &self.entries {
            let info = MENUITEMINFOW {
                cbSize: mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP,
                fType: entry.original_type | MFT_OWNERDRAW,
                dwItemData: (&**entry as *const Entry) as usize,
                // Our renderer draws the icon. Suppress the native bitmap so it
                // neither paints twice nor adds a second column.
                hbmpItem: ptr::null_mut(),
                ..Default::default()
            };
            anyhow::ensure!(
                unsafe { SetMenuItemInfoW(entry.menu, entry.position, 1, &info) } != 0,
                "could not set menu drawing mode"
            );
        }
        for background in &self.backgrounds {
            let mut info = background.original;
            info.hbrBack = if background.bar {
                self.chrome_brush.0
            } else {
                self.panel_brush.0
            };
            info.dwStyle |= MNS_NOCHECK;
            anyhow::ensure!(
                unsafe { SetMenuInfo(background.menu, &info) } != 0,
                "could not set menu background"
            );
        }
        Ok(())
    }

    /// Capture muda's current owned bitmap handles after updating fallback icons.
    /// This only runs in native mode, before suppressing bitmaps for owner drawing.
    pub(super) fn capture_bitmaps(&mut self) -> Result<()> {
        anyhow::ensure!(!self.active, "cannot capture suppressed menu bitmaps");
        for entry in &mut self.entries {
            entry.original_bitmap = item_info(entry.menu, entry.position, MIIM_BITMAP)?.hbmpItem;
        }
        Ok(())
    }

    /// Windows paints a non-client separator beneath the menu bar separately
    /// from the menu items. Repaint just that line after native frame painting.
    pub(super) fn paint_frame_separator(&self) {
        if !self.active {
            return;
        }
        let mut bar = MENUBARINFO {
            cbSize: mem::size_of::<MENUBARINFO>() as u32,
            ..Default::default()
        };
        let mut window = RECT::default();
        unsafe {
            if GetMenuBarInfo(self.hwnd, OBJID_MENU, 0, &mut bar) == 0
                || GetWindowRect(self.hwnd, &mut window) == 0
            {
                return;
            }
            let line = RECT {
                left: bar.rcBar.left - window.left,
                right: bar.rcBar.right - window.left,
                top: bar.rcBar.bottom - window.top,
                bottom: bar.rcBar.bottom - window.top + 1,
            };
            let dc = GetWindowDC(self.hwnd);
            if !dc.is_null() {
                FillRect(dc, &line, self.border_brush.0);
                ReleaseDC(self.hwnd, dc);
            }
        }
    }

    fn measure(&mut self) -> Result<()> {
        let dc = unsafe { GetDC(self.hwnd) };
        anyhow::ensure!(!dc.is_null(), "could not measure menu font");
        let result = (|| {
            let _saved = SavedDc::new(dc).context("could not save menu measurement context")?;
            unsafe { SelectObject(dc, self.font.0) };
            let mut metrics = TEXTMETRICW::default();
            anyhow::ensure!(
                unsafe { GetTextMetricsW(dc, &mut metrics) } != 0,
                "could not read menu font metrics"
            );
            let dpi = self.dpi;
            let height = row_height(dpi, metrics.tmHeight);
            let bar_height = unsafe { GetSystemMetricsForDpi(SM_CYMENU, dpi) }
                .max(metrics.tmHeight + scaled(4, dpi));
            let mut widths: HashMap<usize, (i32, i32)> = HashMap::new();
            for entry in &mut self.entries {
                if entry.separator {
                    entry.height = scaled(9, dpi) as u32;
                    continue;
                }
                let label = text_width(dc, &entry.label, false);
                let accelerator = text_width(dc, &entry.accelerator, true);
                entry.height = if entry.bar { bar_height } else { height } as u32;
                if entry.bar {
                    entry.width = (label + scaled(16, dpi)) as u32;
                } else {
                    let widest = widths.entry(entry.menu as usize).or_default();
                    widest.0 = widest.0.max(label);
                    widest.1 = widest.1.max(accelerator);
                }
            }
            for entry in &mut self.entries {
                if !entry.bar {
                    let (label, accelerator) = widths
                        .get(&(entry.menu as usize))
                        .copied()
                        .unwrap_or_default();
                    // One icon column for the whole dropdown, including plain
                    // items. Align shortcut labels independently of label width.
                    entry.width = popup_width(dpi, label, accelerator) as u32;
                }
            }
            Ok(())
        })();
        unsafe { ReleaseDC(self.hwnd, dc) };
        result
    }

    /// Never dereference itemData from an unknown native message. Only our own
    /// retained allocations are accepted, so system menus/other controls pass on.
    pub(super) unsafe fn handle(
        &mut self,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if !self.active || lparam == 0 {
            return None;
        }
        match message {
            WM_MEASUREITEM => {
                let measure = unsafe { &mut *(lparam as *mut MEASUREITEMSTRUCT) };
                if measure.CtlType != ODT_MENU {
                    return None;
                }
                let entry = self
                    .entries
                    .iter()
                    .find(|entry| (&***entry as *const Entry) as usize == measure.itemData)?;
                measure.itemWidth = entry.width;
                measure.itemHeight = entry.height;
                Some(1)
            }
            WM_DRAWITEM => {
                let draw = unsafe { &*(lparam as *const DRAWITEMSTRUCT) };
                if draw.CtlType != ODT_MENU {
                    return None;
                }
                let index = self
                    .entries
                    .iter()
                    .position(|entry| (&**entry as *const Entry) as usize == draw.itemData)?;
                self.draw(index, draw);
                Some(1)
            }
            WM_MENUCHAR => self.mnemonic(lparam as HMENU, (wparam & 0xffff) as u32),
            _ => None,
        }
    }

    fn draw(&mut self, index: usize, draw: &DRAWITEMSTRUCT) {
        let Some(_saved) = SavedDc::new(draw.hDC) else {
            return;
        };
        let entry = &self.entries[index];
        let selected = draw.itemState & ODS_SELECTED != 0;
        let hot = draw.itemState & ODS_HOTLIGHT != 0;
        let disabled = draw.itemState & (ODS_DISABLED | ODS_GRAYED) != 0;
        let background = if selected {
            &self.selected_brush
        } else if hot {
            &self.hover_brush
        } else if entry.bar {
            &self.chrome_brush
        } else {
            &self.panel_brush
        };
        unsafe {
            FillRect(draw.hDC, &draw.rcItem, background.0);
            SelectObject(draw.hDC, self.font.0);
            SetBkMode(draw.hDC, TRANSPARENT as i32);
        }
        if entry.separator {
            let mut line = draw.rcItem;
            line.left += scaled(8, self.dpi);
            line.right -= scaled(8, self.dpi);
            line.top = (line.top + line.bottom) / 2;
            line.bottom = line.top + scaled(1, self.dpi);
            unsafe { FillRect(draw.hDC, &line, self.border_brush.0) };
            return;
        }
        let color = if disabled {
            theme::DISABLED
        } else {
            theme::TEXT
        };
        unsafe { SetTextColor(draw.hDC, colorref(color)) };
        let mut rect = draw.rcItem;
        let flags = DT_SINGLELINE
            | DT_VCENTER
            | if draw.itemState & ODS_NOACCEL != 0 {
                DT_HIDEPREFIX
            } else {
                0
            };
        if entry.bar {
            unsafe {
                DrawTextW(
                    draw.hDC,
                    entry.label.as_ptr(),
                    entry.label.len() as i32,
                    &mut rect,
                    flags | DT_CENTER,
                )
            };
        } else {
            rect.left += scaled(34, self.dpi);
            rect.right -= scaled(22, self.dpi);
            unsafe {
                DrawTextW(
                    draw.hDC,
                    entry.label.as_ptr(),
                    entry.label.len() as i32,
                    &mut rect,
                    flags,
                );
                DrawTextW(
                    draw.hDC,
                    entry.accelerator.as_ptr(),
                    entry.accelerator.len() as i32,
                    &mut rect,
                    DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
                );
            }
            if let Some(icon) = entry.icon {
                self.draw_icon(draw, icon, color);
            }
            // Win32 draws the submenu arrow after WM_DRAWITEM; reserve its
            // right-hand column instead of drawing a second arrow over it.
        }
    }

    fn draw_icon(&mut self, draw: &DRAWITEMSTRUCT, icon: &'static str, color: u32) {
        let pixels = scaled(16, self.dpi);
        if let std::collections::hash_map::Entry::Vacant(slot) = self.icons.entry((icon, color)) {
            match bitmap(icon, pixels as u32, color) {
                Ok(bitmap) => {
                    slot.insert(bitmap);
                }
                Err(error) => {
                    eprintln!("Could not draw menu icon: {error:#}");
                    return;
                }
            }
        }
        let bitmap = &self.icons[&(icon, color)];
        let dc = unsafe { CreateCompatibleDC(draw.hDC) };
        if dc.is_null() {
            return;
        }
        unsafe {
            let old = SelectObject(dc, bitmap.0);
            AlphaBlend(
                draw.hDC,
                draw.rcItem.left + scaled(9, self.dpi),
                draw.rcItem.top + (draw.rcItem.bottom - draw.rcItem.top - pixels) / 2,
                pixels,
                pixels,
                dc,
                0,
                0,
                pixels,
                pixels,
                BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                },
            );
            SelectObject(dc, old);
            DeleteDC(dc);
        }
    }

    fn mnemonic(&self, menu: HMENU, character: u32) -> Option<LRESULT> {
        let character = char::from_u32(character)?.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.menu == menu && !entry.separator)
            .find_map(|entry| {
                if entry.mnemonic != Some(character) {
                    return None;
                }
                let info = item_info(menu, entry.position, MIIM_STATE).ok()?;
                (info.fState & (MFS_DISABLED | MFS_GRAYED) == 0)
                    .then_some(((MNC_EXECUTE as usize) << 16 | entry.position as usize) as LRESULT)
            })
    }

    fn restore(&self) {
        for entry in &self.entries {
            let info = MENUITEMINFOW {
                cbSize: mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP,
                fType: entry.original_type,
                dwItemData: entry.original_data,
                hbmpItem: entry.original_bitmap,
                ..Default::default()
            };
            unsafe { SetMenuItemInfoW(entry.menu, entry.position, 1, &info) };
        }
        for background in &self.backgrounds {
            unsafe { SetMenuInfo(background.menu, &background.original) };
        }
    }
}

impl Drop for Menus {
    fn drop(&mut self) {
        // The adapter drops us before detaching/destroying the native menu.
        if self.active {
            self.restore();
        }
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    unsafe { GetDpiForWindow(hwnd) }.max(96)
}

fn scaled(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

fn row_height(dpi: u32, font_height: i32) -> i32 {
    scaled(26, dpi).max(font_height + scaled(8, dpi))
}

fn popup_width(dpi: u32, label: i32, accelerator: i32) -> i32 {
    scaled(34 + 22, dpi)
        + label
        + if accelerator > 0 {
            scaled(28, dpi) + accelerator
        } else {
            0
        }
}

fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

fn brush(rgb: u32) -> Result<GdiObject> {
    let brush = unsafe { CreateSolidBrush(colorref(rgb)) };
    anyhow::ensure!(!brush.is_null(), "could not allocate menu brush");
    Ok(GdiObject(brush))
}

fn menu_font(dpi: u32) -> Result<GdiObject> {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    anyhow::ensure!(
        unsafe {
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS,
                metrics.cbSize,
                (&mut metrics as *mut NONCLIENTMETRICSW).cast(),
                0,
                dpi,
            )
        } != 0,
        "could not read system menu font"
    );
    let font = unsafe { CreateFontIndirectW(&metrics.lfMenuFont) };
    anyhow::ensure!(!font.is_null(), "could not create system menu font");
    Ok(GdiObject(font))
}

fn item_info(menu: HMENU, position: u32, mask: u32) -> Result<MENUITEMINFOW> {
    let mut info = MENUITEMINFOW {
        cbSize: mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: mask,
        ..Default::default()
    };
    anyhow::ensure!(
        unsafe { GetMenuItemInfoW(menu, position, 1, &mut info) } != 0,
        "could not read native menu item"
    );
    Ok(info)
}

fn text_width(dc: HDC, text: &[u16], literal: bool) -> i32 {
    let mut rect = RECT::default();
    unsafe {
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE | if literal { DT_NOPREFIX } else { 0 },
        )
    };
    rect.right - rect.left
}

fn plain_label(text: &str) -> (String, Option<char>) {
    let mut chars = text.chars();
    let mut label = String::new();
    let mut mnemonic = None;
    while let Some(ch) = chars.next() {
        if ch == '&' {
            if let Some(next) = chars.next() {
                if next != '&' && mnemonic.is_none() {
                    mnemonic = Some(next.to_ascii_lowercase());
                }
                label.push(next);
            }
        } else {
            label.push(ch);
        }
    }
    (label, mnemonic)
}

fn bitmap(icon: &str, pixels: u32, color: u32) -> Result<GdiObject> {
    let rgba = menu_icons::rasterize(icon, pixels, color)?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: pixels as i32,
            biHeight: -(pixels as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = ptr::null_mut();
    let handle = unsafe {
        CreateDIBSection(
            ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        )
    };
    anyhow::ensure!(!handle.is_null(), "could not create menu icon bitmap");
    let handle = GdiObject(handle);
    anyhow::ensure!(!bits.is_null(), "could not map menu icon bitmap");
    // AlphaBlend needs premultiplied BGRA, whereas muda's Icon uses straight RGBA.
    let target = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), rgba.len()) };
    for (source, target) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(target.as_chunks_mut::<4>().0.iter_mut())
    {
        let alpha = u16::from(source[3]);
        for (out, channel) in target[..3]
            .iter_mut()
            .zip([source[2], source[1], source[0]])
        {
            *out = ((u16::from(channel) * alpha + 127) / 255) as u8;
        }
        target[3] = source[3];
    }
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_geometry_scales_and_leaves_room_for_shortcuts() {
        for dpi in [96, 120, 144, 192, 288] {
            assert!(row_height(dpi, scaled(16, dpi)) >= scaled(26, dpi));
            assert!(row_height(dpi, scaled(40, dpi)) > scaled(40, dpi));
            assert!(popup_width(dpi, 100, 80) >= 180 + scaled(84, dpi));
            assert!(popup_width(dpi, 100, 0) >= 100 + scaled(56, dpi));
        }
    }

    #[test]
    fn accessible_names_and_mnemonics_preserve_literal_ampersands() {
        assert_eq!(
            plain_label("Brightness && &Contrast…"),
            ("Brightness & Contrast…".into(), Some('c'))
        );
        assert_eq!(plain_label("&Open…"), ("Open…".into(), Some('o')));
        assert_eq!(plain_label("RGB"), ("RGB".into(), None));
        assert_eq!(mem::offset_of!(Entry, msaa), 0);
    }

    #[test]
    fn gdi_colors_match_the_workspace_palette() {
        assert_eq!(colorref(theme::PANEL), 0x241d19);
        assert_eq!(colorref(theme::TEXT), 0xf4ede9);
    }

    #[test]
    fn native_menu_drawing_preserves_ids_accessibility_and_fallback() {
        use super::super::{Command, MenuSection, build_native_menus};
        use crate::commands::{Levels, Undo};

        // No visible window or user interaction. Exercise actual Win32 HMENU,
        // GDI drawing, and state transitions in the native Windows CI job.
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe { DestroyWindow(self.0) };
            }
        }
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                windows_sys::w!("STATIC"),
                windows_sys::w!("Lime menu test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        let window = TestWindow(hwnd);
        let plan = MenuPlan {
            menus: vec![MenuSection {
                label: "File".into(),
                items: vec![
                    MenuNode::Command(0),
                    MenuNode::Separator,
                    MenuNode::Command(1),
                ],
            }],
            commands: vec![
                Command {
                    label: "Undo".into(),
                    action: Box::new(Undo),
                    accelerator: None,
                },
                Command {
                    label: "Levels…".into(),
                    action: Box::new(Levels),
                    accelerator: None,
                },
            ],
        };
        let (native, items) = build_native_menus(&plan).unwrap();
        unsafe { native.init_for_hwnd_with_theme(hwnd as isize, muda::MenuTheme::Light) }.unwrap();
        {
            let mut menus = Menus::new(hwnd as isize, &plan).unwrap();
            let index = menus
                .entries
                .iter()
                .position(|entry| entry.icon == Some("undo"))
                .unwrap();
            let (menu, position) = (menus.entries[index].menu, menus.entries[index].position);
            let original = item_info(
                menu,
                position,
                MIIM_ID | MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP,
            )
            .unwrap();
            assert!(!original.hbmpItem.is_null());
            menus.refresh(true).unwrap();
            let styled = item_info(
                menu,
                position,
                MIIM_ID | MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP,
            )
            .unwrap();
            assert_eq!(styled.wID, original.wID);
            assert_ne!(styled.fType & MFT_OWNERDRAW, 0);
            assert!(styled.hbmpItem.is_null());
            let entry = &menus.entries[index];
            assert_eq!(entry.msaa.dwMSAASignature, MSAA_MENU_SIG as u32);
            assert_eq!(
                unsafe {
                    std::slice::from_raw_parts(entry.msaa.pszWText, entry.msaa.cchWText as usize)
                },
                "Undo".encode_utf16().collect::<Vec<_>>()
            );
            let mut measure = MEASUREITEMSTRUCT {
                CtlType: ODT_MENU,
                itemData: styled.dwItemData,
                ..Default::default()
            };
            assert_eq!(
                unsafe {
                    menus.handle(
                        WM_MEASUREITEM,
                        0,
                        (&mut measure as *mut MEASUREITEMSTRUCT) as LPARAM,
                    )
                },
                Some(1)
            );
            assert!(measure.itemWidth > 0 && measure.itemHeight > 0);
            let dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
            assert!(!dc.is_null());
            let surface = bitmap("undo", 128, 0).unwrap();
            let old = unsafe { SelectObject(dc, surface.0) };
            for state in [0, ODS_SELECTED, ODS_DISABLED, ODS_HOTLIGHT] {
                let draw = DRAWITEMSTRUCT {
                    CtlType: ODT_MENU,
                    hDC: dc,
                    rcItem: RECT {
                        left: 0,
                        top: 0,
                        right: 128,
                        bottom: measure.itemHeight as i32,
                    },
                    itemData: styled.dwItemData,
                    itemState: state,
                    ..Default::default()
                };
                assert_eq!(
                    unsafe {
                        menus.handle(WM_DRAWITEM, 0, (&draw as *const DRAWITEMSTRUCT) as LPARAM)
                    },
                    Some(1)
                );
                let expected = if state == ODS_SELECTED {
                    theme::SELECTED
                } else if state == ODS_HOTLIGHT {
                    theme::HOVER
                } else {
                    theme::PANEL
                };
                assert_eq!(unsafe { GetPixel(dc, 1, 1) }, colorref(expected));
            }
            unsafe {
                SelectObject(dc, old);
                DeleteDC(dc);
            }
            let mnemonic = menus.entries[index].mnemonic.unwrap() as u32;
            assert_eq!(
                menus.mnemonic(menu, mnemonic),
                Some(((MNC_EXECUTE as usize) << 16 | position as usize) as LRESULT)
            );
            items[0].set_enabled(false);
            assert_eq!(menus.mnemonic(menu, mnemonic), None);
            // High contrast restores the original type, bitmap, and item data.
            menus.refresh(false).unwrap();
            let restored = item_info(
                menu,
                position,
                MIIM_ID | MIIM_FTYPE | MIIM_DATA | MIIM_BITMAP,
            )
            .unwrap();
            assert_eq!(
                (
                    restored.wID,
                    restored.fType,
                    restored.dwItemData,
                    restored.hbmpItem
                ),
                (
                    original.wID,
                    original.fType,
                    original.dwItemData,
                    original.hbmpItem
                )
            );
            if let super::super::CommandItem::Icon(item) = &items[0] {
                item.set_icon(Some(
                    muda::Icon::from_rgba(menu_icons::rasterize("undo", 16, 0).unwrap(), 16, 16)
                        .unwrap(),
                ));
            }
            menus.capture_bitmaps().unwrap();
            let replacement = item_info(menu, position, MIIM_BITMAP).unwrap().hbmpItem;
            menus.refresh(true).unwrap();
            menus.refresh(false).unwrap();
            assert_eq!(
                item_info(menu, position, MIIM_BITMAP).unwrap().hbmpItem,
                replacement
            );
            menus.refresh(true).unwrap();
            // Drop also restores styles while their native menu is still alive.
        }
        unsafe { native.remove_for_hwnd(hwnd as isize) }.unwrap();
        drop(window);
    }
}
