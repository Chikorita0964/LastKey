use std::{cell::RefCell, mem::size_of};
use std::{
    fmt,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
};

use windows::Win32::{
    Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{
            GetKeyNameTextW, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
            KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
            PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
            WM_SYSKEYUP,
        },
    },
};

use crate::{
    core::{EventDisposition, InputRouter, KeyAction, LogicalKey, OutputEmitter, PhysicalKey},
    settings::Settings,
};

const INJECTION_TAG: usize = 0x4C41_5354_4B45_5931; // "LASTKEY1"

const COMMAND_MESSAGE: u32 = WM_APP + 1;

thread_local! {
    static ENGINE: RefCell<Option<InputEngine>> = const { RefCell::new(None) };
}

#[derive(Clone, Debug)]
pub struct CapturedKey {
    pub physical: PhysicalKey,
    pub name: String,
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
    Capture(Sender<CapturedKey>),
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
        self.send(InputCommand::Capture(sender))?;
        Ok(receiver)
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
    router: InputRouter,
    settings: Settings,
    capture_sender: Option<Sender<CapturedKey>>,
    suppress_capture_key: Option<PhysicalKey>,
}

impl InputEngine {
    fn new(settings: Settings) -> Self {
        Self {
            router: InputRouter::new(),
            settings,
            capture_sender: None,
            suppress_capture_key: None,
        }
    }

    fn process(&mut self, physical: PhysicalKey, action: KeyAction) -> EventDisposition {
        if self.suppress_capture_key == Some(physical) {
            if action == KeyAction::Up {
                self.suppress_capture_key = None;
            }
            return EventDisposition::Consume;
        }

        if action == KeyAction::Down && self.capture_sender.is_some() {
            let sender = self
                .capture_sender
                .take()
                .expect("capture sender is present");
            let _ = sender.send(CapturedKey {
                physical,
                name: key_name(physical),
            });
            self.suppress_capture_key = Some(physical);
            return EventDisposition::Consume;
        }

        let Some(key) = self.settings.logical_key_for(physical) else {
            return EventDisposition::PassThrough;
        };
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.router.process(key, action, &mut emitter)
    }

    fn apply(&mut self, settings: Settings) {
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.router.reset(&mut emitter);
        self.settings = settings;
        self.capture_sender = None;
        self.suppress_capture_key = None;
    }

    fn release_all(&mut self) {
        let mut emitter = WindowsEmitter {
            settings: &self.settings,
        };
        self.router.release_all(&mut emitter);
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

        unsafe { SendInput(&[input], size_of::<INPUT>() as i32) == 1 }
    }
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
    ENGINE.with(|engine| *engine.borrow_mut() = Some(InputEngine::new(settings)));

    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(error) => {
            let _ = started.send(Err(InputServiceError::Hook(error.to_string())));
            return;
        }
    };
    let hook = match unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            Some(HINSTANCE(module.0)),
            0,
        )
    } {
        Ok(hook) => hook,
        Err(error) => {
            let _ = started.send(Err(InputServiceError::Hook(error.to_string())));
            return;
        }
    };
    let _ = started.send(Ok(unsafe { GetCurrentThreadId() }));

    let mut message = MSG::default();
    let mut keep_running = true;
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result <= 0 {
            break;
        }
        if message.message == COMMAND_MESSAGE {
            while let Ok(command) = commands.try_recv() {
                match command {
                    InputCommand::Apply(settings) => ENGINE.with(|engine| {
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .apply(settings);
                    }),
                    InputCommand::Capture(sender) => ENGINE.with(|engine| {
                        engine
                            .borrow_mut()
                            .as_mut()
                            .expect("input engine is initialized")
                            .capture_sender = Some(sender);
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
    }

    ENGINE.with(|engine| {
        if let Some(engine) = engine.borrow_mut().as_mut() {
            engine.release_all();
        }
    });
    let _ = unsafe { UnhookWindowsHookEx(hook) };
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
    if event.flags.0 & LLKHF_INJECTED.0 != 0 && event.dwExtraInfo == INJECTION_TAG {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    let physical = PhysicalKey::new(event.scanCode as u16, event.flags.0 & 0x01 != 0);
    let Some(action) = action_for(message) else {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    };

    let disposition = ENGINE.with(|engine| {
        engine
            .borrow_mut()
            .as_mut()
            .expect("input engine is initialized")
            .process(physical, action)
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

fn action_for(message: WPARAM) -> Option<KeyAction> {
    match message.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyAction::Down),
        WM_KEYUP | WM_SYSKEYUP => Some(KeyAction::Up),
        _ => None,
    }
}
