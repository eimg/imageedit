use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSCursor, NSImage};
use objc2_foundation::NSData;

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
