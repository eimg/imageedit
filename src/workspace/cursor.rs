use std::cell::Cell;

use gpui::{Pixels, Point};

#[derive(Default)]
pub(super) struct CanvasCursor {
    pub position: Cell<Option<Point<Pixels>>>,
    #[cfg(target_os = "macos")]
    hidden: Cell<bool>,
}

impl CanvasCursor {
    pub fn leave(&self) {
        self.position.set(None);
        self.show_native(true);
    }

    #[cfg(target_os = "macos")]
    pub fn show_native(&self, show: bool) {
        // NSCursor hide calls stack. Balance the one owned by this canvas even
        // when the window deactivates or closes without another mouse move.
        if self.hidden.replace(!show) != !show {
            crate::macos::set_cursor_visible(show);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn show_native(&self, _: bool) {}
}

impl Drop for CanvasCursor {
    fn drop(&mut self) {
        self.show_native(true);
    }
}
