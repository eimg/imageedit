//! Native Windows menu bar built from GPUI's canonical menu tree.
//!
//! GPUI 0.2.2 stores application menus but does not display them on Windows.
//! This module converts the stored tree into `muda` menus, attaches them to the
//! window's HWND, keeps their native owners alive on the UI thread, and routes
//! activation back to GPUI actions after the Win32 callback has returned.

mod appearance;
mod drawing;

use std::{cell::RefCell, collections::HashSet, rc::Rc};

use anyhow::{Context as _, Result};
use futures::{StreamExt, channel::mpsc};
use gpui::{Action, App, Keymap, Keystroke, OwnedMenu, OwnedMenuItem, Window};
use muda::{
    Icon, IconMenuItem, Menu as NativeMenu, MenuEvent, MenuId, MenuItem as NativeMenuItem,
    MenuTheme, PredefinedMenuItem, Submenu as NativeSubmenu,
    accelerator::{Key, KeyAccelerator, Modifiers, NamedKey},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_F10, VK_MENU, VK_SHIFT},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            DefWindowProcW, DrawMenuBar, MSG, PostMessageW, TranslateMessage, WM_DPICHANGED,
            WM_EXITMENULOOP, WM_INITMENUPOPUP, WM_KEYDOWN, WM_KEYUP, WM_NCACTIVATE, WM_NCDESTROY,
            WM_NCPAINT, WM_SETTINGCHANGE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_THEMECHANGED,
        },
    },
};

const COMMAND_ID_PREFIX: &str = "lime.command.";
const KEYBOARD_SUBCLASS_ID: usize = 0x4c494d45;

thread_local! {
    static ATTACHED: RefCell<Option<AttachedMenus>> = const { RefCell::new(None) };
    static REQUESTS: RefCell<Option<mpsc::UnboundedSender<MenuRequest>>> = const { RefCell::new(None) };
}

enum MenuRequest {
    Activate(MenuId),
    Refresh,
    Appearance,
}

struct AttachedMenus {
    hwnd: isize,
    menu: NativeMenu,
    commands: Vec<Command>,
    items: Vec<CommandItem>,
    enabled: Vec<Option<bool>>,
    mnemonics: Vec<usize>,
    sync_pending: bool,
    drawing: Option<Rc<RefCell<drawing::Menus>>>,
}

#[derive(Clone)]
enum CommandItem {
    Text(NativeMenuItem),
    Icon(IconMenuItem),
}

impl CommandItem {
    fn set_enabled(&self, enabled: bool) {
        match self {
            Self::Text(item) => item.set_enabled(enabled),
            Self::Icon(item) => item.set_enabled(enabled),
        }
    }
}

impl AttachedMenus {
    fn remove(mut self) {
        // Restore menu data/backgrounds before either the native menu or our
        // retained drawing buffers are destroyed.
        drop(self.drawing.take());
        // SAFETY: attachment and teardown run on the HWND's owning UI thread.
        // RemoveWindowSubclass also safely fails if Windows already destroyed it.
        unsafe {
            RemoveWindowSubclass(
                self.hwnd as HWND,
                Some(keyboard_subclass),
                KEYBOARD_SUBCLASS_ID,
            );
            let _ = self.menu.remove_for_hwnd(self.hwnd);
        }
    }
}

pub(crate) fn configure_renderer() {
    // GPUI's DirectComposition path uses WS_EX_NOREDIRECTIONBITMAP, so the
    // GDI-painted native menu bar has no backing surface (the desktop shows
    // through). Its HWND swap-chain path keeps DirectX acceleration and native
    // non-client painting. This must run before Application::new reads the flag.
    // SAFETY: this is Windows-only and called first in main, before GPUI starts.
    unsafe { std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "true") };
    appearance::configure_app();
}

#[derive(Debug)]
struct Command {
    label: String,
    action: Box<dyn Action>,
    accelerator: Option<KeyAccelerator>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuNode {
    Separator,
    Submenu(MenuSection),
    Command(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuSection {
    label: String,
    items: Vec<MenuNode>,
}

#[derive(Debug)]
struct MenuPlan {
    menus: Vec<MenuSection>,
    commands: Vec<Command>,
}

impl MenuPlan {
    fn from_menus(menus: Vec<OwnedMenu>, keymap: &Keymap) -> Self {
        let mut plan = Self {
            menus: Vec::new(),
            commands: Vec::new(),
        };
        for menu in menus {
            let section = plan.convert_menu(menu, keymap);
            plan.menus.push(section);
        }
        plan
    }

    fn convert_menu(&mut self, menu: OwnedMenu, keymap: &Keymap) -> MenuSection {
        let mut items = Vec::new();
        for item in menu.items {
            self.convert_item(item, keymap, &mut items);
        }
        MenuSection {
            label: menu.name.to_string(),
            items,
        }
    }

    fn convert_item(&mut self, item: OwnedMenuItem, keymap: &Keymap, items: &mut Vec<MenuNode>) {
        match item {
            OwnedMenuItem::Separator => items.push(MenuNode::Separator),
            OwnedMenuItem::Submenu(menu) => {
                items.push(MenuNode::Submenu(self.convert_menu(menu, keymap)));
            }
            // Windows does not use macOS system menus such as Services.
            OwnedMenuItem::SystemMenu(_) => {}
            OwnedMenuItem::Action {
                name,
                action,
                os_action: _,
            } => {
                let id = self.commands.len();
                let accelerator = accelerator_for(keymap, action.as_ref());
                self.commands.push(Command {
                    label: name,
                    action,
                    accelerator,
                });
                items.push(MenuNode::Command(id));
            }
        }
    }
}

fn command_availability(
    commands: &[Command],
    mut is_available: impl FnMut(&dyn Action) -> bool,
) -> Vec<bool> {
    commands
        .iter()
        .map(|command| is_available(command.action.as_ref()))
        .collect()
}

pub(crate) fn start(cx: &mut App) {
    let (sender, mut receiver) = mpsc::unbounded::<MenuRequest>();
    REQUESTS.with(|requests| *requests.borrow_mut() = Some(sender.clone()));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = sender.unbounded_send(MenuRequest::Activate(event.id));
    }));
    cx.on_window_closed(detach).detach();
    cx.spawn(async move |cx| {
        while let Some(request) = receiver.next().await {
            if let Err(error) = cx.update(|cx| match request {
                MenuRequest::Activate(id) => dispatch(cx, &id),
                MenuRequest::Refresh => {
                    request_sync(cx);
                    Ok(())
                }
                MenuRequest::Appearance => sync_appearance(),
            }) {
                eprintln!("Could not run a Windows menu command: {error:#}");
            }
        }
    })
    .detach();
}

pub(crate) fn attach(window: &mut Window, cx: &mut App) {
    if let Err(error) = try_attach(window, cx) {
        eprintln!("Could not attach the Windows menu bar: {error:#}");
    }
}

fn try_attach(window: &mut Window, cx: &mut App) -> Result<()> {
    let hwnd = window_hwnd(window)?;
    appearance::for_window(hwnd);
    let menus = cx
        .get_menus()
        .context("no application menus were registered")?;
    let keymap = cx.key_bindings();
    let plan = MenuPlan::from_menus(menus, &keymap.borrow());
    let (menu, items) = build_native_menus(&plan)?;
    let mnemonics = menu_mnemonics(&plan);

    if let Some(previous) = ATTACHED.with(|attached| attached.borrow_mut().take()) {
        previous.remove();
    }

    // SAFETY: `hwnd` came from GPUI's live window handle. `menu` and its
    // children stay owned by ATTACHED on this UI thread for the window's
    // lifetime. The native subclass installed by muda is removed in `detach`.
    unsafe {
        // Our owner drawing supplies the bar colors as well as popup colors.
        // Disable muda's separate fixed-color UAH bar painter to avoid it
        // painting over the same items. High contrast uses system rendering.
        menu.init_for_hwnd_with_theme(hwnd, MenuTheme::Light)
            .context("muda could not attach the menu bar to the window")?;
        if SetWindowSubclass(
            hwnd as HWND,
            Some(keyboard_subclass),
            KEYBOARD_SUBCLASS_ID,
            0,
        ) == 0
        {
            let _ = menu.remove_for_hwnd(hwnd);
            anyhow::bail!("could not attach the native menu keyboard bridge");
        }
    }

    let drawing = match drawing::Menus::new(hwnd, &plan) {
        Ok(drawing) => Some(Rc::new(RefCell::new(drawing))),
        Err(error) => {
            eprintln!("Could not prepare menu drawing: {error:#}");
            None
        }
    };
    let enabled = vec![None; plan.commands.len()];
    ATTACHED.with(|attached| {
        *attached.borrow_mut() = Some(AttachedMenus {
            hwnd,
            menu,
            commands: plan.commands,
            items,
            enabled,
            mnemonics,
            sync_pending: false,
            drawing,
        });
    });

    if let Err(error) = sync_appearance() {
        eprintln!("Could not configure menu appearance: {error:#}");
    }
    request_sync(cx);
    Ok(())
}

fn menu_mnemonics(plan: &MenuPlan) -> Vec<usize> {
    let mut used = HashSet::new();
    plan.menus
        .iter()
        .filter_map(|section| {
            let label = mnemonic_label(&section.label, &mut used);
            label
                .chars()
                .skip_while(|character| *character != '&')
                .nth(1)
                .map(|character| character.to_ascii_uppercase() as usize)
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum KeyRoute {
    Forward,
    TranslateMnemonic,
    System,
    AltRelease,
}

fn key_route(
    message: u32,
    key: usize,
    control: bool,
    shift: bool,
    alt: bool,
    menu_key: bool,
) -> KeyRoute {
    if key == usize::from(VK_F10)
        && !control
        && !shift
        && !alt
        && matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP)
    {
        KeyRoute::System
    } else if message == WM_SYSKEYDOWN && alt && !control && menu_key {
        KeyRoute::TranslateMnemonic
    } else if message == WM_SYSKEYUP && key == usize::from(VK_MENU) && !control && !shift {
        KeyRoute::AltRelease
    } else {
        KeyRoute::Forward
    }
}

// SAFETY: installed and removed on the native window's UI thread. No borrowed
// GPUI state or pointers are held across native calls or the modal menu loop.
unsafe extern "system" fn keyboard_subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Clone only the drawing owner before calling Win32/GDI; never hold the
    // adapter's RefCell borrow across native callbacks or GPUI action dispatch.
    let drawing = ATTACHED.with(|attached| {
        attached.try_borrow().ok().and_then(|attached| {
            attached
                .as_ref()
                .and_then(|attached| attached.drawing.clone())
        })
    });
    if let Some(drawing) = drawing
        && let Ok(mut drawing) = drawing.try_borrow_mut()
        && let Some(result) = unsafe { drawing.handle(message, wparam, lparam) }
    {
        return result;
    }
    if message == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(hwnd, Some(keyboard_subclass), KEYBOARD_SUBCLASS_ID) };
    }
    if matches!(message, WM_SETTINGCHANGE | WM_THEMECHANGED | WM_DPICHANGED) {
        REQUESTS.with(|requests| {
            if let Some(sender) = requests.borrow().as_ref() {
                let _ = sender.unbounded_send(MenuRequest::Appearance);
            }
        });
    }
    if matches!(message, WM_INITMENUPOPUP | WM_EXITMENULOOP) {
        // Updating GPUI synchronously from a native callback can re-enter an
        // existing window borrow. Wake the UI task instead.
        REQUESTS.with(|requests| {
            if let Some(sender) = requests.borrow().as_ref() {
                let _ = sender.unbounded_send(MenuRequest::Refresh);
            }
        });
    }
    if message == WM_EXITMENULOOP {
        // The modal menu can consume Alt-up. Ask GPUI's native modifier-key
        // handler to sample GetKeyState after closing, without synthesizing a
        // character or activating the menu again through WM_SYSKEYUP. Posting
        // keeps this outside the native callback's potentially borrowed state.
        unsafe {
            PostMessageW(
                hwnd,
                WM_KEYUP,
                usize::from(VK_MENU),
                (3_u32 << 30) as LPARAM,
            );
        }
    }
    if matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP) {
        let menu_key = ATTACHED.with(|attached| {
            attached
                .try_borrow()
                .ok()
                .and_then(|attached| {
                    attached
                        .as_ref()
                        .map(|attached| attached.mnemonics.contains(&wparam))
                })
                .unwrap_or(false)
        });
        let (control, shift, alt) = unsafe {
            (
                GetKeyState(i32::from(VK_CONTROL)) < 0,
                GetKeyState(i32::from(VK_SHIFT)) < 0,
                GetKeyState(i32::from(VK_MENU)) < 0,
            )
        };
        match key_route(message, wparam, control, shift, alt, menu_key) {
            KeyRoute::TranslateMnemonic => {
                // GPUI's main message loop omits TranslateMessage and otherwise
                // offers Alt+letters to canvas text. Only translate our menu
                // mnemonics; leave AltGr and application shortcuts to GPUI.
                let msg = MSG {
                    hwnd,
                    message,
                    wParam: wparam,
                    lParam: lparam,
                    ..Default::default()
                };
                unsafe { TranslateMessage(&msg) };
                return 0;
            }
            KeyRoute::System => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            KeyRoute::AltRelease => {
                // The extended Alt key is AltGr on international layouts. Its
                // synthetic Ctrl may already be released; do not focus menus.
                if lparam & (1 << 24) != 0 {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                // Let GPUI reset its modifier/cursor state, then give Windows
                // the Alt key-up that GPUI otherwise consumes.
                unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            KeyRoute::Forward => {}
        }
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if matches!(message, WM_NCPAINT | WM_NCACTIVATE) {
        let drawing = ATTACHED.with(|attached| {
            attached.try_borrow().ok().and_then(|attached| {
                attached
                    .as_ref()
                    .and_then(|attached| attached.drawing.clone())
            })
        });
        if let Some(drawing) = drawing
            && let Ok(drawing) = drawing.try_borrow()
        {
            drawing.paint_frame_separator();
        }
    }
    result
}

fn window_hwnd(window: &Window) -> Result<isize> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| anyhow::anyhow!("the window has no native handle"))?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        anyhow::bail!("the window handle is not Win32");
    };
    Ok(handle.hwnd.get())
}

fn detach(_cx: &mut App) {
    if let Some(attached) = ATTACHED.with(|attached| attached.borrow_mut().take()) {
        attached.remove();
    }
}

pub(crate) fn request_sync(cx: &mut App) {
    let schedule = ATTACHED.with(|attached| {
        let mut attached = attached.borrow_mut();
        let Some(attached) = attached.as_mut() else {
            return false;
        };
        if attached.sync_pending {
            return false;
        }
        attached.sync_pending = true;
        true
    });
    if schedule {
        // A Workspace render calls this while its window is borrowed. Defer
        // until the completed frame's action tree can be queried safely.
        cx.defer(|cx| {
            ATTACHED.with(|attached| {
                if let Some(attached) = attached.borrow_mut().as_mut() {
                    attached.sync_pending = false;
                }
            });
            sync_enabled(cx);
        });
    }
}

fn sync_appearance() -> Result<()> {
    let current = ATTACHED.with(|attached| {
        attached.borrow().as_ref().map(|attached| {
            (
                attached.hwnd,
                attached.menu.clone(),
                attached.drawing.clone(),
            )
        })
    });
    let Some((hwnd, menu, drawing)) = current else {
        return Ok(());
    };
    let theme = appearance::for_window(hwnd);
    if let Some(drawing) = &drawing {
        // Restore before changing muda-owned fallback icons. The old HBITMAPs
        // must stop being referenced before muda replaces/frees them.
        drawing.borrow_mut().refresh(false)?;
    }
    let icons = ATTACHED.with(|attached| {
        attached
            .borrow()
            .as_ref()
            .map(|attached| {
                attached
                    .commands
                    .iter()
                    .zip(&attached.items)
                    .filter_map(|(command, item)| {
                        if let CommandItem::Icon(item) = item {
                            crate::menu_icons::for_action(command.action.as_ref())
                                .map(|name| (item.clone(), name))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let pixels =
        unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hwnd as HWND) }.max(96) / 6;
    let color = appearance::icon_color();
    for (item, icon) in icons {
        let result = crate::menu_icons::rasterize(icon, pixels, color)
            .and_then(|rgba| Ok(Icon::from_rgba(rgba, pixels, pixels)?));
        match result {
            Ok(icon) => item.set_icon(Some(icon)),
            Err(error) => eprintln!("Could not refresh menu icon: {error:#}"),
        }
    }
    let mut custom = false;
    if let Some(drawing) = drawing {
        let result = {
            let mut drawing = drawing.borrow_mut();
            drawing
                .capture_bitmaps()
                .and_then(|()| drawing.refresh(theme == MenuTheme::Dark))
        };
        match result {
            Ok(()) => custom = theme == MenuTheme::Dark,
            Err(error) => {
                eprintln!("Could not refresh menu drawing: {error:#}");
                // Drop restores any partially changed native items. Retain a
                // working native menu even when a GDI allocation fails.
                ATTACHED.with(|attached| {
                    if let Some(attached) = attached.borrow_mut().as_mut() {
                        attached.drawing = None;
                    }
                });
            }
        }
    }
    let native_theme = if custom { MenuTheme::Light } else { theme };
    unsafe { menu.set_theme_for_hwnd(hwnd, native_theme) }
        .context("could not refresh the native menu theme")?;
    // Both calls may synchronously measure/draw the menu; no drawing or adapter
    // borrow remains live here.
    unsafe { DrawMenuBar(hwnd as HWND) };
    Ok(())
}

fn sync_enabled(cx: &mut App) {
    let states = ATTACHED.with(|attached| {
        let attached = attached.borrow();
        let attached = attached.as_ref()?;
        Some(command_availability(&attached.commands, |action| {
            cx.is_action_available(action)
        }))
    });
    let Some(states) = states else {
        return;
    };

    let updates = ATTACHED.with(|attached| {
        let mut attached = attached.borrow_mut();
        let Some(attached) = attached.as_mut() else {
            return Vec::new();
        };
        let mut updates = Vec::new();
        for (index, enabled) in states.into_iter().enumerate() {
            if attached.enabled[index] != Some(enabled) {
                updates.push((attached.items[index].clone(), enabled));
                attached.enabled[index] = Some(enabled);
            }
        }
        updates
    });
    for (item, enabled) in updates {
        // muda redraws synchronously; drawing callbacks need to borrow ATTACHED.
        item.set_enabled(enabled);
    }
}

fn dispatch(cx: &mut App, id: &MenuId) -> Result<()> {
    let action = ATTACHED.with(|attached| {
        let attached = attached.borrow();
        let attached = attached.as_ref().context("the menu bar is not attached")?;
        let index = parse_command_id(&id.0).context("unknown menu command")?;
        let command = attached
            .commands
            .get(index)
            .context("unknown menu command")?;
        Ok::<_, anyhow::Error>(command.action.boxed_clone())
    })?;

    // Revalidate outside any window borrow; App::is_action_available refreshes
    // the active window internally.
    if !cx.is_action_available(action.as_ref()) {
        return Ok(());
    }

    let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return Ok(());
    };
    window.update(cx, |_, window: &mut Window, cx: &mut App| {
        window.dispatch_action(action, cx);
    })
}

fn parse_command_id(id: &str) -> Option<usize> {
    id.strip_prefix(COMMAND_ID_PREFIX)?.parse().ok()
}

fn accelerator_for(keymap: &Keymap, action: &dyn Action) -> Option<KeyAccelerator> {
    let binding = keymap
        .bindings_for_action(action)
        .find(|binding| binding.predicate().is_none())?;
    let keystroke = binding.keystrokes().first()?.inner();
    key_accelerator(keystroke)
}

fn key_accelerator(keystroke: &Keystroke) -> Option<KeyAccelerator> {
    let mut modifiers = Modifiers::empty();
    if keystroke.modifiers.control {
        modifiers |= Modifiers::CONTROL;
    }
    if keystroke.modifiers.alt {
        modifiers |= Modifiers::ALT;
    }
    if keystroke.modifiers.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if keystroke.modifiers.platform {
        modifiers |= Modifiers::META;
    }

    let key = match keystroke.key.as_str() {
        "enter" => Key::Named(NamedKey::Enter),
        "escape" => Key::Named(NamedKey::Escape),
        "backspace" => Key::Named(NamedKey::Backspace),
        "delete" => Key::Named(NamedKey::Delete),
        "tab" => Key::Named(NamedKey::Tab),
        "space" => Key::Character(" ".into()),
        "left" => Key::Named(NamedKey::ArrowLeft),
        "right" => Key::Named(NamedKey::ArrowRight),
        "up" => Key::Named(NamedKey::ArrowUp),
        "down" => Key::Named(NamedKey::ArrowDown),
        "home" => Key::Named(NamedKey::Home),
        "end" => Key::Named(NamedKey::End),
        "pageup" => Key::Named(NamedKey::PageUp),
        "pagedown" => Key::Named(NamedKey::PageDown),
        "insert" => Key::Named(NamedKey::Insert),
        key if key.chars().count() == 1 => Key::Character(key.into()),
        _ => return None,
    };
    Some(KeyAccelerator::new(modifiers, key))
}

fn build_native_menus(plan: &MenuPlan) -> Result<(NativeMenu, Vec<CommandItem>)> {
    let menu = NativeMenu::new();
    let mut top_level_mnemonics = HashSet::new();
    let mut items = Vec::with_capacity(plan.commands.len());
    for section in &plan.menus {
        let submenu = build_submenu(section, plan, &mut top_level_mnemonics, &mut items)?;
        menu.append(&submenu)?;
    }
    debug_assert_eq!(items.len(), plan.commands.len());
    Ok((menu, items))
}

fn build_submenu(
    section: &MenuSection,
    plan: &MenuPlan,
    name_mnemonics: &mut HashSet<char>,
    items: &mut Vec<CommandItem>,
) -> Result<NativeSubmenu> {
    let submenu = NativeSubmenu::new(mnemonic_label(&section.label, name_mnemonics), true);
    let mut item_mnemonics = HashSet::new();
    for node in &section.items {
        match node {
            MenuNode::Separator => {
                submenu.append(&PredefinedMenuItem::separator())?;
            }
            MenuNode::Submenu(child) => {
                let child = build_submenu(child, plan, &mut item_mnemonics, items)?;
                submenu.append(&child)?;
            }
            MenuNode::Command(index) => {
                let command = &plan.commands[*index];
                let id = format!("{COMMAND_ID_PREFIX}{index}");
                let label = mnemonic_label(&command.label, &mut item_mnemonics);
                let icon = crate::menu_icons::for_action(command.action.as_ref())
                    .map(|name| {
                        let rgba =
                            crate::menu_icons::rasterize(name, 16, appearance::icon_color())?;
                        Ok::<_, anyhow::Error>(Icon::from_rgba(rgba, 16, 16)?)
                    })
                    .transpose()?;
                let item = if let Some(icon) = icon {
                    let item = IconMenuItem::with_id(id, label, true, Some(icon), None);
                    item.set_key_accelerator(command.accelerator.clone())?;
                    submenu.append(&item)?;
                    CommandItem::Icon(item)
                } else {
                    let item = NativeMenuItem::with_id(id, label, true, None);
                    item.set_key_accelerator(command.accelerator.clone())?;
                    submenu.append(&item)?;
                    CommandItem::Text(item)
                };
                debug_assert_eq!(items.len(), *index);
                items.push(item);
            }
        }
    }
    Ok(submenu)
}

fn mnemonic_label(label: &str, used: &mut HashSet<char>) -> String {
    let mnemonic = label.chars().find(|character| {
        character.is_ascii_alphanumeric() && used.insert(character.to_ascii_lowercase())
    });

    let Some(mnemonic) = mnemonic else {
        return label.replace('&', "&&");
    };
    let mut output = String::new();
    let mut inserted = false;
    for character in label.chars() {
        if !inserted && character == mnemonic {
            output.push('&');
            inserted = true;
        }
        if character == '&' {
            output.push('&');
        }
        output.push(character);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        Command, Keymap, MenuNode, MenuPlan, command_availability, mnemonic_label, parse_command_id,
    };
    use crate::commands::{NewImage, SaveImage, SelectTool, Tool, Zoom, application_menus};
    use gpui::{KeyBinding, Menu, MenuItem};
    use std::collections::HashSet;

    #[test]
    fn menu_keyboard_bridge_reserves_only_native_navigation() {
        use super::*;
        let f = 'F' as usize;
        assert_eq!(
            key_route(WM_SYSKEYDOWN, f, false, false, true, true),
            KeyRoute::TranslateMnemonic
        );
        assert_eq!(
            key_route(WM_SYSKEYDOWN, f, true, false, true, true),
            KeyRoute::Forward,
            "AltGr stays with text input"
        );
        assert_eq!(
            key_route(WM_SYSKEYDOWN, 'O' as usize, false, false, true, false),
            KeyRoute::Forward,
            "Alt+O remains a color shortcut"
        );
        assert_eq!(
            key_route(WM_KEYDOWN, f, false, false, false, true),
            KeyRoute::Forward
        );
        for message in [WM_KEYDOWN, WM_KEYUP] {
            assert_eq!(
                key_route(message, usize::from(VK_F10), false, false, false, false),
                KeyRoute::System
            );
        }
        assert_eq!(
            key_route(
                WM_SYSKEYUP,
                usize::from(VK_MENU),
                false,
                false,
                false,
                false
            ),
            KeyRoute::AltRelease
        );
    }

    #[test]
    fn keyboard_mnemonics_match_the_native_top_level_labels() {
        let plan = plan(application_menus(false), &Keymap::default());
        let keys = super::menu_mnemonics(&plan);
        assert_eq!(keys.len(), plan.menus.len());
        assert_eq!(
            keys.iter().copied().collect::<HashSet<_>>().len(),
            keys.len()
        );
        assert_eq!(keys[0], 'F' as usize);
    }

    fn plan(menus: Vec<Menu>, keymap: &Keymap) -> MenuPlan {
        MenuPlan::from_menus(menus.into_iter().map(Menu::owned).collect(), keymap)
    }

    fn section_labels(plan: &MenuPlan) -> Vec<&str> {
        plan.menus.iter().map(|menu| menu.label.as_str()).collect()
    }

    fn find_command<'a>(plan: &'a MenuPlan, label: &str) -> &'a Command {
        plan.commands
            .iter()
            .find(|command| command.label == label)
            .unwrap_or_else(|| panic!("missing command {label}"))
    }

    #[test]
    fn conversion_preserves_the_shared_tree() {
        let plan = plan(application_menus(false), &Keymap::default());
        assert_eq!(
            section_labels(&plan),
            [
                "File", "Edit", "Select", "View", "Image", "Layer", "Tools", "Colors", "Filters",
                "Window", "Help",
            ]
        );

        let file = &plan.menus[0];
        assert_eq!(
            file.items[0],
            MenuNode::Command(0),
            "File starts with New Image"
        );
        assert_eq!(file.items[1], MenuNode::Command(1));
        assert!(matches!(file.items[2], MenuNode::Separator));
        assert!(
            find_command(&plan, "Exit")
                .action
                .partial_eq(&crate::commands::Quit)
        );
        assert!(
            find_command(&plan, "Check for Updates…")
                .action
                .partial_eq(&crate::commands::CheckForUpdates)
        );

        let view = plan.menus.iter().find(|menu| menu.label == "View").unwrap();
        let zoom = view
            .items
            .iter()
            .find_map(|item| match item {
                MenuNode::Submenu(menu) if menu.label == "Zoom" => Some(menu),
                _ => None,
            })
            .expect("View has a Zoom submenu");
        assert_eq!(
            zoom.items
                .iter()
                .filter(|item| matches!(item, MenuNode::Command(_)))
                .count(),
            8
        );
    }

    #[test]
    fn conversion_keeps_parameterized_actions() {
        let plan = plan(application_menus(false), &Keymap::default());
        assert!(
            find_command(&plan, "Crop")
                .action
                .partial_eq(&SelectTool(Tool::Crop))
        );
        assert!(
            find_command(&plan, "400%")
                .action
                .partial_eq(&Zoom::Percent(400.))
        );
        assert!(
            !find_command(&plan, "400%")
                .action
                .partial_eq(&Zoom::Percent(100.))
        );
        assert!(
            find_command(&plan, "Align Top")
                .action
                .partial_eq(&crate::commands::AlignLayer::Top)
        );
    }

    #[test]
    fn availability_marks_commands_that_have_no_handler() {
        let plan = plan(application_menus(false), &Keymap::default());
        let states = command_availability(&plan.commands, |action| action.partial_eq(&NewImage));
        let new_image = plan
            .commands
            .iter()
            .position(|command| command.action.partial_eq(&NewImage))
            .unwrap();
        let save = plan
            .commands
            .iter()
            .position(|command| command.action.partial_eq(&SaveImage))
            .unwrap();
        assert!(states[new_image]);
        assert!(!states[save]);
        assert_eq!(states.len(), plan.commands.len());
    }

    #[test]
    fn accelerator_labels_follow_the_global_key_bindings() {
        let keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-n", NewImage, None),
            KeyBinding::new("ctrl-shift-v", crate::commands::PasteAsLayer, None),
            KeyBinding::new("escape", crate::commands::CancelMove, Some("Move")),
        ]);
        let menus = vec![Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Image…", NewImage),
                MenuItem::separator(),
                MenuItem::action("Save", SaveImage),
                MenuItem::action("Paste as New Layer", crate::commands::PasteAsLayer),
            ],
        }];
        let plan = plan(menus, &keymap);
        assert_eq!(
            find_command(&plan, "New Image…")
                .accelerator
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("Ctrl+N")
        );
        assert_eq!(
            find_command(&plan, "Paste as New Layer")
                .accelerator
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("Ctrl+Shift+V")
        );
        assert!(find_command(&plan, "Save").accelerator.is_none());
    }

    #[test]
    fn mnemonics_are_unique_and_escape_existing_ampersands() {
        let mut used = HashSet::new();
        assert_eq!(mnemonic_label("File", &mut used), "&File");
        assert_eq!(mnemonic_label("Filters", &mut used), "F&ilters");
        assert_eq!(
            mnemonic_label("Brightness & Contrast…", &mut used),
            "&Brightness && Contrast…"
        );
    }

    #[test]
    fn command_ids_round_trip() {
        assert_eq!(parse_command_id("lime.command.12"), Some(12));
        assert_eq!(parse_command_id("other.12"), None);
        assert_eq!(parse_command_id("lime.command.nope"), None);
    }
}
