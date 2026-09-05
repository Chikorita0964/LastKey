#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, TRUE, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextW, GetWindowThreadProcessId, ICON_SMALL, IMAGE_ICON,
            IsWindowVisible, LR_DEFAULTCOLOR, LR_SHARED, LoadImageW, SWP_FRAMECHANGED, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos, WM_SETICON,
        },
    },
    core::{BOOL, PCWSTR},
};

#[cfg(windows)]
fn main() -> iced::Result {
    apply_window_icon();
    let Some(_single_instance) = SettingsSingleInstance::acquire() else {
        return Ok(());
    };
    lastkey::ui::run()
}

#[cfg(windows)]
struct SettingsSingleInstance(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl SettingsSingleInstance {
    fn acquire() -> Option<Self> {
        use windows::{
            Win32::{
                Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
                System::Threading::CreateMutexW,
            },
            core::w,
        };

        let handle =
            unsafe { CreateMutexW(None, true, w!("Local\\LastKey.Settings.SingleInstance")) }
                .ok()?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }
        Some(Self(handle))
    }
}

#[cfg(windows)]
impl Drop for SettingsSingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Paints the embedded application icon onto the title bar and taskbar.
///
/// winit registers its window class without icons and iced exposes no icon
/// API, so Windows falls back to the generic application glyph. Sending the
/// exe's own icon as the small icon replaces that fallback in both places.
/// Runs on a throwaway thread that exits once our windows are found (or after
/// a timeout) and never fails the app.
#[cfg(windows)]
fn apply_window_icon() {
    std::thread::Builder::new()
        .name("lastkey-window-icon".into())
        .spawn(|| {
            let Some(icon) = app_small_icon() else {
                return;
            };
            let icon_bits = icon.0 as isize;

            for _ in 0..100 {
                unsafe {
                    let _ = EnumWindows(Some(paint_matching_window), LPARAM(icon_bits));
                }
                if ICON_APPLIED.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
        .ok();
}

/// Loads the embedded application icon (winres id 1) at title-bar size.
/// Returns `None` when the resource is missing, leaving the window as is.
/// `LR_SHARED` keeps the handle owned by the system, so nothing leaks.
#[cfg(windows)]
fn app_small_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let handle = LoadImageW(
            Some(HINSTANCE(module.0)),
            // Resource id 1, where winres places the application icon. This
            // is an id, not a dereferenceable pointer.
            PCWSTR(std::ptr::without_provenance::<u16>(1)),
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR | LR_SHARED,
        )
        .ok()?;
        Some(windows::Win32::UI::WindowsAndMessaging::HICON(handle.0))
    }
}

#[cfg(windows)]
static ICON_APPLIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// [`EnumWindows`] callback: paints the small icon of every visible top-level
/// window owned by this process whose title starts with `LastKey`, then
/// repaints the frame. Big icons (Alt-Tab) resolve from the exe on their own.
#[cfg(windows)]
unsafe extern "system" fn paint_matching_window(hwnd: HWND, icon_bits: LPARAM) -> BOOL {
    unsafe {
        if icon_bits.0 == 0 || !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut owner as *mut u32));
        if owner != GetCurrentProcessId() {
            return TRUE;
        }
        let mut title = [0u16; 256];
        let length = GetWindowTextW(hwnd, &mut title) as usize;
        if !String::from_utf16_lossy(&title[..length.min(title.len())]).starts_with("LastKey") {
            return TRUE;
        }
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(icon_bits.0)),
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
        ICON_APPLIED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    TRUE
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LastKey Settings is currently available on Windows only.");
}
