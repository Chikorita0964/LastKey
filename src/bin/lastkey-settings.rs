#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() -> iced::Result {
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

#[cfg(not(windows))]
fn main() {
    eprintln!("LastKey Settings is currently available on Windows only.");
}
