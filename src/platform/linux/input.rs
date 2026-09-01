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
    core::{KeyAction, LogicalKey, OutputEmitter, PhysicalKey, TimingController},
    settings::Settings,
};

const POLL_INTERVAL: Duration = Duration::from_millis(5);

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
        match commands.recv_timeout(POLL_INTERVAL) {
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
                let _ = timing.process(key, action, event.received_at, &mut emitter);
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
        .and_then(|builder| builder.name(&"LastKey virtual keyboard").with_keys(&keys))
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

fn is_keyboard_candidate(device: &Device, settings: &Settings) -> bool {
    device.supported_keys().is_some_and(|keys| {
        LogicalKey::ALL
            .into_iter()
            .all(|key| keys.contains(KeyCode::new(settings.binding(key).scan_code)))
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
        .logical_key_for(PhysicalKey::new(code.0, false))
        .map(|key| (key, action))
}
struct LinuxEmitter<'a> {
    output: &'a mut VirtualDevice,
    settings: &'a Settings,
}
impl OutputEmitter for LinuxEmitter<'_> {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        self.output
            .emit(&[InputEvent::new(
                EventType::KEY.0,
                self.settings.binding(key).scan_code,
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
}
