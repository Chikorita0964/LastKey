#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows_app {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use lastkey::{
        core::{LogicalKey, MIN_RECOMMENDATION_SAMPLES, RecommendedTimingRange},
        debug_log,
        platform::windows::{
            DebugInputSampler, InputService, MeasurementUpdate, physical_key_name,
        },
        settings::{self, Settings},
    };
    use slint::{ComponentHandle, Timer, TimerMode};
    use tray_icon::{
        Icon, TrayIcon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem},
    };
    use windows::{
        Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND},
            System::Threading::{CreateMutexW, GetCurrentProcessId},
            UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
            },
        },
        core::w,
    };

    slint::include_modules!();

    struct UiState {
        saved: Settings,
        working: Settings,
        capture_generation: u64,
        measurement_generation: u64,
        measurement_active: bool,
    }

    struct SingleInstance(HANDLE);

    impl SingleInstance {
        fn acquire() -> Result<Self, String> {
            let handle = unsafe {
                CreateMutexW(
                    None,
                    true,
                    w!("Local\\LastKey.TemporaryDebug.SingleInstance"),
                )
            }
            .map_err(|error| error.to_string())?;
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                debug_log::write(format_args!(
                    "single-instance acquisition rejected because another instance exists"
                ));
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err("Another LastKey debug instance is already running.".into());
            }
            debug_log::write(format_args!("single-instance mutex acquired"));
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
        let debug_log_path = debug_log::init().map_err(|error| error.to_string())?;
        let single_instance = SingleInstance::acquire()?;
        let process_id = std::process::id();
        debug_log::write(format_args!("windows app starting pid={process_id}"));
        let debug_input = DebugInputSampler::start().map_err(|error| error.to_string())?;
        let settings = settings::load().unwrap_or_else(|error| {
            debug_log::write(format_args!("settings load failed error={error}"));
            eprintln!("LastKey settings could not be loaded: {error}");
            Settings::default()
        });
        debug_log::write(format_args!("settings loaded settings={settings:?}"));
        let window = MainWindow::new().map_err(|error| error.to_string())?;
        let measurement_window = MeasurementWindow::new().map_err(|error| error.to_string())?;
        let error_window = ErrorWindow::new().map_err(|error| error.to_string())?;
        update_window(&window, &settings);
        reset_measurement_window(&measurement_window);
        window.set_debug_log_path(debug_log_path.display().to_string().into());
        window.set_debug_process_id(process_id.to_string().into());
        let state = Arc::new(Mutex::new(UiState {
            saved: settings.clone(),
            working: settings.clone(),
            capture_generation: 0,
            measurement_generation: 0,
            measurement_active: false,
        }));
        log_mapping_state(
            "startup",
            &window,
            &state.lock().expect("UI state mutex is not poisoned"),
        );

        configure_close_behavior(&window);
        configure_measurement_close_behavior(&measurement_window);
        configure_error_close_behavior(&error_window);
        window.show().map_err(|error| error.to_string())?;
        debug_log::write(format_args!("settings window shown"));
        let input =
            Rc::new(InputService::start(settings.clone()).map_err(|error| error.to_string())?);
        configure_callbacks(
            &window,
            &measurement_window,
            &error_window,
            Rc::clone(&input),
            Arc::clone(&state),
        );
        let tray_timer = configure_tray(&window, &error_window);
        slint::run_event_loop_until_quit().map_err(|error| error.to_string())?;
        window.hide().map_err(|error| error.to_string())?;
        measurement_window
            .hide()
            .map_err(|error| error.to_string())?;
        error_window.hide().map_err(|error| error.to_string())?;
        drop(tray_timer);
        drop(error_window);
        drop(measurement_window);
        drop(window);
        drop(state);
        drop(input);
        drop(debug_input);
        drop(single_instance);
        debug_log::write(format_args!("windows app stopped"));
        Ok(())
    }

    fn configure_callbacks(
        window: &MainWindow,
        measurement_window: &MeasurementWindow,
        error_window: &ErrorWindow,
        input: Rc<InputService>,
        state: Arc<Mutex<UiState>>,
    ) {
        let weak = window.as_weak();
        window.on_adjust_number(move |index, delta| {
            if let Some(window) = weak.upgrade() {
                adjust_numeric_field(&window, index, delta);
            }
        });

        let weak = window.as_weak();
        window.on_toggle_socd_transition_delay(move |enabled| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !enabled && window.get_preserve_overlap() {
                remember_configured_preservation_rate(&window);
            }
            window.set_socd_transition_delay_enabled(enabled);
            refresh_effective_preservation_rate(&window);
            debug_log::write(format_args!(
                "UI SOCD Transition Delay toggled enabled={enabled} preserve_overlap={}",
                window.get_preserve_overlap()
            ));
        });

        let weak = window.as_weak();
        window.on_toggle_preserve_overlap(move |enabled| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let enabled = enabled && window.get_socd_transition_delay_enabled();
            if !enabled {
                remember_configured_preservation_rate(&window);
            }
            window.set_preserve_overlap(enabled);
            refresh_effective_preservation_rate(&window);
            debug_log::write(format_args!(
                "UI Preserve Overlap toggled enabled={enabled} configured_rate={} effective_rate={}",
                window.get_configured_overlap_preservation_rate(),
                window.get_overlap_preservation_rate()
            ));
        });

        let weak = window.as_weak();
        let error_for_capture = error_window.as_weak();
        let input_for_capture = Rc::clone(&input);
        let state_for_capture = Arc::clone(&state);
        window.on_capture_key(move |index| {
            debug_log::write(format_args!("UI capture button clicked index={index}"));
            let Some(key) = logical_key(index) else {
                debug_log::write(format_args!("UI capture rejected unknown index={index}"));
                show_error(&error_for_capture, "Key Capture Error", "Unknown key field.");
                return;
            };
            match input_for_capture.capture_next() {
                Ok(receiver) => {
                    let generation = {
                        let mut state = state_for_capture
                            .lock()
                            .expect("UI state mutex is not poisoned");
                        state.capture_generation = state.capture_generation.wrapping_add(1);
                        state.capture_generation
                    };
                    debug_log::write(format_args!(
                        "UI capture armed logical={key:?} generation={generation}"
                    ));
                    set_mapping_status(&weak, "Press a key...");

                    let weak_for_result = weak.clone();
                    let state_for_result = Arc::clone(&state_for_capture);
                    if let Err(error) = thread::Builder::new()
                        .name("lastkey-key-capture".into())
                        .spawn(move || {
                            let result = receiver.recv();
                            debug_log::write(format_args!(
                                "capture receiver completed logical={key:?} generation={generation} result={result:?}"
                            ));
                            let _ = weak_for_result.upgrade_in_event_loop(move |window| {
                                let mut state = state_for_result
                                    .lock()
                                    .expect("UI state mutex is not poisoned");
                                if state.capture_generation != generation {
                                    debug_log::write(format_args!(
                                        "UI ignored stale capture logical={key:?} generation={generation} current_generation={}",
                                        state.capture_generation
                                    ));
                                    return;
                                }
                                match result {
                                    Ok(captured) => {
                                        let displayed_before = displayed_key_name(&window, key);
                                        let draft_before = state.working.binding(key);
                                        state.working.set_binding(key, captured.physical);
                                        set_key_name(&window, key, &captured.name);
                                        request_settings_redraw(&window, "mapping capture completed");
                                        let displayed_after = displayed_key_name(&window, key);
                                        debug_log::write(format_args!(
                                            "UI mapping label update logical={key:?} generation={generation} displayed_before={displayed_before:?} displayed_after={displayed_after:?} draft_before={draft_before:?} draft_after={:?} captured_name={:?} captured_physical={:?}",
                                            state.working.binding(key), captured.name, captured.physical
                                        ));
                                        log_mapping_state(
                                            "capture-complete",
                                            &window,
                                            &state,
                                        );
                                        window.set_mapping_status(
                                            "Mapping changed. Select Apply when ready.".into(),
                                        );
                                    }
                                    Err(_) => {
                                        debug_log::write(format_args!(
                                            "UI capture cancelled logical={key:?} generation={generation}"
                                        ));
                                        window.set_mapping_status("Key capture was cancelled.".into())
                                    }
                                }
                            });
                        })
                    {
                        debug_log::write(format_args!(
                            "capture receiver thread spawn failed error={error}"
                        ));
                        state_for_capture
                            .lock()
                            .expect("UI state mutex is not poisoned")
                            .capture_generation = generation.wrapping_add(1);
                        show_error(
                            &error_for_capture,
                            "Key Capture Error",
                            &format!("Key capture could not start: {error}"),
                        );
                    }
                }
                Err(error) => show_error(
                    &error_for_capture,
                    "Key Capture Error",
                    &error.to_string(),
                ),
            }
        });

        let weak = window.as_weak();
        let error_for_apply = error_window.as_weak();
        let measurement_for_apply = measurement_window.as_weak();
        let input_for_apply = Rc::clone(&input);
        let state_for_apply = Arc::clone(&state);
        window.on_apply_settings(move || {
            debug_log::write(format_args!("UI apply clicked"));
            if let Some(window) = weak.upgrade() {
                let state = state_for_apply
                    .lock()
                    .expect("UI state mutex is not poisoned");
                log_mapping_state("apply-click", &window, &state);
            }
            let mut settings = state_for_apply
                .lock()
                .expect("UI state mutex is not poisoned")
                .working
                .clone();
            let Some(window) = weak.upgrade() else {
                return;
            };
            settings.timing.socd_transition_min_micros =
                parse_millis_text(window.get_socd_transition_min_ms().as_str(), 1_000_000);
            settings.timing.socd_transition_max_micros =
                parse_millis_text(window.get_socd_transition_max_ms().as_str(), 1_000_000);
            settings.timing.socd_transition_delay_enabled =
                window.get_socd_transition_delay_enabled();
            settings.timing.preserve_overlap = window.get_preserve_overlap();
            let configured_rate = if settings.timing.socd_transition_delay_enabled
                && settings.timing.preserve_overlap
            {
                window.get_overlap_preservation_rate()
            } else {
                window.get_configured_overlap_preservation_rate()
            };
            settings.timing.overlap_preservation_rate =
                parse_numeric_text(configured_rate.as_str(), 100) as u8;
            settings.timing.preserved_overlap_min_micros =
                parse_millis_text(window.get_preserved_overlap_min_ms().as_str(), 1_000_000);
            settings.timing.preserved_overlap_max_micros =
                parse_millis_text(window.get_preserved_overlap_max_ms().as_str(), 1_000_000);
            if let Err(error) = settings.validate() {
                debug_log::write(format_args!("UI apply validation failed error={error}"));
                show_error(&error_for_apply, "Invalid Settings", &error.to_string());
                return;
            }
            if let Err(error) = settings::save(&settings) {
                debug_log::write(format_args!("UI apply save failed error={error}"));
                show_error(&error_for_apply, "Settings Save Error", &error.to_string());
                return;
            }
            if let Err(error) = input_for_apply.apply(settings.clone()) {
                debug_log::write(format_args!("UI apply input update failed error={error}"));
                show_error(&error_for_apply, "Input Service Error", &error.to_string());
                return;
            }
            let mut state = state_for_apply
                .lock()
                .expect("UI state mutex is not poisoned");
            debug_log::write(format_args!("UI apply completed settings={settings:?}"));
            state.saved = settings.clone();
            state.working = settings.clone();
            state.capture_generation = state.capture_generation.wrapping_add(1);
            state.measurement_generation = state.measurement_generation.wrapping_add(1);
            state.measurement_active = false;
            if let Some(window) = weak.upgrade() {
                update_window(&window, &settings);
                log_mapping_state("apply-complete", &window, &state);
            }
            if let Some(measurement_window) = measurement_for_apply.upgrade() {
                measurement_window.set_measurement_active(false);
                measurement_window
                    .set_status("Measurement stopped because settings changed.".into());
            }
            set_action_status(&weak, "Settings applied.");
        });

        let weak = window.as_weak();
        let state_for_cancel = Arc::clone(&state);
        window.on_cancel_settings(move || {
            let mut state = state_for_cancel
                .lock()
                .expect("UI state mutex is not poisoned");
            if let Some(window) = weak.upgrade() {
                log_mapping_state("cancel-before", &window, &state);
            }
            state.working = state.saved.clone();
            if let Some(window) = weak.upgrade() {
                update_window(&window, &state.working);
                request_settings_redraw(&window, "cancel restored saved settings");
                log_mapping_state("cancel-after", &window, &state);
                window.set_action_status("Unsaved changes were reverted.".into());
            }
        });

        let weak = window.as_weak();
        let state_for_defaults = Arc::clone(&state);
        window.on_restore_defaults(move || {
            let mut state = state_for_defaults
                .lock()
                .expect("UI state mutex is not poisoned");
            restore_all_defaults(&mut state.working);
            state.capture_generation = state.capture_generation.wrapping_add(1);
            if let Some(window) = weak.upgrade() {
                update_window(&window, &state.working);
                request_settings_redraw(&window, "draft defaults restored");
                log_mapping_state("restore-defaults", &window, &state);
                window.set_action_status(
                    "All default settings restored. Select Apply to activate them.".into(),
                );
            }
        });

        let weak = window.as_weak();
        let state_for_mapping_defaults = Arc::clone(&state);
        window.on_restore_key_mappings(move || {
            let mut state = state_for_mapping_defaults
                .lock()
                .expect("UI state mutex is not poisoned");
            restore_default_bindings(&mut state.working);
            state.capture_generation = state.capture_generation.wrapping_add(1);
            if let Some(window) = weak.upgrade() {
                for key in LogicalKey::ALL {
                    set_key_name(&window, key, &physical_key_name(state.working.binding(key)));
                }
                request_settings_redraw(&window, "draft key mappings restored");
                log_mapping_state("restore-key-mappings", &window, &state);
                window.set_mapping_status(
                    "Default key mappings restored. Select Apply to activate them.".into(),
                );
            }
        });

        let measurement_for_open = measurement_window.as_weak();
        let error_for_open_measurement = error_window.as_weak();
        window.on_open_measurement(move || {
            debug_log::write(format_args!("UI open-measurement clicked"));
            if let Some(measurement_window) = measurement_for_open.upgrade() {
                if let Err(error) = measurement_window.show() {
                    debug_log::write(format_args!("measurement window show failed error={error}"));
                    show_error(
                        &error_for_open_measurement,
                        "Measurement Error",
                        &format!("Measurement results window could not open: {error}"),
                    );
                    return;
                }
                measurement_window.window().request_redraw();
                debug_log::write(format_args!("measurement window shown"));
            }
        });

        let measurement = measurement_window.as_weak();
        let error_for_measurement = error_window.as_weak();
        let input_for_measurement = Rc::clone(&input);
        let state_for_measurement = Arc::clone(&state);
        measurement_window.on_toggle_measurement(move || {
            let measurement_active = state_for_measurement
                .lock()
                .expect("UI state mutex is not poisoned")
                .measurement_active;
            if measurement_active {
                debug_log::write(format_args!("UI stop-measurement clicked"));
                {
                    let mut state = state_for_measurement
                        .lock()
                        .expect("UI state mutex is not poisoned");
                    state.measurement_active = false;
                    state.measurement_generation = state.measurement_generation.wrapping_add(1);
                }
                match input_for_measurement.stop_measurement() {
                    Ok(update) => {
                        debug_log::write(format_args!(
                            "UI stop-measurement completed final_update={update:?}"
                        ));
                        if let Some(measurement_window) = measurement.upgrade() {
                            measurement_window.set_measurement_active(false);
                            if let Some(update) = update {
                                set_measurement_update(&measurement_window, update);
                                measurement_window.set_status(
                                    "Measurement stopped. Final results are shown below.".into(),
                                );
                            } else {
                                measurement_window.set_status(
                                    "Measurement stopped. No configured key input was recorded."
                                        .into(),
                                );
                            }
                        }
                    }
                    Err(error) => show_error(
                        &error_for_measurement,
                        "Measurement Error",
                        &error.to_string(),
                    ),
                }
            } else {
                debug_log::write(format_args!("UI start-measurement clicked"));
                {
                    let mut state = state_for_measurement
                        .lock()
                        .expect("UI state mutex is not poisoned");
                    state.capture_generation = state.capture_generation.wrapping_add(1);
                    debug_log::write(format_args!(
                        "UI invalidated pending capture before measurement generation={}",
                        state.capture_generation
                    ));
                }
                let Some(measurement_window) = measurement.upgrade() else {
                    show_error(
                        &error_for_measurement,
                        "Measurement Error",
                        "Measurement results window is unavailable.",
                    );
                    return;
                };
                reset_measurement_window(&measurement_window);
                measurement_window.set_measurement_active(true);
                measurement_window.set_status("Starting measurement...".into());
                if let Err(error) = measurement_window.show() {
                    measurement_window.set_measurement_active(false);
                    show_error(
                        &error_for_measurement,
                        "Measurement Error",
                        &format!("Measurement results window could not open: {error}"),
                    );
                    return;
                }
                measurement_window.window().request_redraw();
                match input_for_measurement.start_measurement() {
                    Ok(receiver) => {
                        let generation = {
                            let mut state = state_for_measurement
                                .lock()
                                .expect("UI state mutex is not poisoned");
                            state.measurement_generation =
                                state.measurement_generation.wrapping_add(1);
                            state.measurement_active = true;
                            state.measurement_generation
                        };
                        debug_log::write(format_args!(
                            "UI measurement armed generation={generation}"
                        ));
                        measurement_window
                            .set_status("Measuring configured physical key transitions...".into());

                        let measurement_for_updates = measurement.clone();
                        let state_for_updates = Arc::clone(&state_for_measurement);
                        if let Err(error) = thread::Builder::new()
                            .name("lastkey-measurement-ui".into())
                            .spawn(move || {
                                while let Ok(update) = receiver.recv() {
                                    debug_log::write(format_args!(
                                        "measurement receiver update generation={generation} update={update:?}"
                                    ));
                                    let state = Arc::clone(&state_for_updates);
                                    let _ = measurement_for_updates.upgrade_in_event_loop(
                                        move |measurement_window| {
                                            let state = state
                                                .lock()
                                                .expect("UI state mutex is not poisoned");
                                            if state.measurement_active
                                                && state.measurement_generation == generation
                                            {
                                                set_measurement_update(
                                                    &measurement_window,
                                                    update,
                                                );
                                                debug_log::write(format_args!(
                                                    "UI measurement rendered generation={generation} update={update:?}"
                                                ));
                                            }
                                        },
                                    );
                                }
                            })
                        {
                            let _ = input_for_measurement.stop_measurement();
                            let mut state = state_for_measurement
                                .lock()
                                .expect("UI state mutex is not poisoned");
                            state.measurement_active = false;
                            state.measurement_generation =
                                state.measurement_generation.wrapping_add(1);
                            show_error(
                                &error_for_measurement,
                                "Measurement Error",
                                &format!("Measurement display could not start: {error}"),
                            );
                            measurement_window.set_measurement_active(false);
                            measurement_window.set_status("Measurement is not active.".into());
                        }
                    }
                    Err(error) => {
                        measurement_window.set_measurement_active(false);
                        measurement_window.set_status("Measurement is not active.".into());
                        show_error(
                            &error_for_measurement,
                            "Measurement Error",
                            &error.to_string(),
                        );
                    }
                }
            }
        });
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

    fn configure_measurement_close_behavior(window: &MeasurementWindow) {
        let weak = window.as_weak();
        window.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }

    fn configure_error_close_behavior(window: &ErrorWindow) {
        let weak = window.as_weak();
        window.on_dismiss_error(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
        });

        let weak = window.as_weak();
        window.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
            slint::CloseRequestResponse::KeepWindowShown
        });
    }

    fn configure_tray(window: &MainWindow, error_window: &ErrorWindow) -> Timer {
        let weak = window.as_weak();
        let error_window_weak = error_window.as_weak();
        let tray = Rc::new(RefCell::new(None));
        let tray_for_timer = Rc::clone(&tray);
        let tray_error_reported = Rc::new(Cell::new(false));
        let tray_error_for_timer = Rc::clone(&tray_error_reported);
        let last_foreground = Rc::new(RefCell::new(None));
        let last_foreground_for_timer = Rc::clone(&last_foreground);
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            let foreground = foreground_window_snapshot();
            if last_foreground_for_timer.borrow().as_ref() != Some(&foreground) {
                debug_log::write(format_args!(
                    "foreground changed hwnd=0x{:X} pid={} title={:?} settings_active={}",
                    foreground.hwnd,
                    foreground.process_id,
                    foreground.title,
                    foreground.process_id == unsafe { GetCurrentProcessId() }
                ));
                *last_foreground_for_timer.borrow_mut() = Some(foreground);
            }
            if tray_for_timer.borrow().is_none() {
                match create_tray() {
                    Ok(icon) => {
                        *tray_for_timer.borrow_mut() = Some(icon);
                        tray_error_for_timer.set(false);
                    }
                    Err(error) => {
                        if !tray_error_for_timer.replace(true) {
                            show_error(
                                &error_window_weak,
                                "Tray Error",
                                &format!("Tray initialization failed: {error}"),
                            );
                        }
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
                    "lastkey-exit" => {
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
            }
        });
        timer
    }

    #[derive(Eq, PartialEq)]
    struct ForegroundWindow {
        hwnd: usize,
        process_id: u32,
        title: String,
    }

    fn foreground_window_snapshot() -> ForegroundWindow {
        let window = unsafe { GetForegroundWindow() };
        let mut process_id = 0;
        if window != HWND::default() {
            unsafe {
                GetWindowThreadProcessId(window, Some(&mut process_id));
            }
        }
        let mut title = [0_u16; 512];
        let length = if window == HWND::default() {
            0
        } else {
            unsafe { GetWindowTextW(window, &mut title) }
        };
        ForegroundWindow {
            hwnd: window.0 as usize,
            process_id,
            title: String::from_utf16_lossy(&title[..length.max(0) as usize]),
        }
    }

    fn create_tray() -> Result<TrayIcon, String> {
        let menu = Menu::new();
        let open = MenuItem::with_id("lastkey-open", "Open Settings", true, None);
        let exit = MenuItem::with_id("lastkey-exit", "Exit", true, None);
        menu.append(&open).map_err(|error| error.to_string())?;
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

    fn parse_numeric_text(text: &str, maximum: u32) -> u32 {
        text.trim().parse::<u32>().unwrap_or(0).min(maximum)
    }

    fn parse_millis_text(text: &str, maximum_micros: u32) -> u32 {
        let Ok(milliseconds) = text.trim().parse::<f64>() else {
            return 0;
        };
        if !milliseconds.is_finite() || milliseconds <= 0.0 {
            return 0;
        }
        let maximum_tenths = maximum_micros / 100;
        let tenths = (milliseconds * 10.0)
            .round()
            .clamp(0.0, f64::from(maximum_tenths));
        tenths as u32 * 100
    }

    fn restore_all_defaults(settings: &mut Settings) {
        *settings = Settings::default();
    }

    fn restore_default_bindings(settings: &mut Settings) {
        settings.bindings = Settings::default().bindings;
    }

    fn adjust_numeric_field(window: &MainWindow, index: i32, delta: i32) {
        let Some((text, maximum)) = numeric_field(window, index) else {
            debug_log::write(format_args!(
                "UI numeric adjustment ignored unknown index={index}"
            ));
            return;
        };
        let is_timing = index != 2;
        let current = if is_timing {
            parse_millis_text(&text, maximum)
        } else {
            parse_numeric_text(&text, maximum)
        };
        let step = if is_timing { 100 } else { 1 };
        let adjustment = delta.unsigned_abs().saturating_mul(step);
        let adjusted = if delta >= 0 {
            current.saturating_add(adjustment)
        } else {
            current.saturating_sub(adjustment)
        }
        .min(maximum)
        .max(if index == 2 { 1 } else { 0 });
        let displayed = if is_timing {
            format_millis_input(adjusted)
        } else {
            adjusted.to_string()
        };
        set_numeric_field(window, index, displayed.into());
        debug_log::write(format_args!(
            "UI numeric adjustment index={index} input={text:?} delta={delta} result={adjusted}"
        ));
    }

    fn numeric_field(window: &MainWindow, index: i32) -> Option<(String, u32)> {
        match index {
            0 => Some((window.get_socd_transition_min_ms().to_string(), 1_000_000)),
            1 => Some((window.get_socd_transition_max_ms().to_string(), 1_000_000)),
            2 => Some((window.get_overlap_preservation_rate().to_string(), 100)),
            3 => Some((window.get_preserved_overlap_min_ms().to_string(), 1_000_000)),
            4 => Some((window.get_preserved_overlap_max_ms().to_string(), 1_000_000)),
            _ => None,
        }
    }

    fn set_numeric_field(window: &MainWindow, index: i32, value: slint::SharedString) {
        match index {
            0 => window.set_socd_transition_min_ms(value),
            1 => window.set_socd_transition_max_ms(value),
            2 => {
                window.set_overlap_preservation_rate(value.clone());
                window.set_configured_overlap_preservation_rate(value);
            }
            3 => window.set_preserved_overlap_min_ms(value),
            4 => window.set_preserved_overlap_max_ms(value),
            _ => {}
        }
    }

    fn set_numeric_fields(window: &MainWindow, settings: &Settings) {
        window.set_socd_transition_min_ms(
            format_millis_input(settings.timing.socd_transition_min_micros).into(),
        );
        window.set_socd_transition_max_ms(
            format_millis_input(settings.timing.socd_transition_max_micros).into(),
        );
        window.set_configured_overlap_preservation_rate(
            settings.timing.overlap_preservation_rate.to_string().into(),
        );
        window.set_preserved_overlap_min_ms(
            format_millis_input(settings.timing.preserved_overlap_min_micros).into(),
        );
        window.set_preserved_overlap_max_ms(
            format_millis_input(settings.timing.preserved_overlap_max_micros).into(),
        );
    }

    fn update_window(window: &MainWindow, settings: &Settings) {
        for key in LogicalKey::ALL {
            set_key_name(window, key, &physical_key_name(settings.binding(key)));
        }
        set_numeric_fields(window, settings);
        window.set_socd_transition_delay_enabled(settings.timing.socd_transition_delay_enabled);
        window.set_preserve_overlap(settings.timing.preserve_overlap);
        refresh_effective_preservation_rate(window);
        window.set_mapping_status("".into());
        window.set_action_status("".into());
    }

    fn remember_configured_preservation_rate(window: &MainWindow) {
        let rate = parse_numeric_text(window.get_overlap_preservation_rate().as_str(), 100);
        if rate > 0 {
            window.set_configured_overlap_preservation_rate(rate.to_string().into());
        }
    }

    fn refresh_effective_preservation_rate(window: &MainWindow) {
        let effective =
            if window.get_socd_transition_delay_enabled() && window.get_preserve_overlap() {
                window.get_configured_overlap_preservation_rate()
            } else {
                "0".into()
            };
        window.set_overlap_preservation_rate(effective);
    }

    fn set_key_name(window: &MainWindow, key: LogicalKey, name: &str) {
        match key {
            LogicalKey::VerticalFirst => window.set_vertical_first(name.into()),
            LogicalKey::VerticalSecond => window.set_vertical_second(name.into()),
            LogicalKey::HorizontalFirst => window.set_horizontal_first(name.into()),
            LogicalKey::HorizontalSecond => window.set_horizontal_second(name.into()),
        }
    }

    fn displayed_key_name(window: &MainWindow, key: LogicalKey) -> String {
        match key {
            LogicalKey::VerticalFirst => window.get_vertical_first().to_string(),
            LogicalKey::VerticalSecond => window.get_vertical_second().to_string(),
            LogicalKey::HorizontalFirst => window.get_horizontal_first().to_string(),
            LogicalKey::HorizontalSecond => window.get_horizontal_second().to_string(),
        }
    }

    fn log_mapping_state(label: &str, window: &MainWindow, state: &UiState) {
        let displayed = LogicalKey::ALL.map(|key| displayed_key_name(window, key));
        let draft = LogicalKey::ALL.map(|key| state.working.binding(key));
        let saved = LogicalKey::ALL.map(|key| state.saved.binding(key));
        debug_log::write(format_args!(
            "mapping state label={label} displayed={displayed:?} draft={draft:?} saved={saved:?}"
        ));
    }

    fn request_settings_redraw(window: &MainWindow, reason: &str) {
        debug_log::write(format_args!("UI redraw requested reason={reason:?}"));
        window.window().request_redraw();
        debug_log::write(format_args!("UI redraw request returned reason={reason:?}"));
    }

    fn reset_measurement_window(window: &MeasurementWindow) {
        window.set_measurement_active(false);
        window.set_status("Measurement has not started.".into());
        window.set_edge_count(0);
        window.set_sample_count(0);
        window.set_near_simultaneous_count(0);
        window.set_transition_count(0);
        window.set_transition_median("—".into());
        window.set_transition_p10("—".into());
        window.set_transition_p90("—".into());
        window.set_transition_minimum("—".into());
        window.set_transition_maximum("—".into());
        window.set_overlap_count(0);
        window.set_overlap_median("—".into());
        window.set_overlap_p10("—".into());
        window.set_overlap_p90("—".into());
        window.set_overlap_minimum("—".into());
        window.set_overlap_maximum("—".into());
        window.set_observed_overlap_frequency("—".into());
        window.set_near_simultaneous_frequency("—".into());
        window.set_suggested_transition_range(
            format!("Collect {MIN_RECOMMENDATION_SAMPLES} neutral transition samples.").into(),
        );
        window.set_suggested_overlap_range(
            format!("Collect {MIN_RECOMMENDATION_SAMPLES} physical overlap samples.").into(),
        );
    }

    fn set_measurement_update(window: &MeasurementWindow, update: MeasurementUpdate) {
        let statistics = update.statistics;
        let sample_count = statistics.sample_count();
        window.set_edge_count(display_count(update.observed_event_count));
        window.set_sample_count(display_count(sample_count));
        window.set_near_simultaneous_count(display_count(statistics.near_simultaneous_count()));
        window.set_transition_count(display_count(statistics.transition_count()));
        window.set_transition_median(format_duration(statistics.transition_median_micros()).into());
        window.set_transition_p10(format_duration(statistics.transition_p10_micros()).into());
        window.set_transition_p90(format_duration(statistics.transition_p90_micros()).into());
        window.set_transition_minimum(format_duration(statistics.transition_min_micros()).into());
        window.set_transition_maximum(format_duration(statistics.transition_max_micros()).into());
        window.set_overlap_count(display_count(statistics.overlap_count()));
        window.set_overlap_median(format_duration(statistics.overlap_median_micros()).into());
        window.set_overlap_p10(format_duration(statistics.overlap_p10_micros()).into());
        window.set_overlap_p90(format_duration(statistics.overlap_p90_micros()).into());
        window.set_overlap_minimum(format_duration(statistics.overlap_min_micros()).into());
        window.set_overlap_maximum(format_duration(statistics.overlap_max_micros()).into());
        window.set_observed_overlap_frequency(
            format_percentage(statistics.overlap_count(), sample_count).into(),
        );
        window.set_near_simultaneous_frequency(
            format_percentage(statistics.near_simultaneous_count(), sample_count).into(),
        );
        window.set_suggested_transition_range(
            format_recommended_range(
                update.recommendation.socd_transition,
                statistics.transition_count(),
                "neutral transition",
            )
            .into(),
        );
        window.set_suggested_overlap_range(
            format_recommended_range(
                update.recommendation.preserved_overlap,
                statistics.overlap_count(),
                "physical overlap",
            )
            .into(),
        );
    }

    fn format_recommended_range(
        recommendation: Option<RecommendedTimingRange>,
        sample_count: u32,
        sample_name: &str,
    ) -> String {
        match recommendation {
            Some(range) => format!(
                "{}–{} ms",
                format_millis_input(range.min_micros),
                format_millis_input(range.max_micros)
            ),
            None => {
                let remaining = MIN_RECOMMENDATION_SAMPLES.saturating_sub(sample_count);
                format!("Need {remaining} more {sample_name} samples.")
            }
        }
    }

    fn display_count(value: u32) -> i32 {
        i32::try_from(value).unwrap_or(i32::MAX)
    }

    fn format_percentage(count: u32, total: u32) -> String {
        if total == 0 {
            "—".into()
        } else {
            format!("{:.1}%", f64::from(count) * 100.0 / f64::from(total))
        }
    }

    fn format_duration(micros: Option<u64>) -> String {
        micros.map(format_millis).unwrap_or_else(|| "—".into())
    }

    fn set_mapping_status(window: &slint::Weak<MainWindow>, message: &str) {
        if let Some(window) = window.upgrade() {
            window.set_mapping_status(message.into());
        }
    }

    fn set_action_status(window: &slint::Weak<MainWindow>, message: &str) {
        if let Some(window) = window.upgrade() {
            window.set_action_status(message.into());
        }
    }

    fn show_error(window: &slint::Weak<ErrorWindow>, title: &str, message: &str) {
        debug_log::write(format_args!(
            "UI error window requested title={title:?} message={message:?}"
        ));
        if let Some(window) = window.upgrade() {
            window.set_error_title(title.into());
            window.set_error_message(message.into());
            if let Err(error) = window.show() {
                debug_log::write(format_args!("UI error window show failed error={error}"));
                return;
            }
            window.window().request_redraw();
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

    fn format_millis(micros: u64) -> String {
        format!("{:.1} ms", micros as f64 / 1_000.0)
    }

    fn format_millis_input(micros: u32) -> String {
        format!("{}.{:01}", micros / 1_000, (micros % 1_000) / 100)
    }

    #[cfg(test)]
    mod tests {
        use super::{
            Settings, parse_millis_text, parse_numeric_text, restore_all_defaults,
            restore_default_bindings,
        };

        #[test]
        fn empty_and_null_numeric_input_normalize_to_zero() {
            assert_eq!(parse_numeric_text("", 1000), 0);
            assert_eq!(parse_numeric_text("   ", 1000), 0);
            assert_eq!(parse_numeric_text("null", 1000), 0);
        }

        #[test]
        fn numeric_input_is_limited_to_the_field_maximum() {
            assert_eq!(parse_numeric_text("100", 1000), 100);
            assert_eq!(parse_numeric_text("1001", 1000), 1000);
            assert_eq!(parse_numeric_text("101", 100), 100);
        }

        #[test]
        fn millisecond_input_rounds_to_one_decimal_place() {
            assert_eq!(parse_millis_text("1.94", 1_000_000), 1_900);
            assert_eq!(parse_millis_text("1.95", 1_000_000), 2_000);
            assert_eq!(parse_millis_text("null", 1_000_000), 0);
            assert_eq!(parse_millis_text("1000.1", 1_000_000), 1_000_000);
        }

        #[test]
        fn restoring_all_defaults_resets_bindings_and_timing() {
            let mut settings = Settings::default();
            settings.bindings.rotate_left(1);
            settings.timing.socd_transition_min_micros = 15_000;
            settings.timing.socd_transition_max_micros = 20_000;
            settings.timing.preserve_overlap = true;
            settings.timing.overlap_preservation_rate = 50;

            restore_all_defaults(&mut settings);

            assert_eq!(settings, Settings::default());
        }

        #[test]
        fn restoring_key_mappings_preserves_timing() {
            let mut settings = Settings::default();
            settings.bindings.rotate_left(1);
            settings.timing.socd_transition_min_micros = 15_000;
            settings.timing.socd_transition_max_micros = 20_000;
            settings.timing.preserve_overlap = true;
            settings.timing.overlap_preservation_rate = 50;
            let timing = settings.timing.clone();

            restore_default_bindings(&mut settings);

            assert_eq!(settings.bindings, Settings::default().bindings);
            assert_eq!(settings.timing, timing);
        }
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
