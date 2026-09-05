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
/// Posted to the main thread when the keyboard hook is lost or recovered.
/// `wParam` is nonzero while lost. Read by `src/bin/lastkey.rs`.
pub const HOOK_STATUS_MESSAGE: u32 = WM_APP + 2;
const RAW_KEY_BREAK: u16 = 0x01;
const RAW_KEY_E0: u16 = 0x02;
const RAW_KEY_E1: u16 = 0x04;
const HOOK_EVENT_MAX_AGE: Duration = Duration::from_secs(1);
const HOOK_EVENT_QUEUE_CAPACITY: usize = 64;
const HOOK_MISSES_BEFORE_REINSTALL: u8 = 3;
const HOOK_REINSTALL_COOLDOWN: Duration = Duration::from_secs(2);
/// Consecutive reinstall failures before the hook counts as lost. Attempts
/// are spaced by the cooldown above, so notification needs no timer.
const HOOK_LOST_AFTER_FAILURES: u8 = 3;
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
        /// Commands queued behind a stall must not activate after the caller
        /// gave up and rolled persistence back.
        deadline: Instant,
        ready: mpsc::SyncSender<bool>,
    },
    ActiveSettings(mpsc::SyncSender<Settings>),
    Capture {
        sender: Sender<CapturedKey>,
        /// Like Apply and StartMeasurement: a capture that outlives its
        /// acknowledgement wait must not arm for a caller that gave up.
        deadline: Instant,
        ready: mpsc::SyncSender<Result<(), InputServiceError>>,
    },
    CancelCapture(mpsc::SyncSender<()>),
    StartMeasurement {
        sender: Sender<MeasurementUpdate>,
        /// Like Apply: a start that outlives its acknowledgement wait must
        /// not arm a session its caller already abandoned.
        deadline: Instant,
        ready: mpsc::SyncSender<Result<(), InputServiceError>>,
    },
    StopMeasurement(mpsc::SyncSender<Option<MeasurementUpdate>>),
    Stop,
}

impl InputService {
    pub fn start(
        settings: Settings,
        status_thread: u32,
    ) -> std::result::Result<Self, InputServiceError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("lastkey-input".into())
            .spawn(move || input_thread(settings, command_receiver, started_sender, status_thread))
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
            deadline: Instant::now() + COMMAND_ACK_TIMEOUT,
            ready: ready_sender,
        })?;
        if receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)? {
            Ok(())
        } else {
            // The engine discarded an expired command instead of activating
            // it; report the timeout so the caller reconciles the outcome.
            Err(InputServiceError::ServiceTimeout)
        }
    }

    /// Returns the settings the input engine is currently running. Queued
    /// behind any in-flight Apply, so the answer confirms its outcome.
    pub fn active_settings(&self) -> std::result::Result<Settings, InputServiceError> {
        self.request(InputCommand::ActiveSettings)
    }

    pub fn capture_next(&self) -> std::result::Result<Receiver<CapturedKey>, InputServiceError> {
        let (sender, receiver) = mpsc::channel();
        let deadline = Instant::now() + COMMAND_ACK_TIMEOUT;
        self.request(|ready| InputCommand::Capture {
            sender,
            deadline,
            ready,
        })??;
        Ok(receiver)
    }

    pub fn cancel_capture(&self) -> std::result::Result<(), InputServiceError> {
        self.request(InputCommand::CancelCapture)
    }

    pub fn start_measurement(
        &self,
    ) -> std::result::Result<Receiver<MeasurementUpdate>, InputServiceError> {
        let (sender, receiver) = mpsc::channel();
        let deadline = Instant::now() + COMMAND_ACK_TIMEOUT;
        self.request(|ready| InputCommand::StartMeasurement {
            sender,
            deadline,
            ready,
        })??;
        Ok(receiver)
    }

    pub fn stop_measurement(
        &self,
    ) -> std::result::Result<Option<MeasurementUpdate>, InputServiceError> {
        self.request(InputCommand::StopMeasurement)
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

    /// Sends one command and waits for its acknowledgement. Every request-shaped
    /// command shares this: a one-slot channel, `COMMAND_ACK_TIMEOUT`, and a
    /// disconnect that means the service is gone (see `receive_service_response`).
    /// Arming commands (Apply, Capture, StartMeasurement) additionally carry a
    /// deadline so a late activation cannot arm for a caller that gave up;
    /// disarming commands deliberately do not.
    fn request<T>(
        &self,
        command: impl FnOnce(mpsc::SyncSender<T>) -> InputCommand,
    ) -> std::result::Result<T, InputServiceError> {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        self.send(command(ready_sender))?;
        receive_service_response(ready_receiver, COMMAND_ACK_TIMEOUT)
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

    fn active_settings(&self) -> Result<Settings, String> {
        InputService::active_settings(self).map_err(|error| error.to_string())
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
    consecutive_reinstall_failures: u8,
    status_reported_as_lost: bool,
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
            consecutive_reinstall_failures: 0,
            status_reported_as_lost: false,
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
        self.consecutive_reinstall_failures = 0;
    }

    fn note_reinstall_failed(&mut self) {
        self.consecutive_reinstall_failures = self.consecutive_reinstall_failures.saturating_add(1);
    }

    /// Reports `Some(lost)` exactly once per lost/recovered transition, so a
    /// permanently dead hook notifies once instead of on every retry.
    fn take_hook_status_change(&mut self) -> Option<bool> {
        let lost = self.consecutive_reinstall_failures >= HOOK_LOST_AFTER_FAILURES;
        if lost == self.status_reported_as_lost {
            return None;
        }
        self.status_reported_as_lost = lost;
        Some(lost)
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
            if action == KeyAction::Down && is_capture_eligible(physical) {
                self.complete_capture(physical);
                return EventDisposition::Consume;
            }
            // A key held before capture started has no consumed key-down,
            // so its key-up must pass through to release existing output.
            // Ineligible modifiers pass through as well and keep waiting.
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
            if is_capture_eligible(physical) {
                self.complete_capture(physical);
            }
            return;
        }

        let Some(key) = self.settings.logical_key_for(physical) else {
            return;
        };
        if self.measurement.is_none() && self.capture_sender.is_none() {
            self.hook_health.observe_raw(physical, action, now);
        }
        if let Some(session) = self.measurement.as_mut() {
            let edges = session.edge_count();
            session.observe(key, action, now);
            // Repeat down or duplicate up: nothing observable changed.
            if session.edge_count() == edges {
                return;
            }
            let statistics = session.statistics();
            let update = MeasurementUpdate {
                observed_event_count: session.edge_count(),
                statistics,
                recommendation: recommend(statistics),
            };
            let delivered = self
                .measurement_sender
                .as_ref()
                .is_some_and(|sender| sender.send(update).is_ok());
            if !delivered {
                // The consumer is gone; running on would bypass SOCD with no
                // owner, so drop the session instead of feeding a dead queue.
                self.measurement = None;
                self.measurement_sender = None;
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
    }

    /// Activates settings unless the caller's acknowledgement wait already
    /// expired, in which case persistence was rolled back and a late
    /// activation would diverge from it. Returns whether it activated.
    /// Check-then-execute runs on the single input thread, and execution is
    /// immediate, so a command started in time always reports back in time.
    fn apply_if_current(&mut self, settings: Settings, deadline: Instant) -> bool {
        if Instant::now() > deadline {
            return false;
        }
        self.apply(settings);
        true
    }

    fn active_settings(&self) -> Settings {
        self.settings.clone()
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

    /// Entering capture reconciles live state first: any held output is
    /// released, physical SOCD state is cleared, and pending transitions are
    /// dropped. Otherwise a pre-held key's auto-repeat could become the
    /// captured input while its release is consumed, leaving output held
    /// with no key down to release it.
    fn begin_capture(&mut self, sender: Sender<CapturedKey>) {
        self.captured_key_awaiting_release = None;
        self.reset_timing_state();
        self.capture_sender = Some(sender);
    }

    /// Arms capture unless the caller's acknowledgement wait already expired,
    /// in which case the caller reported failure and dropped its receiver.
    /// Arming then would release live output and swallow the next keystroke
    /// for nobody. Returns whether it armed.
    fn begin_capture_if_current(&mut self, sender: Sender<CapturedKey>, deadline: Instant) -> bool {
        if Instant::now() > deadline {
            return false;
        }
        self.begin_capture(sender);
        true
    }

    fn start_measurement(&mut self, sender: Sender<MeasurementUpdate>) {
        self.capture_sender = None;
        self.captured_key_awaiting_release = None;
        self.reset_timing_state();
        self.measurement = Some(MeasurementSession::new());
        self.measurement_sender = Some(sender);
    }

    /// Arms a session unless the caller's acknowledgement wait already
    /// expired, in which case the caller reported failure and dropped its
    /// receiver. Arming then would bypass SOCD with no owner. Returns
    /// whether it armed.
    fn start_measurement_if_current(
        &mut self,
        sender: Sender<MeasurementUpdate>,
        deadline: Instant,
    ) -> bool {
        if Instant::now() > deadline {
            return false;
        }
        self.start_measurement(sender);
        true
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
        // Only a session that actually ran needs the timing reset: SOCD was
        // bypassed while it ran, so physical and output state are stale. With
        // no session there is nothing to reconcile, and resetting would
        // release live output — a UI disconnect must never interrupt SOCD.
        let Some(session) = self.measurement.take() else {
            self.measurement_sender = None;
            return None;
        };
        let update = (session.edge_count() > 0).then(|| {
            let statistics = session.statistics();
            MeasurementUpdate {
                observed_event_count: session.edge_count(),
                statistics,
                recommendation: recommend(statistics),
            }
        });
        self.measurement_sender = None;
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

    /// Records a failed reinstall attempt. Any output held at that moment is
    /// released first: with the hook gone, no release event will ever arrive
    /// to clear it, so leaving it held would stick a key down.
    fn hook_reinstall_failed(&mut self) {
        self.release_all();
        self.hook_health.note_reinstall_failed();
    }

    fn take_hook_status_change(&mut self) -> Option<bool> {
        self.hook_health.take_hook_status_change()
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

/// Whether raw input arrived without a device handle, marking it as injected
/// (our own `SendInput` output, or another app's) rather than physical.
/// Injected input must not reach hook-health: the hook never observes our own
/// emissions, so each one would count as a hook miss toward a spurious
/// reinstall (runtime invariant 4). Capture and measurement must not observe
/// it either: an armed capture would otherwise complete on our own output.
fn is_injected(header: &RAWINPUTHEADER) -> bool {
    header.hDevice.0.is_null()
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
    if is_injected(&raw.header) {
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
    status_thread: u32,
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
                    InputCommand::Apply {
                        settings,
                        deadline,
                        ready,
                    } => ENGINE.with(|engine| {
                        let activated = engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .apply_if_current(settings, deadline);
                        let _ = ready.send(activated);
                    }),
                    InputCommand::ActiveSettings(ready) => ENGINE.with(|engine| {
                        let settings = engine
                            .borrow()
                            .as_ref()
                            .expect("input engine is initialized")
                            .active_settings();
                        let _ = ready.send(settings);
                    }),
                    InputCommand::Capture {
                        sender,
                        deadline,
                        ready,
                    } => match raw_input_window.ensure_keyboard_registration() {
                        Ok(()) => ENGINE.with(|engine| {
                            let armed = engine
                                .borrow_mut()
                                .as_mut()
                                .expect("input engine is initialized")
                                .begin_capture_if_current(sender, deadline);
                            if armed {
                                let _ = ready.send(Ok(()));
                            } else {
                                let _ = ready.send(Err(InputServiceError::ServiceTimeout));
                            }
                        }),
                        Err(error) => {
                            let _ = ready.send(Err(error));
                        }
                    },
                    InputCommand::CancelCapture(ready) => ENGINE.with(|engine| {
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .cancel_capture();
                        let _ = ready.send(());
                    }),
                    InputCommand::StartMeasurement {
                        sender,
                        deadline,
                        ready,
                    } => match raw_input_window.ensure_keyboard_registration() {
                        Ok(()) => ENGINE.with(|engine| {
                            let started = engine
                                .borrow_mut()
                                .as_mut()
                                .expect("input engine is initialized")
                                .start_measurement_if_current(sender, deadline);
                            if started {
                                let _ = ready.send(Ok(()));
                            } else {
                                let _ = ready.send(Err(InputServiceError::ServiceTimeout));
                            }
                        }),
                        Err(error) => {
                            let _ = ready.send(Err(error));
                        }
                    },
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
        if reinstall_requested {
            let restored = replace_keyboard_hook(&mut hook, instance).is_ok();
            let change = ENGINE.with(|engine| {
                let mut engine = engine.borrow_mut();
                let engine = engine.as_mut().expect("input engine is initialized");
                if restored {
                    engine.hook_reinstalled();
                    // Keys pressed and released while the hook was gone were
                    // never observed, so physical state from before is stale.
                    engine.reset_timing_state();
                } else {
                    engine.hook_reinstall_failed();
                }
                engine.take_hook_status_change()
            });
            if let Some(lost) = change {
                let _ = unsafe {
                    PostThreadMessageW(
                        status_thread,
                        HOOK_STATUS_MESSAGE,
                        WPARAM(usize::from(lost)),
                        LPARAM(0),
                    )
                };
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

/// Capture is for SOCD movement keys. A bound modifier would be swallowed
/// globally, so modifiers stay eligible for normal typing instead: their
/// key-down passes through and capture keeps waiting.
fn is_capture_eligible(physical: PhysicalKey) -> bool {
    !matches!(
        (physical.scan_code, physical.extended),
        (0x2A, false) | (0x36, false) | (0x1D, _) | (0x38, _)
    )
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
                engine
                    .measurement
                    .as_ref()
                    .expect("measurement is active")
                    .edge_count()
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
        assert_eq!(update.statistics.sample_count, 1);
        assert_eq!(update.statistics.transition.count, 1);
        assert_eq!(update.statistics.overlap.count, 0);
    }

    #[test]
    fn stop_without_session_leaves_live_socd_state_untouched() {
        let settings = Settings::default();
        let key = LogicalKey::HorizontalFirst;
        let mut engine = InputEngine::new(settings);
        let mut emitter = TestEmitter { attempts: vec![] };
        let now = Instant::now();
        engine
            .timing
            .process(key, KeyAction::Down, now, &mut emitter);
        assert_eq!(emitter.attempts.len(), 1);

        assert!(engine.stop_measurement().is_none());

        // Still physically held, so this is a repeat: no new emission.
        // Counting (rather than asserting Consume) is what discriminates:
        // a cleared state would press and emit here.
        engine
            .timing
            .process(key, KeyAction::Down, now, &mut emitter);
        assert_eq!(emitter.attempts.len(), 1);
    }

    #[test]
    fn stop_measurement_without_configured_input_returns_none() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();

        engine.start_measurement(sender);

        assert!(engine.stop_measurement().is_none());
    }

    #[test]
    fn expired_measurement_start_arms_no_session() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();

        assert!(
            !engine.start_measurement_if_current(sender, Instant::now() - Duration::from_secs(1))
        );
        assert!(engine.measurement.is_none());
    }

    #[test]
    fn disconnected_consumer_drops_the_measurement_session() {
        let settings = Settings::default();
        let key = settings.binding(LogicalKey::HorizontalFirst);
        let mut engine = InputEngine::new(settings);
        let (sender, receiver) = mpsc::channel();
        engine.start_measurement(sender);
        drop(receiver);

        engine.process_raw(key, KeyAction::Down, Instant::now());

        assert!(engine.measurement.is_none());
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

    struct TestEmitter {
        attempts: Vec<(LogicalKey, KeyAction)>,
    }

    impl OutputEmitter for TestEmitter {
        fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
            self.attempts.push((key, action));
            true
        }
    }

    fn enabled_timing_settings() -> Settings {
        let mut settings = Settings::default();
        settings.timing.socd_transition_delay_enabled = true;
        settings
    }

    #[test]
    fn begin_capture_clears_stale_physical_state_without_emitting() {
        let mut engine = InputEngine::new(enabled_timing_settings());
        let mut emitter = TestEmitter { attempts: vec![] };
        let start = Instant::now();

        // A Down, then D Down: A is released through the fake emitter and D
        // is left pending, so output is empty while A stays physically held.
        engine.timing.process(
            LogicalKey::HorizontalFirst,
            KeyAction::Down,
            start,
            &mut emitter,
        );
        engine.timing.process(
            LogicalKey::HorizontalSecond,
            KeyAction::Down,
            start,
            &mut emitter,
        );

        let (sender, _receiver) = mpsc::channel();
        engine.begin_capture(sender);
        assert!(engine.capture_sender.is_some());

        // The stale physical hold is gone: the release passes through to the
        // timing core instead of resurrecting output.
        let attempts = emitter.attempts.len();
        assert_eq!(
            engine.timing.process(
                LogicalKey::HorizontalFirst,
                KeyAction::Up,
                start,
                &mut emitter,
            ),
            EventDisposition::PassThrough
        );
        assert_eq!(emitter.attempts.len(), attempts);
    }

    #[test]
    fn capture_repeat_then_release_leaves_no_held_output() {
        let mut engine = InputEngine::new(Settings::default());
        let key = LogicalKey::HorizontalFirst;
        let physical = engine.settings.binding(key);
        let (sender, receiver) = mpsc::channel();
        engine.begin_capture(sender);

        // Auto-repeat of a pre-held key becomes the captured input.
        assert_eq!(
            engine.process_hook(physical, KeyAction::Down, Instant::now()),
            EventDisposition::Consume
        );
        assert_eq!(receiver.recv().expect("captured key").physical, physical);
        // Its release is consumed by the capture contract.
        assert_eq!(
            engine.process_hook(physical, KeyAction::Up, Instant::now()),
            EventDisposition::Consume
        );
        engine.cancel_capture();

        assert_eq!(
            engine.timing.output_state(key),
            crate::core::DeliveryState::NotHeld
        );
        // A fresh press works instead of being swallowed as a repeat.
        let mut emitter = TestEmitter { attempts: vec![] };
        engine
            .timing
            .process(key, KeyAction::Down, Instant::now(), &mut emitter);
        assert_eq!(emitter.attempts.len(), 1);
    }

    #[test]
    fn expired_capture_arms_no_capture() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, _receiver) = mpsc::channel();

        assert!(!engine.begin_capture_if_current(sender, Instant::now() - Duration::from_secs(1)));
        assert!(engine.capture_sender.is_none());
    }

    #[test]
    fn expired_capture_preserves_live_output() {
        let settings = Settings::default();
        let key = LogicalKey::HorizontalFirst;
        let mut engine = InputEngine::new(settings);
        let mut emitter = TestEmitter { attempts: vec![] };
        engine
            .timing
            .process(key, KeyAction::Down, Instant::now(), &mut emitter);
        assert_eq!(emitter.attempts.len(), 1);

        let (sender, _receiver) = mpsc::channel();
        assert!(!engine.begin_capture_if_current(sender, Instant::now() - Duration::from_secs(1)));
        assert!(engine.capture_sender.is_none());

        // Still held live: no release was emitted for the skipped capture.
        assert_eq!(emitter.attempts.len(), 1);
        assert_eq!(
            engine.timing.output_state(key),
            crate::core::DeliveryState::SyntheticHeld
        );
    }

    #[test]
    fn modifier_scan_codes_are_not_capture_eligible() {
        for physical in [
            PhysicalKey::new(0x2A, false),
            PhysicalKey::new(0x36, false),
            PhysicalKey::new(0x1D, false),
            PhysicalKey::new(0x1D, true),
            PhysicalKey::new(0x38, false),
            PhysicalKey::new(0x38, true),
        ] {
            assert!(!is_capture_eligible(physical));
        }
        for physical in [
            PhysicalKey::new(0x11, false),
            PhysicalKey::new(0x1E, false),
            PhysicalKey::new(0x48, true),
        ] {
            assert!(is_capture_eligible(physical));
        }
    }

    #[test]
    fn modifier_down_during_capture_passes_through_and_keeps_waiting() {
        let mut engine = InputEngine::new(Settings::default());
        let (sender, receiver) = mpsc::channel();
        engine.capture_sender = Some(sender);
        let shift = PhysicalKey::new(0x2A, false);

        assert_eq!(
            engine.process_hook(shift, KeyAction::Down, Instant::now()),
            EventDisposition::PassThrough
        );
        engine.process_raw(shift, KeyAction::Down, Instant::now());

        assert!(engine.capture_sender.is_some());
        assert!(receiver.try_recv().is_err());
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
    fn expired_apply_is_skipped_without_touching_settings() {
        let mut engine = InputEngine::new(Settings::default());
        let mut next = Settings::default();
        next.timing.socd_transition_delay_enabled = true;

        assert!(!engine.apply_if_current(next, Instant::now() - Duration::from_secs(1)));
        assert_eq!(engine.settings, Settings::default());

        assert!(
            engine.apply_if_current(Settings::default(), Instant::now() + Duration::from_secs(2))
        );
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
    fn hook_status_changes_once_per_lost_and_recovered_transition() {
        let mut health = HookHealth::new();
        assert_eq!(health.take_hook_status_change(), None);

        for _ in 0..HOOK_LOST_AFTER_FAILURES - 1 {
            health.note_reinstall_failed();
            assert_eq!(health.take_hook_status_change(), None);
        }
        health.note_reinstall_failed();
        assert_eq!(health.take_hook_status_change(), Some(true));
        // A permanently dead hook notifies once, not on every retry.
        health.note_reinstall_failed();
        assert_eq!(health.take_hook_status_change(), None);

        health.reinstalled();
        assert_eq!(health.take_hook_status_change(), Some(false));
        assert_eq!(health.take_hook_status_change(), None);
    }

    #[test]
    fn injected_raw_input_is_identified_by_a_null_device() {
        let mut header = RAWINPUTHEADER {
            dwType: RIM_TYPEKEYBOARD.0,
            dwSize: 0,
            hDevice: HANDLE(std::ptr::null_mut()),
            wParam: WPARAM(0),
        };
        assert!(is_injected(&header));

        header.hDevice = HANDLE(std::ptr::without_provenance_mut(1));
        assert!(!is_injected(&header));
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
        assert_eq!(
            engine
                .measurement
                .as_ref()
                .expect("measurement is active")
                .edge_count(),
            0
        );
    }

    #[test]
    fn repeat_down_produces_no_measurement_update() {
        let settings = Settings::default();
        let key = settings.binding(LogicalKey::HorizontalFirst);
        let mut engine = InputEngine::new(settings);
        let (sender, receiver) = mpsc::channel();
        engine.start_measurement(sender);

        engine.process_raw(key, KeyAction::Down, Instant::now());
        receiver.recv().expect("first edge reports");

        engine.process_raw(key, KeyAction::Down, Instant::now());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
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
