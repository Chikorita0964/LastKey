use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode, uinput::VirtualDevice};

use crate::{
    core::{EventDisposition, KeyAction, LogicalKey, OutputEmitter, PhysicalKey, TimingController},
    settings::Settings,
};

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const VIRTUAL_KEYBOARD_NAME: &str = "LastKey virtual keyboard";

#[derive(Debug)]
pub enum InputServiceError {
    Device(String),
    ServiceStopped,
}

impl fmt::Display for InputServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Device(message) => write!(f, "Linux input backend failed: {message}"),
            Self::ServiceStopped => write!(f, "the input service is no longer running"),
        }
    }
}
impl std::error::Error for InputServiceError {}

pub struct InputService {
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
enum Command {
    Apply(Settings),
    Stop,
}
struct CapturedEvent {
    key: KeyCode,
    value: i32,
    received_at: Instant,
}

impl InputService {
    pub fn start(settings: Settings) -> Result<Self, InputServiceError> {
        settings
            .validate()
            .map_err(|error| InputServiceError::Device(error.to_string()))?;
        let (commands, receiver) = mpsc::channel();
        let (started, started_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("lastkey-linux-input".into())
            .spawn(move || run(settings, receiver, thread_stop, started))
            .map_err(|error| InputServiceError::Device(error.to_string()))?;
        started_receiver
            .recv()
            .map_err(|_| InputServiceError::ServiceStopped)??;
        Ok(Self {
            commands,
            stop,
            thread: Some(thread),
        })
    }

    pub fn apply(&self, settings: Settings) -> Result<(), InputServiceError> {
        settings
            .validate()
            .map_err(|error| InputServiceError::Device(error.to_string()))?;
        self.commands
            .send(Command::Apply(settings))
            .map_err(|_| InputServiceError::ServiceStopped)
    }

    fn stop_inner(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for InputService {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

fn run(
    settings: Settings,
    commands: Receiver<Command>,
    stop: Arc<AtomicBool>,
    started: mpsc::SyncSender<Result<(), InputServiceError>>,
) {
    let mut output = match create_output() {
        Ok(device) => device,
        Err(error) => {
            let _ = started.send(Err(error));
            return;
        }
    };
    let (events, event_receiver) = mpsc::channel();
    let mut readers = start_readers(&settings, events, Arc::clone(&stop));
    if readers.is_empty() {
        let _ = started.send(Err(InputServiceError::Device(
            "no accessible keyboard device exposes all configured keys".into(),
        )));
        return;
    }
    let _ = started.send(Ok(()));
    let mut settings = settings;
    let mut timing = TimingController::new(settings.timing.clone());
    while !stop.load(Ordering::Acquire) {
        // Wake early for pending timing deadlines but never sleep past the
        // 5 ms input bound: std mpsc has no select(), so a deadline-only wait
        // would starve commands or physical events on the other channel.
        // Reader threads stay on non-blocking reads plus sleep so shutdown can
        // join them without an extra wakeup fd; see read_device below.
        let wait = timing
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(POLL_INTERVAL)
            .min(POLL_INTERVAL);
        match commands.recv_timeout(wait) {
            Ok(Command::Apply(next)) => {
                let mut emitter = LinuxEmitter {
                    output: &mut output,
                    settings: &settings,
                };
                timing.release_all(&mut emitter);
                settings = next;
                timing = TimingController::new(settings.timing.clone());
            }
            Ok(Command::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(event) = event_receiver.try_recv() {
            if let Some((key, action)) = logical_event(&settings, event.key, event.value) {
                let mut emitter = LinuxEmitter {
                    output: &mut output,
                    settings: &settings,
                };
                // Mirror the Windows CallNextHookEx semantics: a PassThrough
                // disposition means the original event must still reach
                // applications through the virtual device.
                if timing.process(key, action, event.received_at, &mut emitter)
                    == EventDisposition::PassThrough
                {
                    let _ =
                        output.emit(&[InputEvent::new(EventType::KEY.0, event.key.0, event.value)]);
                }
            } else {
                let _ = output.emit(&[InputEvent::new(EventType::KEY.0, event.key.0, event.value)]);
            }
        }
        let mut emitter = LinuxEmitter {
            output: &mut output,
            settings: &settings,
        };
        timing.poll(Instant::now(), &mut emitter);
    }
    let mut emitter = LinuxEmitter {
        output: &mut output,
        settings: &settings,
    };
    timing.release_all(&mut emitter);
    stop.store(true, Ordering::Release);
    for reader in readers.drain(..) {
        let _ = reader.join();
    }
}

fn create_output() -> Result<VirtualDevice, InputServiceError> {
    let mut keys = AttributeSet::new();
    for code in 1..=767 {
        keys.insert(KeyCode::new(code));
    }
    VirtualDevice::builder()
        .and_then(|builder| builder.name(&VIRTUAL_KEYBOARD_NAME).with_keys(&keys))
        .and_then(|builder| builder.build())
        .map_err(|error| InputServiceError::Device(error.to_string()))
}

fn start_readers(
    settings: &Settings,
    sender: Sender<CapturedEvent>,
    stop: Arc<AtomicBool>,
) -> Vec<JoinHandle<()>> {
    evdev::enumerate()
        .filter_map(|(path, mut device)| {
            if !is_keyboard_candidate(&device, settings)
                || device.grab().is_err()
                || device.set_nonblocking(true).is_err()
            {
                return None;
            }
            let sender = sender.clone();
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name(format!("lastkey-{}", path.display()))
                .spawn(move || read_device(device, sender, stop))
                .ok()
        })
        .collect()
}

fn read_device(mut device: Device, sender: Sender<CapturedEvent>, stop: Arc<AtomicBool>) {
    // Non-blocking reads plus a short sleep keep shutdown joinable without an
    // eventfd: a blocking fetch_events() would need an extra wakeup path.
    while !stop.load(Ordering::Acquire) {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if event.event_type() == EventType::KEY
                        && sender
                            .send(CapturedEvent {
                                key: KeyCode::new(event.code()),
                                value: event.value(),
                                received_at: Instant::now(),
                            })
                            .is_err()
                    {
                        return;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL)
            }
            Err(_) => return,
        }
    }
    let _ = device.ungrab();
}

/// Windows scan-code bindings are shared with Linux through the same
/// settings file, but evdev uses Linux keycodes and has no `extended` flag.
/// Plain keys such as W/A/S/D coincide numerically, while extended keys do
/// not (for example the up arrow is `0x48 + extended` on Windows but
/// `KEY_UP = 103` on Linux). Bindings without a known Linux mapping return
/// `None` so they are rejected loudly instead of resolving to the wrong key.
fn linux_keycode(binding: PhysicalKey) -> Option<u16> {
    match (binding.scan_code, binding.extended) {
        (0x48, true) => Some(103),
        (0x50, true) => Some(108),
        (0x4B, true) => Some(105),
        (0x4D, true) => Some(106),
        (code, false) => Some(code),
        _ => None,
    }
}

fn physical_from_linux(code: u16) -> PhysicalKey {
    match code {
        103 => PhysicalKey::new(0x48, true),
        108 => PhysicalKey::new(0x50, true),
        105 => PhysicalKey::new(0x4B, true),
        106 => PhysicalKey::new(0x4D, true),
        _ => PhysicalKey::new(code, false),
    }
}

fn is_keyboard_candidate(device: &Device, settings: &Settings) -> bool {
    // Never grab our own virtual output: it exposes every key and would pass
    // the candidate check below, blocking other programs from our output.
    if device
        .name()
        .is_some_and(|name| name == VIRTUAL_KEYBOARD_NAME)
    {
        return false;
    }
    device.supported_keys().is_some_and(|keys| {
        LogicalKey::ALL.into_iter().all(|key| {
            linux_keycode(settings.binding(key))
                .is_some_and(|code| keys.contains(KeyCode::new(code)))
        })
    })
}

fn logical_event(
    settings: &Settings,
    code: KeyCode,
    value: i32,
) -> Option<(LogicalKey, KeyAction)> {
    let action = match value {
        0 => KeyAction::Up,
        1 | 2 => KeyAction::Down,
        _ => return None,
    };
    settings
        .logical_key_for(physical_from_linux(code.0))
        .map(|key| (key, action))
}

struct LinuxEmitter<'a> {
    output: &'a mut VirtualDevice,
    settings: &'a Settings,
}
impl OutputEmitter for LinuxEmitter<'_> {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        let Some(code) = linux_keycode(self.settings.binding(key)) else {
            return false;
        };
        self.output
            .emit(&[InputEvent::new(
                EventType::KEY.0,
                code,
                if action == KeyAction::Down { 1 } else { 0 },
            )])
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_linux_key_events_map_to_logical_actions() {
        let settings = Settings::default();
        assert_eq!(
            logical_event(&settings, KeyCode::new(0x11), 2),
            Some((LogicalKey::VerticalFirst, KeyAction::Down))
        );
        assert_eq!(
            logical_event(&settings, KeyCode::new(0x11), 0),
            Some((LogicalKey::VerticalFirst, KeyAction::Up))
        );
    }

    #[test]
    fn arrow_bindings_translate_between_scan_codes_and_evdev() {
        assert_eq!(linux_keycode(PhysicalKey::new(0x48, true)), Some(103));
        assert_eq!(physical_from_linux(103), PhysicalKey::new(0x48, true));
        assert_eq!(linux_keycode(PhysicalKey::new(0x1E, false)), Some(0x1E));
        assert_eq!(linux_keycode(PhysicalKey::new(0x1E, true)), None);
    }
}
