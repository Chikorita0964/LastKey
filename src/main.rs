#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use std::{cell::RefCell, rc::Rc, sync::mpsc::Receiver, time::Duration};

    use lastkey::{
        core::{LogicalKey, PhysicalKey},
        platform::windows::{CapturedKey, InputService, MeasurementUpdate},
        settings::{self, Settings},
    };
    use slint::{ComponentHandle, Timer, TimerMode};
    use tray_icon::{
        Icon, TrayIcon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem},
    };

    slint::include_modules!();

    struct PendingCapture {
        key: LogicalKey,
        receiver: Receiver<CapturedKey>,
    }

    struct UiState {
        saved: Settings,
        working: Settings,
        capture: Option<PendingCapture>,
        measurement: Option<Receiver<MeasurementUpdate>>,
    }

    pub fn run() -> Result<(), String> {
        let settings = settings::load().unwrap_or_else(|error| {
            eprintln!("LastKey settings could not be loaded: {error}");
            Settings::default()
        });
        let input =
            Rc::new(InputService::start(settings.clone()).map_err(|error| error.to_string())?);
        let window = MainWindow::new().map_err(|error| error.to_string())?;
        update_window(&window, &settings);
        let state = Rc::new(RefCell::new(UiState {
            saved: settings.clone(),
            working: settings,
            capture: None,
            measurement: None,
        }));

        configure_callbacks(&window, Rc::clone(&input), Rc::clone(&state));
        let capture_timer = configure_capture_poller(&window, Rc::clone(&state));
        let tray_timer = configure_tray(&window, Rc::clone(&input), Rc::clone(&state));
        configure_close_behavior(&window);
        window.show().map_err(|error| error.to_string())?;
        slint::run_event_loop_until_quit().map_err(|error| error.to_string())?;
        window.hide().map_err(|error| error.to_string())?;
        drop(tray_timer);
        drop(capture_timer);
        drop(window);
        drop(state);
        drop(input);
        Ok(())
    }

    fn configure_callbacks(
        window: &MainWindow,
        input: Rc<InputService>,
        state: Rc<RefCell<UiState>>,
    ) {
        let weak = window.as_weak();
        let input_for_capture = Rc::clone(&input);
        let state_for_capture = Rc::clone(&state);
        window.on_capture_key(move |index| {
            let Some(key) = logical_key(index) else {
                set_status(&weak, "Unknown key field.");
                return;
            };
            match input_for_capture.capture_next() {
                Ok(receiver) => {
                    state_for_capture.borrow_mut().capture = Some(PendingCapture { key, receiver });
                    set_status(&weak, "Press a key...");
                }
                Err(error) => set_status(&weak, &error.to_string()),
            }
        });

        let weak = window.as_weak();
        let input_for_apply = Rc::clone(&input);
        let state_for_apply = Rc::clone(&state);
        window.on_apply_settings(move || {
            let mut settings = state_for_apply.borrow().working.clone();
            let Some(window) = weak.upgrade() else {
                return;
            };
            settings.timing.transition_min_ms = window.get_transition_min_ms() as u32;
            settings.timing.transition_max_ms = window.get_transition_max_ms() as u32;
            settings.timing.overlap_min_ms = window.get_overlap_min_ms() as u32;
            settings.timing.overlap_max_ms = window.get_overlap_max_ms() as u32;
            settings.timing.overlap_probability = window.get_overlap_probability() as u8;
            settings.timing.full_overlap = window.get_full_overlap();
            if let Err(error) = settings.validate() {
                set_status(&weak, &error.to_string());
                return;
            }
            if let Err(error) = settings::save(&settings) {
                set_status(&weak, &error.to_string());
                return;
            }
            if let Err(error) = input_for_apply.apply(settings.clone()) {
                set_status(&weak, &error.to_string());
                return;
            }
            let mut state = state_for_apply.borrow_mut();
            state.saved = settings;
            state.measurement = None;
            if let Some(window) = weak.upgrade() {
                window.set_measurement_active(false);
                window.set_measurement_summary(
                    "Measurement stopped because settings changed.".into(),
                );
                window.set_measurement_recommendation("".into());
            }
            set_status(&weak, "Settings applied.");
        });

        let weak = window.as_weak();
        let state_for_cancel = Rc::clone(&state);
        window.on_cancel_settings(move || {
            let mut state = state_for_cancel.borrow_mut();
            state.working = state.saved.clone();
            if let Some(window) = weak.upgrade() {
                update_window(&window, &state.working);
            }
        });

        let weak = window.as_weak();
        let state_for_defaults = Rc::clone(&state);
        window.on_restore_defaults(move || {
            let mut state = state_for_defaults.borrow_mut();
            state.working = Settings::default();
            if let Some(window) = weak.upgrade() {
                update_window(&window, &state.working);
                window
                    .set_status("Default mappings restored. Select Apply to activate them.".into());
            }
        });

        let weak = window.as_weak();
        let input_for_measurement = Rc::clone(&input);
        let state_for_measurement = Rc::clone(&state);
        window.on_toggle_measurement(move || {
            let mut state = state_for_measurement.borrow_mut();
            if state.measurement.is_some() {
                match input_for_measurement.stop_measurement() {
                    Ok(()) => {
                        state.measurement = None;
                        if let Some(window) = weak.upgrade() {
                            window.set_measurement_active(false);
                            window.set_measurement_summary(
                                "Measurement stopped. Results were kept only for this session."
                                    .into(),
                            );
                        }
                    }
                    Err(error) => set_status(&weak, &error.to_string()),
                }
            } else {
                match input_for_measurement.start_measurement() {
                    Ok(receiver) => {
                        state.measurement = Some(receiver);
                        if let Some(window) = weak.upgrade() {
                            window.set_measurement_active(true);
                            window.set_measurement_summary(
                                "Measuring configured physical key transitions...".into(),
                            );
                            window.set_measurement_recommendation("".into());
                        }
                    }
                    Err(error) => set_status(&weak, &error.to_string()),
                }
            }
        });
    }

    fn configure_capture_poller(window: &MainWindow, state: Rc<RefCell<UiState>>) -> Timer {
        let weak = window.as_weak();
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let captured = {
                let mut state = state.borrow_mut();
                let Some(capture) = state.capture.as_ref() else {
                    return;
                };
                match capture.receiver.try_recv() {
                    Ok(captured) => Some((capture.key, captured)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        state.capture = None;
                        set_status(&weak, "Key capture was cancelled.");
                        None
                    }
                }
            };
            if let Some((key, captured)) = captured {
                let mut state = state.borrow_mut();
                state.working.set_binding(key, captured.physical);
                state.capture = None;
                if let Some(window) = weak.upgrade() {
                    set_key_name(&window, key, &captured.name);
                    window.set_status("Select Apply to activate the new mapping.".into());
                }
            }

            let update = {
                let state = state.borrow();
                state
                    .measurement
                    .as_ref()
                    .and_then(|receiver| receiver.try_iter().last())
            };
            if let Some(update) = update
                && let Some(window) = weak.upgrade()
            {
                let statistics = update.statistics;
                window.set_measurement_summary(
                    format!(
                        "{} samples: {} transitions, {} overlaps.",
                        statistics.sample_count(),
                        statistics.transition_count(),
                        statistics.overlap_count()
                    )
                    .into(),
                );
                let transition = update
                    .recommendation
                    .transition_micros
                    .map(format_millis)
                    .unwrap_or_else(|| "—".into());
                let overlap = update
                    .recommendation
                    .overlap_micros
                    .map(format_millis)
                    .unwrap_or_else(|| "—".into());
                window.set_measurement_recommendation(
                    format!("Suggested averages — transition: {transition}; overlap: {overlap}.")
                        .into(),
                );
            }
        });
        timer
    }

    fn configure_close_behavior(window: &MainWindow) {
        let weak = window.as_weak();
        window.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }

    fn configure_tray(
        window: &MainWindow,
        input: Rc<InputService>,
        state: Rc<RefCell<UiState>>,
    ) -> Timer {
        let weak = window.as_weak();
        let tray = Rc::new(RefCell::new(None));
        let tray_for_timer = Rc::clone(&tray);
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if tray_for_timer.borrow().is_none() {
                match create_tray() {
                    Ok(icon) => *tray_for_timer.borrow_mut() = Some(icon),
                    Err(error) => {
                        set_status(&weak, &format!("Tray initialization failed: {error}"));
                        return;
                    }
                }
            }

            while let Ok(event) = MenuEvent::receiver().try_recv() {
                match event.id.as_ref() {
                    "lastkey-open" => {
                        if let Some(window) = weak.upgrade() {
                            let _ = window.show();
                        }
                    }
                    "lastkey-defaults" => {
                        let settings = Settings::default();
                        match settings::save(&settings).and_then(|_| {
                            input.apply(settings.clone()).map_err(|error| {
                                settings::SettingsError::Io(std::io::Error::other(error))
                            })
                        }) {
                            Ok(()) => {
                                let mut state = state.borrow_mut();
                                state.saved = settings.clone();
                                state.working = settings.clone();
                                if let Some(window) = weak.upgrade() {
                                    update_window(&window, &settings);
                                    window.set_status("Default mappings applied.".into());
                                }
                            }
                            Err(error) => set_status(&weak, &error.to_string()),
                        }
                    }
                    "lastkey-exit" => {
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
            }
        });
        timer
    }

    fn create_tray() -> Result<TrayIcon, String> {
        let menu = Menu::new();
        let open = MenuItem::with_id("lastkey-open", "Open Settings", true, None);
        let defaults = MenuItem::with_id("lastkey-defaults", "Restore Defaults", true, None);
        let exit = MenuItem::with_id("lastkey-exit", "Exit", true, None);
        menu.append(&open).map_err(|error| error.to_string())?;
        menu.append(&defaults).map_err(|error| error.to_string())?;
        menu.append(&exit).map_err(|error| error.to_string())?;
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(tray_icon_image()?)
            .with_tooltip("LastKey")
            .build()
            .map_err(|error| error.to_string())
    }

    fn tray_icon_image() -> Result<Icon, String> {
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

    fn update_window(window: &MainWindow, settings: &Settings) {
        for key in LogicalKey::ALL {
            set_key_name(window, key, &display_name(settings.binding(key)));
        }
        window.set_transition_min_ms(settings.timing.transition_min_ms as i32);
        window.set_transition_max_ms(settings.timing.transition_max_ms as i32);
        window.set_overlap_min_ms(settings.timing.overlap_min_ms as i32);
        window.set_overlap_max_ms(settings.timing.overlap_max_ms as i32);
        window.set_overlap_probability(settings.timing.overlap_probability as i32);
        window.set_full_overlap(settings.timing.full_overlap);
        window.set_measurement_active(false);
        window.set_measurement_summary("Measurement is off. No samples are stored.".into());
        window.set_measurement_recommendation("".into());
        window.set_status("".into());
    }

    fn set_key_name(window: &MainWindow, key: LogicalKey, name: &str) {
        match key {
            LogicalKey::VerticalFirst => window.set_vertical_first(name.into()),
            LogicalKey::VerticalSecond => window.set_vertical_second(name.into()),
            LogicalKey::HorizontalFirst => window.set_horizontal_first(name.into()),
            LogicalKey::HorizontalSecond => window.set_horizontal_second(name.into()),
        }
    }

    fn set_status(window: &slint::Weak<MainWindow>, message: &str) {
        if let Some(window) = window.upgrade() {
            window.set_status(message.into());
        }
    }

    fn logical_key(index: i32) -> Option<LogicalKey> {
        match index {
            0 => Some(LogicalKey::VerticalFirst),
            1 => Some(LogicalKey::VerticalSecond),
            2 => Some(LogicalKey::HorizontalFirst),
            3 => Some(LogicalKey::HorizontalSecond),
            _ => None,
        }
    }

    fn display_name(key: PhysicalKey) -> String {
        match (key.scan_code, key.extended) {
            (0x11, false) => "W".into(),
            (0x1F, false) => "S".into(),
            (0x1E, false) => "A".into(),
            (0x20, false) => "D".into(),
            (0x48, true) => "Up Arrow".into(),
            (0x50, true) => "Down Arrow".into(),
            (0x4B, true) => "Left Arrow".into(),
            (0x4D, true) => "Right Arrow".into(),
            _ => format!("Scan code 0x{:02X}", key.scan_code),
        }
    }

    fn format_millis(micros: u64) -> String {
        format!("{:.1} ms", micros as f64 / 1_000.0)
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_app::run() {
        use windows::{
            Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
            core::{HSTRING, w},
        };

        let message = HSTRING::from(format!("LastKey failed to start: {error}"));
        let _ = unsafe { MessageBoxW(None, &message, w!("LastKey"), MB_ICONERROR | MB_OK) };
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
            Err(error) => eprintln!("LastKey Linux input support is unavailable: {error}"),
        }
        return;
    }
    #[cfg(not(target_os = "linux"))]
    eprintln!("LastKey input support is not available on this platform.");
    std::process::exit(1);
}
