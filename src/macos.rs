use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSCursor, NSImage, NSMenu};
use objc2_foundation::{NSData, NSSize};

pub fn set_dock_icon() {
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../macos/AppIcon.icns"));
    if let Some(icon) = NSImage::initWithData(main_thread.alloc(), &data) {
        // AppKit is running on the main thread, and the icon argument is non-null.
        unsafe {
            NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&icon))
        };
    }
}

pub fn set_cursor_visible(visible: bool) {
    if visible {
        NSCursor::unhide();
    } else {
        NSCursor::hide();
    }
}

pub fn set_menu_icons() {
    use gpui::MenuItem;
    use image::ImageEncoder as _;

    fn decorate(menu: &NSMenu, items: &[MenuItem], mtm: MainThreadMarker) {
        let native_items = menu.itemArray();
        for item in items {
            let title = match item {
                MenuItem::Action { name, .. } => name.to_string(),
                MenuItem::Submenu(menu) => menu.name.to_string(),
                _ => continue,
            };
            let Some(native) = native_items
                .iter()
                .find(|item| item.title().to_string() == title)
            else {
                continue;
            };
            match item {
                MenuItem::Submenu(menu) => {
                    if let Some(submenu) = native.submenu() {
                        decorate(&submenu, &menu.items, mtm);
                    }
                }
                MenuItem::Action { action, .. } => {
                    let Some(icon) = crate::menu_icons::for_action(action.as_ref()) else {
                        continue;
                    };
                    let result = (|| -> anyhow::Result<()> {
                        // A 2x template image lets AppKit tint normal, selected,
                        // and disabled items for the current system appearance.
                        let pixels = crate::menu_icons::rasterize(icon, 32, 0)?;
                        let mut png = Vec::new();
                        image::codecs::png::PngEncoder::new(&mut png).write_image(
                            &pixels,
                            32,
                            32,
                            image::ExtendedColorType::Rgba8,
                        )?;
                        let data = NSData::with_bytes(&png);
                        let image = NSImage::initWithData(mtm.alloc(), &data)
                            .ok_or_else(|| anyhow::anyhow!("could not load menu icon"))?;
                        image.setSize(NSSize::new(16., 16.));
                        image.setTemplate(true);
                        native.setImage(Some(&image));
                        Ok(())
                    })();
                    if let Err(error) = result {
                        eprintln!("Could not set menu icon: {error:#}");
                    }
                }
                _ => {}
            }
        }
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(main) = NSApplication::sharedApplication(mtm).mainMenu() else {
        return;
    };
    // Match within each submenu rather than by global labels: "Crop to Content"
    // occurs under both Image and Layer. Leave system menus and bar titles alone.
    for menu in crate::commands::application_menus(true) {
        if let Some(native) = main
            .itemArray()
            .iter()
            .find(|item| item.title().to_string() == menu.name.as_ref())
            && let Some(submenu) = native.submenu()
        {
            decorate(&submenu, &menu.items, mtm);
        }
    }
}

/// Label the Metal backing surface as sRGB so Core Animation can convert it
/// to the display's profile, including when the window moves between screens.
/// GPUI supplies straight BGRA bytes in the editor's sRGB working coordinates.
pub fn set_srgb_surface(window: &gpui::Window) {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        static kCGColorSpaceSRGB: *const c_void;
        fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
        fn CGColorSpaceRelease(space: *mut c_void);
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: GPUI owns this live NSView, and window creation runs on the main
    // thread. Its makeBackingLayer returns a CAMetalLayer. The colorspace
    // property retains the Core Graphics object before our reference is released.
    unsafe {
        let view = handle.ns_view.as_ptr().cast::<AnyObject>();
        let layer: *mut AnyObject = msg_send![view, layer];
        if layer.is_null() {
            return;
        }
        let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        if space.is_null() {
            return;
        }
        let _: () = msg_send![layer, setColorspace: space];
        CGColorSpaceRelease(space);
    }
}
