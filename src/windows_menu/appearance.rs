//! Native menu appearance, isolated from the editor's drawing and input code.

use std::{cell::Cell, mem, sync::OnceLock};

use muda::MenuTheme;
use windows_sys::Win32::{
    Foundation::{FreeLibrary, HWND},
    System::{
        LibraryLoader::{
            GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
        },
        SystemInformation::OSVERSIONINFOW,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
    },
};

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, bool) -> bool;
type FlushMenuThemes = unsafe extern "system" fn();
type NativeProc = unsafe extern "system" fn() -> isize;

struct DarkMenuApi {
    set_mode: SetPreferredAppMode,
    allow_window: AllowDarkModeForWindow,
    flush: Option<FlushMenuThemes>,
}

static DARK_MENU_API: OnceLock<Option<DarkMenuApi>> = OnceLock::new();
thread_local! {
    static DARK_ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
}

fn supports_app_mode(major: u32, build: u32) -> bool {
    // Ordinal 135 was a different function before Windows 10 1903. Never call
    // it with the newer signature on those systems, even if it is exported.
    major >= 10 && build >= 18362
}

fn load_dark_menu_api() -> Option<DarkMenuApi> {
    // These compatibility exports are also used by Chromium/PowerToys. Probe
    // them dynamically, keep their ABI/version checks here, and retain normal
    // system menus if unavailable. No system-wide theme setting is changed.
    // https://chromium.googlesource.com/chromium/src/+/HEAD/base/win/dark_mode_support.cc
    unsafe {
        let ntdll = GetModuleHandleW(windows_sys::w!("ntdll.dll"));
        if ntdll.is_null() {
            return None;
        }
        let version_fn = GetProcAddress(ntdll, c"RtlGetVersion".as_ptr().cast())?;
        let version_fn: unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32 =
            mem::transmute(version_fn);
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        if version_fn(&mut version) != 0
            || !supports_app_mode(version.dwMajorVersion, version.dwBuildNumber)
        {
            return None;
        }
        let module = LoadLibraryExW(
            windows_sys::w!("uxtheme.dll"),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            return None;
        }
        let api = (|| {
            Some(DarkMenuApi {
                set_mode: mem::transmute::<NativeProc, SetPreferredAppMode>(GetProcAddress(
                    module,
                    135_usize as _,
                )?),
                allow_window: mem::transmute::<NativeProc, AllowDarkModeForWindow>(GetProcAddress(
                    module,
                    133_usize as _,
                )?),
                flush: GetProcAddress(module, 136_usize as _)
                    .map(|function| mem::transmute::<NativeProc, FlushMenuThemes>(function)),
            })
        })();
        if api.is_none() {
            FreeLibrary(module);
        }
        // On success the library reference intentionally lasts for the process
        // lifetime, just like the cached function pointers that refer to it.
        api
    }
}

fn high_contrast() -> Option<bool> {
    let mut settings = HIGHCONTRASTW {
        cbSize: mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: settings is a writable HIGHCONTRASTW of the advertised size.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            settings.cbSize,
            (&mut settings as *mut HIGHCONTRASTW).cast(),
            0,
        )
    };
    (read != 0).then_some(settings.dwFlags & HCF_HIGHCONTRASTON != 0)
}

fn request_dark(contrast: Option<bool>) -> bool {
    contrast == Some(false)
}

pub(super) fn configure_app() {
    let Some(api) = DARK_MENU_API.get_or_init(load_dark_menu_api).as_ref() else {
        return;
    };
    let dark = request_dark(high_contrast());
    let changed = DARK_ENABLED.with(|enabled| enabled.replace(Some(dark)) != Some(dark));
    if changed {
        // SAFETY: API availability and the ordinal's signature were verified.
        // ForceDark=2 affects this process's native popups; Default=0 leaves
        // accessibility/high-contrast rendering to Windows.
        unsafe {
            (api.set_mode)(if dark { 2 } else { 0 });
            if let Some(flush) = api.flush {
                flush();
            }
        }
    }
}

pub(super) fn for_window(hwnd: isize) -> MenuTheme {
    configure_app();
    let dark = DARK_ENABLED.with(|enabled| enabled.get().unwrap_or(false));
    if let Some(api) = DARK_MENU_API.get_or_init(load_dark_menu_api).as_ref() {
        // SAFETY: called by the adapter with its live HWND on the UI thread.
        unsafe {
            (api.allow_window)(hwnd as HWND, dark);
        }
    }
    if dark {
        MenuTheme::Dark
    } else {
        MenuTheme::Auto
    }
}

pub(super) fn icon_color() -> u32 {
    if DARK_ENABLED.with(|enabled| enabled.get() == Some(true)) {
        crate::theme::TEXT
    } else {
        let color = unsafe {
            windows_sys::Win32::Graphics::Gdi::GetSysColor(
                windows_sys::Win32::Graphics::Gdi::COLOR_MENUTEXT,
            )
        };
        ((color & 0xff) << 16) | (color & 0xff00) | ((color >> 16) & 0xff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_mode_ordinal_requires_the_modern_signature() {
        assert!(!supports_app_mode(10, 17763));
        assert!(!supports_app_mode(6, 9600));
        assert!(supports_app_mode(10, 18362));
        assert!(supports_app_mode(10, 26100));
    }

    #[test]
    fn accessibility_and_unknown_settings_keep_system_rendering() {
        assert!(request_dark(Some(false)));
        assert!(!request_dark(Some(true)));
        assert!(!request_dark(None));
    }
}
