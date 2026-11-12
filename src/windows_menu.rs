//! Native Windows menu bar built from GPUI's canonical menu tree.
//!
//! GPUI 0.2.2 stores application menus but does not display them on Windows.
//! This module converts the stored tree into `muda` menus, attaches them to the
//! window's HWND, keeps their native owners alive on the UI thread, and routes
//! activation back to GPUI actions after the Win32 callback has returned.

use std::{cell::RefCell, collections::HashSet};

use anyhow::{Context as _, Result};
use futures::{StreamExt, channel::mpsc};
use gpui::{Action, App, Keymap, Keystroke, OwnedMenu, OwnedMenuItem, Window};
use muda::{
    Menu as NativeMenu, MenuEvent, MenuId, MenuItem as NativeMenuItem, PredefinedMenuItem,
    Submenu as NativeSubmenu,
    accelerator::{Key, KeyAccelerator, Modifiers, NamedKey},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const COMMAND_ID_PREFIX: &str = "lime.command.";

thread_local! {
    static ATTACHED: RefCell<Option<AttachedMenus>> = const { RefCell::new(None) };
}

struct AttachedMenus {
    hwnd: isize,
    menu: NativeMenu,
    commands: Vec<Command>,
    items: Vec<NativeMenuItem>,
    enabled: Vec<Option<bool>>,
}

impl AttachedMenus {
    fn remove(self) {
        unsafe {
            let _ = self.menu.remove_for_hwnd(self.hwnd);
        }
    }
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
    let (sender, mut receiver) = mpsc::unbounded::<MenuId>();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = sender.unbounded_send(event.id);
    }));
    cx.on_window_closed(detach).detach();
    cx.spawn(async move |cx| {
        while let Some(id) = receiver.next().await {
            if cx.update(|cx| dispatch(cx, &id)).is_err() {
                break;
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
    let menus = cx
        .get_menus()
        .context("no application menus were registered")?;
    let keymap = cx.key_bindings();
    let plan = MenuPlan::from_menus(menus, &keymap.borrow());
    let (menu, items) = build_native_menus(&plan)?;

    if let Some(previous) = ATTACHED.with(|attached| attached.borrow_mut().take()) {
        previous.remove();
    }

    // SAFETY: `hwnd` came from GPUI's live window handle. `menu` and its
    // children stay owned by ATTACHED on this UI thread for the window's
    // lifetime. The native subclass installed by muda is removed in `detach`.
    unsafe {
        menu.init_for_hwnd(hwnd)
            .context("muda could not attach the menu bar to the window")?;
    }

    let enabled = vec![None; plan.commands.len()];
    ATTACHED.with(|attached| {
        *attached.borrow_mut() = Some(AttachedMenus {
            hwnd,
            menu,
            commands: plan.commands,
            items,
            enabled,
        });
    });

    schedule_sync(window);
    Ok(())
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

fn schedule_sync(window: &mut Window) {
    window.on_next_frame(|window, cx| {
        cx.defer(sync_enabled);
        schedule_sync(window);
    });
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

    ATTACHED.with(|attached| {
        let mut attached = attached.borrow_mut();
        let Some(attached) = attached.as_mut() else {
            return;
        };
        for (index, enabled) in states.into_iter().enumerate() {
            if attached.enabled[index] == Some(enabled) {
                continue;
            }
            attached.items[index].set_enabled(enabled);
            attached.enabled[index] = Some(enabled);
        }
    });
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

fn build_native_menus(plan: &MenuPlan) -> Result<(NativeMenu, Vec<NativeMenuItem>)> {
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
    items: &mut Vec<NativeMenuItem>,
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
                let item = NativeMenuItem::with_id(
                    format!("{COMMAND_ID_PREFIX}{index}"),
                    mnemonic_label(&command.label, &mut item_mnemonics),
                    true,
                    None,
                );
                item.set_key_accelerator(command.accelerator.clone())?;
                submenu.append(&item)?;
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
                "Window",
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
