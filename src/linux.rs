//! Terminal diagnostics for Linux window-system and GPU failures.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

pub(crate) fn init() {
    static LOGGER: StderrLogger = StderrLogger;
    let level = std::env::var("LIME_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(log::LevelFilter::Warn);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}

pub(crate) fn decorations() -> gpui::WindowDecorations {
    // GPUI 0.2.2 records a request for server decorations even when a Wayland
    // compositor does not implement the decoration protocol. Explicitly request
    // client decorations there so GNOME/Weston windows still get usable controls.
    if gpui::guess_compositor() == "Wayland" {
        gpui::WindowDecorations::Client
    } else {
        gpui::WindowDecorations::Server
    }
}
