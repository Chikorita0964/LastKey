#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_runtime {
    use std::{
        io,
        path::PathBuf,
        process::{Child, Command},
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use lastkey::{
        app::{AppController, FileSettingsStore},
        platform::windows::{HOOK_STATUS_MESSAGE, InputService, UiServer},
        protocol::UiView,
        settings::{self, Settings},
    };
    use tray_icon::{
        Icon, TrayIcon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem},
    };
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
            System::Threading::{CreateMutexW, GetCurrentThreadId},
            UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage},
        },
        core::{HSTRING, w},
    };

    struct SingleInstance(HANDLE);

    impl SingleInstance {
        fn acquire() -> Result<Self, String> {
            let handle = unsafe { CreateMutexW(None, true, w!("Local\\LastKey.SingleInstance")) }
                .map_err(|error| error.to_string())?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err("Another LastKey instance is already running.".into());
            }
            Ok(Self(handle))
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn run() -> Result<(), String> {
        let _single_instance = SingleInstance::acquire()?;
        let settings = settings::load().unwrap_or_else(|error| {
            show_error("Settings Load Error", &error.to_string());
            Settings::default()
        });
        let main_thread = unsafe { GetCurrentThreadId() };
        let input = InputService::start(settings.clone(), main_thread)
            .map_err(|error| error.to_string())?;
        let controller = Arc::new(Mutex::new(AppController::new(
            settings,
            FileSettingsStore,
            input,
        )));
        let ui_server =
            UiServer::start(Arc::clone(&controller)).map_err(|error| error.to_string())?;
        let tray = create_tray()?;
        let mut settings_process = SettingsProcess::default();

        run_message_loop(&ui_server, &tray, &mut settings_process)?;

        let _ = ui_server.notify_shutdown();
        settings_process.shutdown();
        drop(ui_server);
        drop(controller);
        Ok(())
    }

    fn run_message_loop(
        ui_server: &UiServer,
        tray: &TrayIcon,
        settings_process: &mut SettingsProcess,
    ) -> Result<(), String> {
        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 == -1 {
                return Err(io::Error::last_os_error().to_string());
            }
            if result.0 == 0 {
                return Ok(());
            }
            // Thread messages carry a null window handle; tray notifications
            // arrive as window messages, so the null check keeps the two
            // apart before dispatch would silently drop ours.
            if message.hwnd.0.is_null() && message.message == HOOK_STATUS_MESSAGE {
                let lost = message.wParam.0 != 0;
                let _ = tray.set_tooltip(Some(if lost {
                    "LastKey — input hook unavailable"
                } else {
                    "LastKey"
                }));
                continue;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            while let Ok(event) = MenuEvent::receiver().try_recv() {
                match event.id.as_ref() {
                    "lastkey-settings" => {
                        // The settings window is optional; its failure must
                        // not stop the input filter message loop.
                        if let Err(error) = settings_process.open(UiView::Settings, ui_server) {
                            show_error("Settings Error", &error);
                        }
                    }
                    "lastkey-exit" => return Ok(()),
                    _ => {}
                }
            }
        }
    }

    fn create_tray() -> Result<TrayIcon, String> {
        let menu = Menu::new();
        for item in [
            MenuItem::with_id("lastkey-settings", "Settings", true, None),
            MenuItem::with_id("lastkey-exit", "Exit", true, None),
        ] {
            menu.append(&item).map_err(|error| error.to_string())?;
        }
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(tray_icon_image()?)
            .with_tooltip("LastKey")
            .build()
            .map_err(|error| error.to_string())
    }

    fn tray_icon_image() -> Result<Icon, String> {
        // build.rs embeds assets/source/lastkey-logo.ico via winres with
        // resource ID 1, so prefer it for consistent branding. Fall back to
        // the hand-drawn placeholder only if the embedded resource is missing.
        if let Ok(icon) = Icon::from_resource(1, None) {
            return Ok(icon);
        }
        placeholder_icon()
    }

    fn placeholder_icon() -> Result<Icon, String> {
        let mut pixels = vec![0_u8; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let offset = (y * 32 + x) * 4;
                let border = !(3..29).contains(&x) || !(3..29).contains(&y);
                let (red, green, blue) = if border {
                    (25, 88, 156)
                } else {
                    (53, 132, 228)
                };
                pixels[offset] = red;
                pixels[offset + 1] = green;
                pixels[offset + 2] = blue;
                pixels[offset + 3] = 255;
            }
        }
        Icon::from_rgba(pixels, 32, 32).map_err(|error| error.to_string())
    }

    #[derive(Default)]
    struct SettingsProcess {
        child: Option<Child>,
    }

    impl SettingsProcess {
        fn open(&mut self, view: UiView, ui_server: &UiServer) -> Result<(), String> {
            // A settings process is already connected, regardless of who started it.
            if ui_server.request_focus(view) {
                return Ok(());
            }
            // A process we started may not have connected yet; do not start a second one.
            if let Some(child) = self.child.as_mut() {
                match child.try_wait() {
                    Ok(None) => return Ok(()),
                    Ok(Some(_)) => self.child = None,
                    Err(error) => return Err(error.to_string()),
                }
            }

            let executable = settings_executable_path().map_err(|error| error.to_string())?;
            let view_name = match view {
                UiView::Settings => "settings",
                UiView::Measurement => "measurement",
            };
            self.child = Some(
                Command::new(&executable)
                    .arg("--view")
                    .arg(view_name)
                    .spawn()
                    .map_err(|error| {
                        format!(
                            "failed to start the settings process at {}: {error}",
                            executable.display()
                        )
                    })?,
            );
            Ok(())
        }

        fn shutdown(&mut self) {
            let Some(child) = self.child.as_mut() else {
                return;
            };
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        return;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(25)),
                    Err(_) => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            self.child = None;
        }
    }

    fn settings_executable_path() -> io::Result<PathBuf> {
        let executable = std::env::current_exe()?;
        let packaged = executable.with_file_name("LastKey.Settings.exe");
        if packaged.is_file() {
            return Ok(packaged);
        }
        let development = executable.with_file_name("lastkey-settings.exe");
        if development.is_file() {
            return Ok(development);
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "settings executable was not found at {} or {}",
                packaged.display(),
                development.display()
            ),
        ))
    }

    fn show_error(title: &str, message: &str) {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

        let title = HSTRING::from(title);
        let message = HSTRING::from(message);
        unsafe {
            let _ = MessageBoxW(None, &message, &title, MB_ICONERROR | MB_OK);
        }
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_runtime::run() {
        use windows::{
            Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
            core::{HSTRING, w},
        };

        let message = HSTRING::from(format!("LastKey failed to start: {error}"));
        unsafe {
            let _ = MessageBoxW(None, &message, w!("LastKey"), MB_ICONERROR | MB_OK);
        }
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    #[cfg(target_os = "linux")]
    {
        use lastkey::{
            platform::linux::InputService,
            settings::{self, Settings},
        };
        let settings = settings::load().unwrap_or_else(|error| {
            eprintln!("LastKey settings could not be loaded: {error}");
            Settings::default()
        });
        match InputService::start(settings) {
            Ok(_service) => {
                eprintln!("LastKey Linux backend is active. Press Ctrl+C to stop.");
                loop {
                    std::thread::park();
                }
            }
            Err(error) => {
                eprintln!("LastKey Linux input support is unavailable: {error}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("LastKey input support is not available on this platform.");
        std::process::exit(1);
    }
}
