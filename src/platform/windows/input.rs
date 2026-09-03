use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::c_void,
    mem::size_of,
    time::{Duration, Instant},
};
use std::{
    fmt,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
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
    core::{
        EventDisposition, KeyAction, LogicalKey, MeasurementSession, MeasurementStatistics,
        OutputEmitter, PhysicalKey, TimingController, TimingRecommendation, recommend,
    },
    debug_log,
    settings::Settings,
};

const INJECTION_TAG: usize = 0x4C41_5354_4B45_5931; // "LASTKEY1"

const COMMAND_MESSAGE: u32 = WM_APP + 1;
const RAW_KEY_BREAK: u16 = 0x01;
const RAW_KEY_E0: u16 = 0x02;
const RAW_KEY_E1: u16 = 0x04;
const HOOK_EVENT_MAX_AGE: Duration = Duration::from_secs(1);
const HOOK_EVENT_QUEUE_CAPACITY: usize = 64;
const HOOK_MISSES_BEFORE_REINSTALL: u8 = 3;
const HOOK_REINSTALL_COOLDOWN: Duration = Duration::from_secs(2);
const WAIT_OBJECT_0_VALUE: u32 = 0;
const WAIT_FAILED_VALUE: u32 = u32::MAX;

thread_local! {
    static ENGINE: RefCell<Option<InputEngine>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct CapturedKey {
    pub physical: PhysicalKey,
    pub name: String,
}

#[derive(Clone, Copy, Debug)]
pub struct MeasurementUpdate {
    pub observed_event_count: u32,
    pub statistics: MeasurementStatistics,
    pub recommendation: TimingRecommendation,
}

#[derive(Debug)]
pub enum InputServiceError {
    Hook(String),
    ServiceStopped,
}

impl fmt::Display for InputServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hook(message) => write!(formatter, "Windows keyboard hook failed: {message}"),
            Self::ServiceStopped => write!(formatter, "the input service is no longer running"),
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
    Apply(Settings),
    Capture {
        sender: Sender<CapturedKey>,
        ready: mpsc::SyncSender<Result<(), InputServiceError>>,
    },
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

        let thread_id = started_receiver
            .recv()
            .map_err(|_| InputServiceError::ServiceStopped)??;
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
        self.send(InputCommand::Apply(settings))
    }

    pub fn capture_next(&self) -> std::result::Result<Receiver<CapturedKey>, InputServiceError> {
        let (sender, receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::Capture {
            sender,
            ready: ready_sender,
        })?;
        ready_receiver
            .recv()
            .map_err(|_| InputServiceError::ServiceStopped)??;
        Ok(receiver)
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
        ready_receiver
            .recv()
            .map_err(|_| InputServiceError::ServiceStopped)??;
        Ok(receiver)
    }

    pub fn stop_measurement(
        &self,
    ) -> std::result::Result<Option<MeasurementUpdate>, InputServiceError> {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(InputCommand::StopMeasurement(ready_sender))?;
        ready_receiver
            .recv()
            .map_err(|_| InputServiceError::ServiceStopped)
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

struct InputEngine {
    timing: TimingController,
    settings: Settings,
    capture_sender: Option<Sender<CapturedKey>>,
    measurement: Option<MeasurementSession>,
    measurement_sender: Option<Sender<MeasurementUpdate>>,
    measurement_event_count: u32,
    capture_suppressed_key: Option<PhysicalKey>,
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
    high_resolution: bool,
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
        let (handle, high_resolution) = match high_resolution {
            Ok(handle) => (handle, true),
            Err(error) => {
                debug_log::write(format_args!(
                    "timing scheduler high-resolution creation failed; falling back error={error}"
                ));
                let handle = unsafe { CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0) }
                    .map_err(|fallback_error| {
                    InputServiceError::Hook(fallback_error.to_string())
                })?;
                (handle, false)
            }
        };
        debug_log::write(format_args!(
            "timing scheduler created kind=waitable-timer high_resolution={high_resolution} handle=0x{:X}",
            handle.0 as usize,
        ));
        Ok(Self {
            handle,
            armed: false,
            high_resolution,
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
        debug_log::write(format_args!(
            "timing scheduler armed kind=waitable-timer high_resolution={} requested_wait_us={}",
            self.high_resolution,
            remaining.as_micros()
        ));
        Ok(())
    }

    fn cancel(&mut self) {
        if !self.armed {
            return;
        }
        match unsafe { CancelWaitableTimer(self.handle) } {
            Ok(()) => debug_log::write(format_args!(
                "timing scheduler cancelled kind=waitable-timer"
            )),
            Err(error) => debug_log::write(format_args!(
                "timing scheduler cancel-failed kind=waitable-timer error={error}"
            )),
        }
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
        debug_log::write(format_args!(
            "keyboard hook health miss count={} threshold={} key={} scan=0x{:02X} action={action:?}",
            self.consecutive_misses,
            HOOK_MISSES_BEFORE_REINSTALL,
            key_name(physical),
            physical.scan_code
        ));
        let cooldown_elapsed = self
            .last_reinstall_request
            .is_none_or(|last| now.saturating_duration_since(last) >= HOOK_REINSTALL_COOLDOWN);
        if self.consecutive_misses >= HOOK_MISSES_BEFORE_REINSTALL && cooldown_elapsed {
            self.reinstall_requested = true;
            self.last_reinstall_request = Some(now);
            debug_log::write(format_args!(
                "keyboard hook health requested reinstall after {} consecutive misses",
                self.consecutive_misses
            ));
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
            capture_suppressed_key: None,
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
        self.hook_health.observe_hook(physical, action, now);
        debug_log::write(format_args!(
            "hook engine event key={} scan=0x{:02X} extended={} action={action:?} capture_waiting={} measurement_active={}",
            key_name(physical),
            physical.scan_code,
            physical.extended,
            self.capture_sender.is_some(),
            self.measurement.is_some()
        ));
        if self.capture_suppressed_key == Some(physical) {
            if action == KeyAction::Up {
                self.capture_suppressed_key = None;
                debug_log::write(format_args!(
                    "hook consumed captured key-up key={} scan=0x{:02X}",
                    key_name(physical),
                    physical.scan_code
                ));
            }
            return EventDisposition::Consume;
        }

        if self.capture_sender.is_some() {
            if action == KeyAction::Down {
                self.complete_capture(physical, "hook");
            }
            debug_log::write(format_args!("hook consumed event for key capture"));
            return EventDisposition::Consume;
        }

        let Some(key) = self.settings.logical_key_for(physical) else {
            debug_log::write(format_args!(
                "hook event is not a configured key; passing through"
            ));
            return EventDisposition::PassThrough;
        };
        if self.measurement.is_some() {
            debug_log::write(format_args!(
                "hook passed configured event to raw measurement path logical={key:?}"
            ));
            return EventDisposition::PassThrough;
        }
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        let disposition = self.timing.process(key, action, now, &mut emitter);
        if self.timing.is_enabled() {
            self.update_timer();
        }
        debug_log::write(format_args!(
            "mapping event logical={key:?} disposition={disposition:?}"
        ));
        disposition
    }

    fn process_raw(&mut self, physical: PhysicalKey, action: KeyAction, now: Instant) {
        debug_log::write(format_args!(
            "raw engine event key={} scan=0x{:02X} extended={} action={action:?} capture_waiting={} measurement_active={}",
            key_name(physical),
            physical.scan_code,
            physical.extended,
            self.capture_sender.is_some(),
            self.measurement.is_some()
        ));
        if action == KeyAction::Up && self.capture_suppressed_key == Some(physical) {
            self.capture_suppressed_key = None;
            debug_log::write(format_args!(
                "raw observed captured key-up and cleared suppression key={} scan=0x{:02X}",
                key_name(physical),
                physical.scan_code
            ));
            return;
        }
        if action == KeyAction::Down && self.capture_sender.is_some() {
            self.complete_capture(physical, "raw");
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
            debug_log::write(format_args!(
                "measurement edge logical={key:?} edges={} samples={} near_simultaneous={} transitions={} overlaps={}",
                self.measurement_event_count,
                statistics.sample_count(),
                statistics.near_simultaneous_count(),
                statistics.transition_count(),
                statistics.overlap_count()
            ));
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

    fn start_measurement(&mut self, sender: Sender<MeasurementUpdate>) {
        let capture_cancelled = self.capture_sender.take().is_some();
        self.capture_suppressed_key = None;
        self.release_all();
        self.measurement = Some(MeasurementSession::new());
        self.measurement_sender = Some(sender);
        self.measurement_event_count = 0;
        debug_log::write(format_args!(
            "measurement engine started pending_capture_cancelled={capture_cancelled}"
        ));
    }

    fn complete_capture(&mut self, physical: PhysicalKey, source: &str) {
        let Some(sender) = self.capture_sender.take() else {
            return;
        };
        let captured = CapturedKey {
            physical,
            name: key_name(physical),
        };
        let delivered = sender.send(captured.clone()).is_ok();
        self.capture_suppressed_key = Some(physical);
        debug_log::write(format_args!(
            "{source} capture completed on key-down captured={captured:?} delivered={delivered}"
        ));
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
        debug_log::write(format_args!(
            "measurement engine stopped final_update={update:?}"
        ));
        update
    }

    fn update_timer(&mut self) {
        self.cancel_timer();
        if let Some(deadline) = self.timing.next_deadline()
            && let Some(scheduler) = self.scheduler.as_mut()
            && let Err(error) = scheduler.arm(deadline)
        {
            debug_log::write(format_args!(
                "timing scheduler arm-failed kind=waitable-timer error={error}"
            ));
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

        debug_log::write(format_args!(
            "synthetic output attempt logical={key:?} physical={} scan=0x{:02X} extended={} action={action:?}",
            key_name(physical),
            physical.scan_code,
            physical.extended
        ));
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        if sent == 1 {
            debug_log::write(format_args!(
                "synthetic output completed logical={key:?} action={action:?} success=true"
            ));
            true
        } else {
            debug_log::write(format_args!(
                "synthetic output completed logical={key:?} action={action:?} success=false sent={sent} error={}",
                unsafe { GetLastError() }.0
            ));
            false
        }
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
        raw_input_window.ensure_keyboard_registration("input-thread-startup")?;
        Ok(raw_input_window)
    }

    fn ensure_keyboard_registration(&self, reason: &str) -> Result<(), InputServiceError> {
        let before = registered_keyboard_target()?;
        debug_log::write(format_args!(
            "raw input registration before reason={reason:?} expected=0x{:X} actual={}",
            self.window.0 as usize,
            format_window_handle(before)
        ));

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
        debug_log::write(format_args!(
            "raw input registration after reason={reason:?} expected=0x{:X} actual={}",
            self.window.0 as usize,
            format_window_handle(after)
        ));
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
        debug_log::write(format_args!("raw input window destroyed"));
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
        debug_log::write(format_args!(
            "raw input read failed copied={copied} byte_count={byte_count}"
        ));
        return;
    }

    let raw = unsafe { raw.assume_init() };
    if raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return;
    }
    let keyboard = unsafe { raw.data.keyboard };
    if keyboard.MakeCode == 0 {
        debug_log::write(format_args!(
            "raw keyboard event ignored zero make-code vkey=0x{:02X} flags=0x{:02X}",
            keyboard.VKey, keyboard.Flags
        ));
        return;
    }

    let action = if keyboard.Flags & RAW_KEY_BREAK != 0 {
        KeyAction::Up
    } else {
        KeyAction::Down
    };
    let extended = keyboard.Flags & (RAW_KEY_E0 | RAW_KEY_E1) != 0;
    let physical = PhysicalKey::new(keyboard.MakeCode, extended);
    debug_log::write(format_args!(
        "raw event key={} scan=0x{:02X} extended={} action={action:?} vkey=0x{:02X} flags=0x{:02X}",
        key_name(physical),
        physical.scan_code,
        physical.extended,
        keyboard.VKey,
        keyboard.Flags
    ));
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
    debug_log::write(format_args!("input thread starting"));
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
    debug_log::write(format_args!(
        "keyboard hook installed handle=0x{:X} module=0x{:X}",
        hook.0 as usize, instance.0 as usize
    ));
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
            ENGINE.with(|engine| {
                engine
                    .borrow_mut()
                    .as_mut()
                    .expect("input engine is initialized")
                    .handle_timer_signal();
            });
            continue;
        }
        if wait_result.0 == WAIT_FAILED_VALUE {
            debug_log::write(format_args!(
                "timing scheduler wait failed error={}",
                unsafe { GetLastError() }.0
            ));
            break;
        }
        if wait_result.0 != WAIT_OBJECT_0_VALUE + 1 {
            debug_log::write(format_args!(
                "timing scheduler wait returned unexpected result={}",
                wait_result.0
            ));
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
                    InputCommand::Apply(settings) => ENGINE.with(|engine| {
                        debug_log::write(format_args!("command apply received"));
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .apply(settings);
                    }),
                    InputCommand::Capture { sender, ready } => {
                        debug_log::write(format_args!("command capture received"));
                        match raw_input_window.ensure_keyboard_registration("capture") {
                            Ok(()) => {
                                ENGINE.with(|engine| {
                                    let mut engine = engine.borrow_mut();
                                    let engine =
                                        engine.as_mut().expect("input engine is initialized");
                                    engine.capture_sender = Some(sender);
                                });
                                debug_log::write(format_args!("command capture armed"));
                                let _ = ready.send(Ok(()));
                            }
                            Err(error) => {
                                debug_log::write(format_args!(
                                    "command capture registration failed error={error}"
                                ));
                                let _ = ready.send(Err(error));
                            }
                        }
                    }
                    InputCommand::StartMeasurement { sender, ready } => {
                        debug_log::write(format_args!("command start-measurement received"));
                        match raw_input_window.ensure_keyboard_registration("measurement") {
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
                                debug_log::write(format_args!(
                                    "command start-measurement registration failed error={error}"
                                ));
                                let _ = ready.send(Err(error));
                            }
                        }
                    }
                    InputCommand::StopMeasurement(ready) => ENGINE.with(|engine| {
                        debug_log::write(format_args!("command stop-measurement received"));
                        let update = engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .stop_measurement();
                        let _ = ready.send(update);
                    }),
                    InputCommand::Stop => {
                        debug_log::write(format_args!("command stop received"));
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
        if reinstall_requested {
            match replace_keyboard_hook(&mut hook, instance) {
                Ok(()) => ENGINE.with(|engine| {
                    engine
                        .borrow_mut()
                        .as_mut()
                        .expect("input engine is initialized")
                        .hook_reinstalled();
                }),
                Err(error) => {
                    debug_log::write(format_args!("keyboard hook reinstall failed error={error}"))
                }
            }
        }
    }

    ENGINE.with(|engine| {
        if let Some(engine) = engine.borrow_mut().as_mut() {
            engine.release_all();
        }
    });
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    drop(raw_input_window);
    debug_log::write(format_args!(
        "keyboard hook uninstalled; input thread stopped"
    ));
}

fn install_keyboard_hook(instance: HINSTANCE) -> Result<HHOOK, InputServiceError> {
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(instance), 0) }
        .map_err(|error| InputServiceError::Hook(error.to_string()))
}

fn replace_keyboard_hook(hook: &mut HHOOK, instance: HINSTANCE) -> Result<(), InputServiceError> {
    debug_log::write(format_args!(
        "keyboard hook reinstall starting previous_handle=0x{:X}",
        hook.0 as usize
    ));
    let replacement = install_keyboard_hook(instance)?;
    let previous = std::mem::replace(hook, replacement);
    let removed = unsafe { UnhookWindowsHookEx(previous) }.is_ok();
    debug_log::write(format_args!(
        "keyboard hook reinstall completed new_handle=0x{:X} previous_removed={removed}",
        hook.0 as usize
    ));
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
        debug_log::write(format_args!(
            "synthetic output observed-by-hook scan=0x{:02X} action={action:?} flags=0x{:02X}",
            event.scanCode, event.flags.0
        ));
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    let physical = PhysicalKey::new(event.scanCode as u16, event.flags.0 & 0x01 != 0);
    debug_log::write(format_args!(
        "hook event key={} scan=0x{:02X} extended={} action={action:?} flags=0x{:02X}",
        key_name(physical),
        physical.scan_code,
        physical.extended,
        event.flags.0
    ));

    let disposition = ENGINE.with(|engine| {
        engine
            .borrow_mut()
            .as_mut()
            .expect("input engine is initialized")
            .process_hook(physical, action, Instant::now())
    });
    if disposition == EventDisposition::PassThrough {
        debug_log::write(format_args!("hook disposition=pass-through"));
        unsafe { CallNextHookEx(None, code, message, l_param) }
    } else {
        debug_log::write(format_args!("hook disposition=consume"));
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
        assert_eq!(engine.capture_suppressed_key, Some(captured));

        engine.process_raw(captured, KeyAction::Up, Instant::now());

        assert_eq!(engine.capture_suppressed_key, None);
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
        assert_eq!(engine.capture_suppressed_key, Some(captured));
    }

    #[test]
    fn starting_measurement_cancels_a_pending_capture() {
        let mut engine = InputEngine::new(Settings::default());
        let (capture_sender, capture_receiver) = mpsc::channel();
        let (measurement_sender, _measurement_receiver) = mpsc::channel();
        engine.capture_sender = Some(capture_sender);
        engine.capture_suppressed_key = Some(PhysicalKey::new(0x17, false));

        engine.start_measurement(measurement_sender);

        assert!(matches!(
            capture_receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(engine.capture_suppressed_key, None);
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
}
