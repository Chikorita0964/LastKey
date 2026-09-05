use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::c_void,
    mem::size_of,
    time::{Duration, Instant},
};
use std::{
    fmt,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateWaitableTimerExW,
                GetCurrentThreadId, SetWaitableTimerEx, TIMER_ALL_ACCESS,
            },
        },
        UI::{
            Input::{
                GetRawInputData, GetRegisteredRawInputDevices, HRAWINPUT,
                KeyboardAndMouse::{
                    GetKeyNameTextW, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput,
                },
                RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK,
                RIM_TYPEKEYBOARD, RegisterRawInputDevices,
            },
            WindowsAndMessaging::{
                CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, MWMO_INPUTAVAILABLE,
                MsgWaitForMultipleObjectsEx, PM_NOREMOVE, PM_REMOVE, PeekMessageW,
                PostThreadMessageW, QS_ALLINPUT, RegisterClassW, SetWindowsHookExW,
                TranslateMessage, UnhookWindowsHookEx, UnregisterClassW, WH_KEYBOARD_LL,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
                WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW,
            },
        },
    },
    core::w,
};

use crate::{
    app::RuntimeService,
    core::{
        EventDisposition, KeyAction, LogicalKey, MeasurementSession, OutputEmitter, PhysicalKey,
        TimingController, recommend,
    },
    settings::Settings,
};

pub use crate::app::{CapturedKey, MeasurementUpdate};

const INJECTION_TAG: usize = 0x4C41_5354_4B45_5931; // "LASTKEY1"

const COMMAND_MESSAGE: u32 = WM_APP + 1;
const RAW_KEY_BREAK: u16 = 0x01;
const RAW_KEY_E0: u16 = 0x02;
const RAW_KEY_E1: u16 = 0x04;
const HOOK_EVENT_MAX_AGE: Duration = Duration::from_secs(1);
const HOOK_EVENT_QUEUE_CAPACITY: usize = 64;
const HOOK_MISSES_BEFORE_REINSTALL: u8 = 3;
const HOOK_REINSTALL_COOLDOWN: Duration = Duration::from_secs(2);
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const WAIT_OBJECT_0_VALUE: u32 = 0;
const WAIT_FAILED_VALUE: u32 = u32::MAX;

thread_local! {
    static ENGINE: RefCell<Option<InputEngine>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub enum InputServiceError {
    Hook(String),
    ServiceStopped,
    ServiceTimeout,
}

impl fmt::Display for InputServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hook(message) => write!(formatter, "Windows keyboard hook failed: {message}"),
            Self::ServiceStopped => write!(formatter, "the input service is no longer running"),
            Self::ServiceTimeout => write!(formatter, "the input service did not respond in time"),
        }
    }
}

impl std::error::Error for InputServiceError {}

pub struct InputService {
    commands: Sender<InputCommand>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

enum InputCommand {
    Apply {
        settings: Settings,
        ready: mpsc::SyncSender<()>,
    },
    Capture {
        sender: Sender<CapturedKey>,
        ready: mpsc::SyncSender<Result<(), InputServiceError>>,
    },
    CancelCapture(mpsc::SyncSender<()>),
    StartMeasurement {
        sender: Sender<MeasurementUpdate>,
        ready: mpsc::SyncSender<Result<(), InputServiceError>>,
    },
    StopMeasurement(mpsc::SyncSender<Option<MeasurementUpdate>>),
    Stop,
}

impl InputService {
    pub fn start(settings: Settings) -> std::result::Result<Self, InputServiceError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("lastkey-input".into())
            .spawn(move || input_thread(settings, command_receiver, started_sender))
            .map_err(|error| InputServiceError::Hook(error.to_string()))?;

        let thread_id = receive_service_response(started_receiver, SERVICE_START_TIMEOUT)??;
        Ok(Self {
            commands: command_sender,
            thread_id,
            thread: Some(thread),
        })
    }

    pub fn apply(&self, settings: Settings) -> std::result::Result<(), InputServiceError> {
        settings
            .validate()
            .map_err(|error| InputServiceError::Hook(error.to_string()))?;
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::Apply {
            settings,
            ready: ready_sender,
        })?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)
    }

    pub fn capture_next(&self) -> std::result::Result<Receiver<CapturedKey>, InputServiceError> {
        let (sender, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::Capture {
            sender,
            ready: ready_sender,
        })?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)??;
        Ok(receiver)
    }

    pub fn cancel_capture(&self) -> std::result::Result<(), InputServiceError> {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::CancelCapture(ready_sender))?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)
    }

    pub fn start_measurement(
        &self,
    ) -> std::result::Result<Receiver<MeasurementUpdate>, InputServiceError> {
        let (sender, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::StartMeasurement {
            sender,
            ready: ready_sender,
        })?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)??;
        Ok(receiver)
    }

    pub fn stop_measurement(
        &self,
    ) -> std::result::Result<Option<MeasurementUpdate>, InputServiceError> {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::StopMeasurement(ready_sender))?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)
    }

    pub fn stop(mut self) {
        self.stop_inner();
    }

    fn stop_inner(&mut self) {
        let _ = self.commands.send(InputCommand::Stop);
        let _ =
            unsafe { PostThreadMessageW(self.thread_id, COMMAND_MESSAGE, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn send(&self, command: InputCommand) -> std::result::Result<(), InputServiceError> {
        self.commands
            .send(command)
            .map_err(|_| InputServiceError::ServiceStopped)?;
        if unsafe { PostThreadMessageW(self.thread_id, COMMAND_MESSAGE, WPARAM(0), LPARAM(0)) }
            .is_err()
        {
            return Err(InputServiceError::ServiceStopped);
        }
        Ok(())
    }
}

impl Drop for InputService {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl RuntimeService for InputService {
    fn apply(&self, settings: Settings) -> Result<(), String> {
        InputService::apply(self, settings).map_err(|error| error.to_string())
    }

    fn begin_key_capture(&self) -> Result<Receiver<CapturedKey>, String> {
        self.capture_next().map_err(|error| error.to_string())
    }

    fn cancel_key_capture(&self) -> Result<(), String> {
        self.cancel_capture().map_err(|error| error.to_string())
    }

    fn start_measurement(&self) -> Result<Receiver<MeasurementUpdate>, String> {
        InputService::start_measurement(self).map_err(|error| error.to_string())
    }

    fn stop_measurement(&self) -> Result<Option<MeasurementUpdate>, String> {
        InputService::stop_measurement(self).map_err(|error| error.to_string())
    }
}

struct InputEngine {
    timing: TimingController,
    settings: Settings,
    capture_sender: Option<Sender<CapturedKey>>,
    measurement: Option<MeasurementSession>,
    measurement_sender: Option<Sender<MeasurementUpdate>>,
    measurement_event_count: u32,
    // Capture consumes key-down, so its matching key-up must also be consumed.
    captured_key_awaiting_release: Option<PhysicalKey>,
    scheduler: Option<HighResolutionTimer>,
    hook_health: HookHealth,
}

#[derive(Clone, Copy)]
struct ObservedHookEvent {
    physical: PhysicalKey,
    action: KeyAction,
    observed_at: Instant,
}

struct HookHealth {
    observed_events: VecDeque<ObservedHookEvent>,
    consecutive_misses: u8,
    reinstall_requested: bool,
    last_reinstall_request: Option<Instant>,
}

struct HighResolutionTimer {
    handle: HANDLE,
    armed: bool,
}

fn receive_service_response<T>(
    receiver: Receiver<T>,
    timeout: Duration,
) -> Result<T, InputServiceError> {
    match receiver.recv_timeout(timeout) {
        Ok(response) => Ok(response),
        Err(RecvTimeoutError::Disconnected) => Err(InputServiceError::ServiceStopped),
        Err(RecvTimeoutError::Timeout) => Err(InputServiceError::ServiceTimeout),
    }
}

impl HighResolutionTimer {
    fn new() -> Result<Self, InputServiceError> {
        let high_resolution = unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
        };
        let handle = match high_resolution {
            Ok(handle) => handle,
            Err(_) => unsafe { CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0) }
                .map_err(|error| InputServiceError::Hook(error.to_string()))?,
        };
        Ok(Self {
            handle,
            armed: false,
        })
    }

    fn handle(&self) -> HANDLE {
        self.handle
    }

    fn arm(&mut self, deadline: Instant) -> Result<(), InputServiceError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let hundred_nanoseconds = remaining.as_nanos().max(100).div_ceil(100);
        let relative_due_time = -(hundred_nanoseconds.min(i64::MAX as u128) as i64);
        unsafe { SetWaitableTimerEx(self.handle, &relative_due_time, 0, None, None, None, 0) }
            .map_err(|error| InputServiceError::Hook(error.to_string()))?;
        self.armed = true;
        Ok(())
    }

    fn cancel(&mut self) {
        if !self.armed {
            return;
        }
        let _ = unsafe { CancelWaitableTimer(self.handle) };
        self.armed = false;
    }

    fn signal_consumed(&mut self) {
        self.armed = false;
    }
}

impl Drop for HighResolutionTimer {
    fn drop(&mut self) {
        self.cancel();
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl HookHealth {
    fn new() -> Self {
        Self {
            observed_events: VecDeque::with_capacity(HOOK_EVENT_QUEUE_CAPACITY),
            consecutive_misses: 0,
            reinstall_requested: false,
            last_reinstall_request: None,
        }
    }

    fn observe_hook(&mut self, physical: PhysicalKey, action: KeyAction, now: Instant) {
        self.prune(now);
        if self.observed_events.len() == HOOK_EVENT_QUEUE_CAPACITY {
            self.observed_events.pop_front();
        }
        self.observed_events.push_back(ObservedHookEvent {
            physical,
            action,
            observed_at: now,
        });
    }

    fn observe_raw(&mut self, physical: PhysicalKey, action: KeyAction, now: Instant) {
        self.prune(now);
        if let Some(position) = self
            .observed_events
            .iter()
            .position(|event| event.physical == physical && event.action == action)
        {
            self.observed_events.remove(position);
            self.consecutive_misses = 0;
            return;
        }

        self.consecutive_misses = self.consecutive_misses.saturating_add(1);
        let cooldown_elapsed = self
            .last_reinstall_request
            .is_none_or(|last| now.saturating_duration_since(last) >= HOOK_REINSTALL_COOLDOWN);
        if self.consecutive_misses >= HOOK_MISSES_BEFORE_REINSTALL && cooldown_elapsed {
            self.reinstall_requested = true;
            self.last_reinstall_request = Some(now);
        }
    }

    fn take_reinstall_request(&mut self) -> bool {
        std::mem::take(&mut self.reinstall_requested)
    }

    fn reinstalled(&mut self) {
        self.observed_events.clear();
        self.consecutive_misses = 0;
    }

    fn prune(&mut self, now: Instant) {
        while self.observed_events.front().is_some_and(|event| {
            now.saturating_duration_since(event.observed_at) > HOOK_EVENT_MAX_AGE
        }) {
            self.observed_events.pop_front();
        }
    }
}

impl InputEngine {
    fn new(settings: Settings) -> Self {
        Self {
            timing: TimingController::new(settings.timing.clone()),
            settings,
            capture_sender: None,
            measurement: None,
            measurement_sender: None,
            measurement_event_count: 0,
            captured_key_awaiting_release: None,
            scheduler: None,
            hook_health: HookHealth::new(),
        }
    }

    fn attach_scheduler(&mut self, scheduler: HighResolutionTimer) {
        self.scheduler = Some(scheduler);
    }

    fn process_hook(
        &mut self,
        physical: PhysicalKey,
        action: KeyAction,
        now: Instant,
    ) -> EventDisposition {
        if self.captured_key_awaiting_release == Some(physical) {
            if action == KeyAction::Up {
                self.captured_key_awaiting_release = None;
            }
            return EventDisposition::Consume;
        }

        if self.capture_sender.is_some() {
            if action == KeyAction::Down {
                self.complete_capture(physical);
                return EventDisposition::Consume;
            }
            // A key held before capture started has no consumed key-down,
            // so its key-up must pass through to release existing output.
            return EventDisposition::PassThrough;
        }

        let Some(key) = self.settings.logical_key_for(physical) else {
            return EventDisposition::PassThrough;
        };
        if self.measurement.is_some() {
            return EventDisposition::PassThrough;
        }
        // Mirror process_raw: only configured keys outside capture and
        // measurement feed hook-health, keeping the queue free of unrelated
        // typing and out of the hottest hook path for other applications.
        self.hook_health.observe_hook(physical, action, now);
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        let disposition = self.timing.process(key, action, now, &mut emitter);
        if self.timing.is_enabled() {
            self.update_timer();
        }
        disposition
    }

    fn process_raw(&mut self, physical: PhysicalKey, action: KeyAction, now: Instant) {
        // Mirror process_hook: consume auto-repeat downs as well so they never
        // reach hook-health as false misses while the captured key is held.
        if self.captured_key_awaiting_release == Some(physical) {
            if action == KeyAction::Up {
                self.captured_key_awaiting_release = None;
            }
            return;
        }
        if action == KeyAction::Down && self.capture_sender.is_some() {
            self.complete_capture(physical);
            return;
        }

        let Some(key) = self.settings.logical_key_for(physical) else {
            return;
        };
        if self.measurement.is_none() && self.capture_sender.is_none() {
            self.hook_health.observe_raw(physical, action, now);
        }
        if let Some(session) = self.measurement.as_mut() {
            self.measurement_event_count += 1;
            session.observe(key, action, now);
            let statistics = session.statistics();
            if let Some(sender) = &self.measurement_sender {
                let _ = sender.send(MeasurementUpdate {
                    observed_event_count: self.measurement_event_count,
                    statistics,
                    recommendation: recommend(statistics),
                });
            }
        }
    }

    fn apply(&mut self, settings: Settings) {
        self.cancel_timer();
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.timing.release_all(&mut emitter);
        self.settings = settings;
        self.timing = TimingController::new(self.settings.timing.clone());
        self.capture_sender = None;
        self.measurement = None;
        self.measurement_sender = None;
        self.measurement_event_count = 0;
    }

    fn release_all(&mut self) {
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.timing.release_all(&mut emitter);
        self.cancel_timer();
    }

    fn poll(&mut self) {
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.timing.poll(Instant::now(), &mut emitter);
        self.update_timer();
    }

    fn handle_timer_signal(&mut self) {
        if let Some(scheduler) = self.scheduler.as_mut() {
            scheduler.signal_consumed();
        }
        self.poll();
    }

    fn reset_timing_state(&mut self) {
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.timing.reset_state(&mut emitter);
        self.cancel_timer();
    }

    fn start_measurement(&mut self, sender: Sender<MeasurementUpdate>) {
        self.capture_sender = None;
        self.captured_key_awaiting_release = None;
        self.reset_timing_state();
        self.measurement = Some(MeasurementSession::new());
        self.measurement_sender = Some(sender);
        self.measurement_event_count = 0;
    }

    fn cancel_capture(&mut self) {
        self.capture_sender = None;
    }

    fn complete_capture(&mut self, physical: PhysicalKey) {
        let Some(sender) = self.capture_sender.take() else {
            return;
        };
        let captured = CapturedKey {
            physical,
            name: key_name(physical),
        };
        let _ = sender.send(captured);
        self.captured_key_awaiting_release = Some(physical);
    }

    fn stop_measurement(&mut self) -> Option<MeasurementUpdate> {
        let update = self.measurement.take().and_then(|session| {
            (self.measurement_event_count > 0).then(|| {
                let statistics = session.statistics();
                MeasurementUpdate {
                    observed_event_count: self.measurement_event_count,
                    statistics,
                    recommendation: recommend(statistics),
                }
            })
        });
        self.measurement_sender = None;
        self.measurement_event_count = 0;
        self.reset_timing_state();
        update
    }

    fn update_timer(&mut self) {
        self.cancel_timer();
        if let Some(deadline) = self.timing.next_deadline()
            && let Some(scheduler) = self.scheduler.as_mut()
        {
            let _ = scheduler.arm(deadline);
        }
    }

    fn cancel_timer(&mut self) {
        if let Some(scheduler) = self.scheduler.as_mut() {
            scheduler.cancel();
        }
    }

    fn take_hook_reinstall_request(&mut self) -> bool {
        self.hook_health.take_reinstall_request()
    }

    fn hook_reinstalled(&mut self) {
        self.hook_health.reinstalled();
    }
}

struct WindowsEmitter<'a> {
    settings: &'a Settings,
}

impl OutputEmitter for WindowsEmitter<'_> {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        let physical = self.settings.binding(key);
        let mut flags = KEYEVENTF_SCANCODE;
        if physical.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if action == KeyAction::Up {
            flags |= KEYEVENTF_KEYUP;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: Default::default(),
                    wScan: physical.scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: INJECTION_TAG,
                },
            },
        };

        (unsafe { SendInput(&[input], size_of::<INPUT>() as i32) }) == 1
    }
}

const RAW_INPUT_WINDOW_CLASS: windows::core::PCWSTR = w!("LastKey.RawInput.MessageWindow");

struct RawInputWindow {
    window: HWND,
    instance: HINSTANCE,
}

impl RawInputWindow {
    fn create(instance: HINSTANCE) -> Result<Self, InputServiceError> {
        let class = WNDCLASSW {
            lpfnWndProc: Some(raw_input_window_proc),
            hInstance: instance,
            lpszClassName: RAW_INPUT_WINDOW_CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(InputServiceError::Hook(
                windows::core::Error::from_thread().to_string(),
            ));
        }

        let window = match unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                RAW_INPUT_WINDOW_CLASS,
                w!("LastKey Raw Input"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            )
        } {
            Ok(window) => window,
            Err(error) => {
                unsafe {
                    let _ = UnregisterClassW(RAW_INPUT_WINDOW_CLASS, Some(instance));
                }
                return Err(InputServiceError::Hook(error.to_string()));
            }
        };

        let raw_input_window = Self { window, instance };
        raw_input_window.ensure_keyboard_registration()?;
        Ok(raw_input_window)
    }

    fn ensure_keyboard_registration(&self) -> Result<(), InputServiceError> {
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x06,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: self.window,
        };
        if let Err(error) =
            unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
        {
            return Err(InputServiceError::Hook(error.to_string()));
        }

        let after = registered_keyboard_target()?;
        if after != Some(self.window) {
            return Err(InputServiceError::Hook(format!(
                "raw keyboard input registration belongs to {} instead of 0x{:X}",
                format_window_handle(after),
                self.window.0 as usize
            )));
        }
        Ok(())
    }
}

fn registered_keyboard_target() -> Result<Option<HWND>, InputServiceError> {
    let device_size = size_of::<RAWINPUTDEVICE>() as u32;
    let mut device_count = 0;
    let result = unsafe { GetRegisteredRawInputDevices(None, &mut device_count, device_size) };
    if result == u32::MAX {
        return Err(InputServiceError::Hook(
            windows::core::Error::from_thread().to_string(),
        ));
    }
    if device_count == 0 {
        return Ok(None);
    }

    let mut devices = vec![RAWINPUTDEVICE::default(); device_count as usize];
    let result = unsafe {
        GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut device_count, device_size)
    };
    if result == u32::MAX {
        return Err(InputServiceError::Hook(
            windows::core::Error::from_thread().to_string(),
        ));
    }
    Ok(devices
        .into_iter()
        .take(result as usize)
        .find(|device| device.usUsagePage == 0x01 && device.usUsage == 0x06)
        .map(|device| device.hwndTarget))
}

fn format_window_handle(window: Option<HWND>) -> String {
    window
        .map(|window| format!("0x{:X}", window.0 as usize))
        .unwrap_or_else(|| "none".into())
}

impl Drop for RawInputWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.window);
            let _ = UnregisterClassW(RAW_INPUT_WINDOW_CLASS, Some(self.instance));
        }
    }
}

unsafe extern "system" fn raw_input_window_proc(
    window: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        process_raw_input(HRAWINPUT(l_param.0 as *mut c_void));
    }
    unsafe { DefWindowProcW(window, message, w_param, l_param) }
}

fn process_raw_input(handle: HRAWINPUT) {
    let mut raw = std::mem::MaybeUninit::<RAWINPUT>::zeroed();
    let mut byte_count = size_of::<RAWINPUT>() as u32;
    let copied = unsafe {
        GetRawInputData(
            handle,
            RID_INPUT,
            Some(raw.as_mut_ptr().cast::<c_void>()),
            &mut byte_count,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if copied == u32::MAX || copied == 0 {
        return;
    }

    let raw = unsafe { raw.assume_init() };
    if raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return;
    }
    let keyboard = unsafe { raw.data.keyboard };
    if keyboard.MakeCode == 0 {
        return;
    }

    let action = if keyboard.Flags & RAW_KEY_BREAK != 0 {
        KeyAction::Up
    } else {
        KeyAction::Down
    };
    let extended = keyboard.Flags & (RAW_KEY_E0 | RAW_KEY_E1) != 0;
    let physical = PhysicalKey::new(keyboard.MakeCode, extended);
    ENGINE.with(|engine| {
        engine
            .borrow_mut()
            .as_mut()
            .expect("input engine is initialized")
            .process_raw(physical, action, Instant::now());
    });
}

fn input_thread(
    settings: Settings,
    commands: Receiver<InputCommand>,
    started: mpsc::SyncSender<Result<u32, InputServiceError>>,
) {
    let mut ignored = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut ignored, None, 0, 0, PM_NOREMOVE);
    }
    let scheduler = match HighResolutionTimer::new() {
        Ok(scheduler) => scheduler,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    let scheduler_handle = scheduler.handle();
    ENGINE.with(|engine| {
        let mut input = InputEngine::new(settings);
        input.attach_scheduler(scheduler);
        *engine.borrow_mut() = Some(input);
    });

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(error) => {
            let _ = started.send(Err(InputServiceError::Hook(error.to_string())));
            return;
        }
    };
    let instance = HINSTANCE(module.0);
    let raw_input_window = match RawInputWindow::create(instance) {
        Ok(window) => window,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    let mut hook = match install_keyboard_hook(instance) {
        Ok(hook) => hook,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    let _ = started.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut message = MSG::default();
    let mut keep_running = true;
    loop {
        let wait_result = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[scheduler_handle]),
                u32::MAX,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if wait_result.0 == WAIT_OBJECT_0_VALUE {
            // Never panic on re-entry: if the hook is inside SendInput above,
            // skip this tick and let the next deadline retry the poll.
            ENGINE.with(|engine| {
                if let Ok(mut borrowed) = engine.try_borrow_mut() {
                    borrowed
                        .as_mut()
                        .expect("input engine is initialized")
                        .handle_timer_signal();
                }
            });
            continue;
        }
        if wait_result.0 == WAIT_FAILED_VALUE {
            break;
        }
        if wait_result.0 != WAIT_OBJECT_0_VALUE + 1 {
            break;
        }
        if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            continue;
        }
        if message.message == WM_QUIT {
            break;
        }
        if message.message == COMMAND_MESSAGE {
            while let Ok(command) = commands.try_recv() {
                match command {
                    InputCommand::Apply { settings, ready } => ENGINE.with(|engine| {
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .apply(settings);
                        let _ = ready.send(());
                    }),
                    InputCommand::Capture { sender, ready } => {
                        match raw_input_window.ensure_keyboard_registration() {
                            Ok(()) => {
                                ENGINE.with(|engine| {
                                    let mut engine = engine.borrow_mut();
                                    let engine =
                                        engine.as_mut().expect("input engine is initialized");
                                    engine.capture_sender = Some(sender);
                                });
                                let _ = ready.send(Ok(()));
                            }
                            Err(error) => {
                                let _ = ready.send(Err(error));
                            }
                        }
                    }
                    InputCommand::CancelCapture(ready) => ENGINE.with(|engine| {
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .cancel_capture();
                        let _ = ready.send(());
                    }),
                    InputCommand::StartMeasurement { sender, ready } => {
                        match raw_input_window.ensure_keyboard_registration() {
                            Ok(()) => {
                                ENGINE.with(|engine| {
                                    engine
                                        .borrow_mut()
                                        .as_mut()
                                        .expect("input engine is initialized")
                                        .start_measurement(sender);
                                });
                                let _ = ready.send(Ok(()));
                            }
                            Err(error) => {
                                let _ = ready.send(Err(error));
                            }
                        }
                    }
                    InputCommand::StopMeasurement(ready) => ENGINE.with(|engine| {
                        let update = engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .stop_measurement();
                        let _ = ready.send(update);
                    }),
                    InputCommand::Stop => {
                        keep_running = false;
                        break;
                    }
                }
            }
            if !keep_running {
                break;
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let reinstall_requested = ENGINE.with(|engine| {
            engine
                .borrow_mut()
                .as_mut()
                .expect("input engine is initialized")
                .take_hook_reinstall_request()
        });
        if reinstall_requested && replace_keyboard_hook(&mut hook, instance).is_ok() {
            ENGINE.with(|engine| {
                engine
                    .borrow_mut()
                    .as_mut()
                    .expect("input engine is initialized")
                    .hook_reinstalled();
            });
        }
    }

    ENGINE.with(|engine| {
        if let Some(engine) = engine.borrow_mut().as_mut() {
            engine.release_all();
        }
    });
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    drop(raw_input_window);
}

fn install_keyboard_hook(instance: HINSTANCE) -> Result<HHOOK, InputServiceError> {
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(instance), 0) }
        .map_err(|error| InputServiceError::Hook(error.to_string()))
}

fn replace_keyboard_hook(hook: &mut HHOOK, instance: HINSTANCE) -> Result<(), InputServiceError> {
    let replacement = install_keyboard_hook(instance)?;
    let previous = std::mem::replace(hook, replacement);
    let _ = unsafe { UnhookWindowsHookEx(previous) };
    Ok(())
}

unsafe extern "system" fn keyboard_proc(code: i32, message: WPARAM, l_param: LPARAM) -> LRESULT {
    if code != 0 {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    if l_param.0 == 0 {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    // WH_KEYBOARD_LL guarantees a KBDLLHOOKSTRUCT for HC_ACTION callbacks.
    let event = unsafe { &*(l_param.0 as *const KBDLLHOOKSTRUCT) };
    let Some(action) = action_for(message) else {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    };
    if event.flags.0 & LLKHF_INJECTED.0 != 0 && event.dwExtraInfo == INJECTION_TAG {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    let physical = PhysicalKey::new(event.scanCode as u16, event.flags.0 & 0x01 != 0);

    // SendInput from this thread can re-enter the hook while the engine is
    // borrowed above. Passing through on re-entry is safer than panicking
    // across the extern "system" boundary.
    let disposition = ENGINE.with(|engine| match engine.try_borrow_mut() {
        Ok(mut borrowed) => borrowed
            .as_mut()
            .expect("input engine is initialized")
            .process_hook(physical, action, Instant::now()),
        Err(_) => EventDisposition::PassThrough,
    });
    if disposition == EventDisposition::PassThrough {
        unsafe { CallNextHookEx(None, code, message, l_param) }
    } else {
        LRESULT(1)
    }
}

fn key_name(physical: PhysicalKey) -> String {
    let mut buffer = [0_u16; 64];
    let l_param =
        ((physical.scan_code as i32) << 16) | if physical.extended { 1_i32 << 24 } else { 0 };
    let length = unsafe { GetKeyNameTextW(l_param, &mut buffer) };
    if length > 0 {
        return String::from_utf16_lossy(&buffer[..length as usize]);
    }

    match (physical.scan_code, physical.extended) {
        (0x11, false) => "W".into(),
        (0x1F, false) => "S".into(),
        (0x1E, false) => "A".into(),
        (0x20, false) => "D".into(),
        (0x48, true) => "Up Arrow".into(),
        (0x50, true) => "Down Arrow".into(),
        (0x4B, true) => "Left Arrow".into(),
        (0x4D, true) => "Right Arrow".into(),
        _ => format!("Scan code 0x{:02X}", physical.scan_code),
    }
}

pub fn physical_key_name(physical: PhysicalKey) -> String {
    key_name(physical)
}

fn action_for(message: WPARAM) -> Option<KeyAction> {
    match message.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyAction::Down),
        WM_KEYUP | WM_SYSKEYUP => Some(KeyAction::Up),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_response_timeout_is_reported() {
        let (_sender, receiver) = mpsc::sync_channel::<()>(1);

        assert!(matches!(
            receive_service_response(receiver, Duration::ZERO),
            Err(InputServiceError::ServiceTimeout)
        ));
    }

    #[test]
    fn disconnected_service_response_is_reported() {
        let (sender, receiver) = mpsc::sync_channel::<()>(1);
        drop(sender);

        assert!(matches!(
            receive_service_response(receiver, COMMAND_ACK_TIMEOUT),
            Err(InputServiceError::ServiceStopped)
        ));
    }

    #[test]
    fn physical_key_names_hide_internal_representation() {
        for physical in [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x17, false),
            PhysicalKey::new(0x4D, true),
        ] {
            let name = physical_key_name(physical);
            assert!(!name.is_empty());
            assert!(!name.contains("scan_code"));
            assert!(!name.contains("false"));
            assert!(!name.contains("true"));
        }
    }

    #[test]
    fn stop_measurement_returns_the_final_snapshot_without_ui_polling() {
        let settings = Settings::default();
        let first = settings.binding(LogicalKey::HorizontalFirst);
        let second = settings.binding(LogicalKey::HorizontalSecond);
        let mut engine = InputEngine::new(settings);
        let (sender, _receiver) = mpsc::channel();
        let start = Instant::now();

        engine.start_measurement(sender);
        assert_eq!(
            {
                engine.process_raw(first, KeyAction::Down, start);
                engine.measurement_event_count
            },
            1
        );
        engine.process_raw(
            first,
            KeyAction::Up,
            start + std::time::Duration::from_millis(10),
        );
        engine.process_raw(
            second,
            KeyAction::Down,
            start + std::time::Duration::from_millis(15),
        );
        engine.process_raw(
            second,
            KeyAction::Up,
            start + std::time::Duration::from_millis(20),
        );

        let update = engine.stop_measurement().expect("final measurement update");
        assert_eq!(update.observed_event_count, 4);
        assert_eq!(update.statistics.sample_count(), 1);
        assert_eq!(update.statistics.transition_count(), 1);
        assert_eq!(update.statistics.overlap_count(), 0);
    }

    #[test]
    fn stop_measurement_without_configured_input_returns_none() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();

        engine.start_measurement(sender);

        assert!(engine.stop_measurement().is_none());
    }

    #[test]
    fn capture_is_delivered_on_raw_key_down() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, receiver) = mpsc::channel();
        let captured = PhysicalKey::new(0x17, false);
        engine.capture_sender = Some(sender);

        assert_eq!(
            {
                engine.process_raw(captured, KeyAction::Down, Instant::now());
                receiver.recv().expect("captured key").physical
            },
            captured
        );
        assert_eq!(engine.captured_key_awaiting_release, Some(captured));

        engine.process_raw(captured, KeyAction::Up, Instant::now());

        assert_eq!(engine.captured_key_awaiting_release, None);
    }

    #[test]
    fn capture_is_delivered_on_hook_key_down_when_available() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, receiver) = mpsc::channel();
        let captured = PhysicalKey::new(0x17, false);
        engine.capture_sender = Some(sender);

        assert_eq!(
            engine.process_hook(captured, KeyAction::Down, Instant::now()),
            EventDisposition::Consume
        );
        assert_eq!(receiver.recv().expect("captured key").physical, captured);
        assert_eq!(engine.captured_key_awaiting_release, Some(captured));
    }

    #[test]
    fn starting_measurement_cancels_a_pending_capture() {
        let mut engine = InputEngine::new(Settings::default());
        let (capture_sender, capture_receiver) = mpsc::channel();
        let (measurement_sender, _measurement_receiver) = mpsc::channel();
        engine.capture_sender = Some(capture_sender);
        engine.captured_key_awaiting_release = Some(PhysicalKey::new(0x17, false));

        engine.start_measurement(measurement_sender);

        assert!(matches!(
            capture_receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(engine.captured_key_awaiting_release, None);
        assert!(engine.measurement.is_some());
    }

    #[test]
    fn apply_keeps_the_captured_key_release_suppressed() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, receiver) = mpsc::channel();
        let captured = PhysicalKey::new(0x17, false);
        engine.capture_sender = Some(sender);

        engine.process_raw(captured, KeyAction::Down, Instant::now());
        assert_eq!(receiver.recv().expect("captured key").physical, captured);

        engine.apply(Settings::default());

        assert_eq!(
            engine.process_hook(captured, KeyAction::Up, Instant::now()),
            EventDisposition::Consume
        );
        assert_eq!(
            engine.process_hook(captured, KeyAction::Down, Instant::now()),
            EventDisposition::PassThrough
        );
    }

    #[test]
    fn hook_health_requests_reinstall_after_consecutive_raw_only_events() {
        let mut health = HookHealth::new();
        let key = PhysicalKey::new(0x1E, false);
        let start = Instant::now();

        for offset in 0..HOOK_MISSES_BEFORE_REINSTALL - 1 {
            health.observe_raw(
                key,
                KeyAction::Down,
                start + Duration::from_millis(u64::from(offset)),
            );
            assert!(!health.take_reinstall_request());
        }
        health.observe_raw(
            key,
            KeyAction::Down,
            start + Duration::from_millis(u64::from(HOOK_MISSES_BEFORE_REINSTALL)),
        );

        assert!(health.take_reinstall_request());
    }

    #[test]
    fn hook_health_matches_hook_and_raw_events_without_reinstalling() {
        let mut health = HookHealth::new();
        let first = PhysicalKey::new(0x1E, false);
        let second = PhysicalKey::new(0x20, false);
        let start = Instant::now();

        health.observe_hook(first, KeyAction::Down, start);
        health.observe_hook(second, KeyAction::Down, start + Duration::from_millis(1));
        health.observe_raw(first, KeyAction::Down, start + Duration::from_millis(2));
        health.observe_raw(second, KeyAction::Down, start + Duration::from_millis(3));

        assert_eq!(health.consecutive_misses, 0);
        assert!(!health.take_reinstall_request());
        assert!(health.observed_events.is_empty());
    }

    #[test]
    fn waitable_scheduler_can_be_created_armed_and_cancelled() {
        let mut scheduler = HighResolutionTimer::new().expect("waitable timer");

        scheduler
            .arm(Instant::now() + Duration::from_millis(5))
            .expect("arm waitable timer");
        scheduler.cancel();

        assert!(!scheduler.armed);
    }

    #[test]
    fn capture_waiting_passes_through_preexisting_key_release() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();
        let preexisting = PhysicalKey::new(0x1E, false);
        engine.capture_sender = Some(sender);

        assert_eq!(
            engine.process_hook(preexisting, KeyAction::Up, Instant::now()),
            EventDisposition::PassThrough
        );
        assert!(engine.capture_sender.is_some());
        assert_eq!(engine.captured_key_awaiting_release, None);
    }

    #[test]
    fn capture_consumes_only_its_own_key_down_and_release() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, receiver) = mpsc::channel();
        let captured = PhysicalKey::new(0x17, false);
        engine.capture_sender = Some(sender);

        assert_eq!(
            engine.process_hook(captured, KeyAction::Down, Instant::now()),
            EventDisposition::Consume
        );
        assert_eq!(receiver.recv().expect("captured key").physical, captured);

        assert_eq!(
            engine.process_hook(captured, KeyAction::Up, Instant::now()),
            EventDisposition::Consume
        );
        assert_eq!(engine.captured_key_awaiting_release, None);
    }

    #[test]
    fn measurement_boundaries_clear_physical_and_output_state() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();
        engine.start_measurement(sender);
        let _ = engine.stop_measurement();
        // After start/stop without SendInput, the timing state must be fresh:
        // a subsequent measurement session starts clean.
        let (second_sender, _second_receiver) = mpsc::channel();
        engine.start_measurement(second_sender);
        assert!(engine.measurement.is_some());
        assert_eq!(engine.measurement_event_count, 0);
    }

    #[test]
    fn captured_key_auto_repeat_never_reaches_hook_health() {
        let settings = Settings::default();
        let captured = settings.binding(LogicalKey::VerticalFirst);
        let mut engine = InputEngine::new(settings);
        let (sender, receiver) = mpsc::channel();
        engine.capture_sender = Some(sender);

        engine.process_raw(captured, KeyAction::Down, Instant::now());
        assert_eq!(receiver.recv().expect("captured key").physical, captured);

        // Auto-repeat while the captured key is still held.
        engine.process_raw(captured, KeyAction::Down, Instant::now());

        assert_eq!(engine.hook_health.consecutive_misses, 0);
        assert_eq!(engine.captured_key_awaiting_release, Some(captured));
    }
}
