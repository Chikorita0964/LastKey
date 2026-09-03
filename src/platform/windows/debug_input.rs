use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use windows::Win32::{
    Foundation::POINT,
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::GetCursorPos},
};

use crate::debug_log;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(5);
const FIRST_VIRTUAL_KEY: u8 = 1;

pub struct DebugInputSampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DebugInputSampler {
    pub fn start() -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("lastkey-debug-input".into())
            .spawn(move || run(thread_stop))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DebugInputSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(stop: Arc<AtomicBool>) {
    let mut states = [false; 256];
    for virtual_key in FIRST_VIRTUAL_KEY..=u8::MAX {
        states[usize::from(virtual_key)] = is_down(virtual_key);
    }
    debug_log::write(format_args!(
        "independent input sampler started interval_ms={}",
        SAMPLE_INTERVAL.as_millis()
    ));

    while !stop.load(Ordering::Acquire) {
        thread::sleep(SAMPLE_INTERVAL);
        for virtual_key in FIRST_VIRTUAL_KEY..=u8::MAX {
            let down = is_down(virtual_key);
            let index = usize::from(virtual_key);
            if states[index] == down {
                continue;
            }
            states[index] = down;
            log_transition(virtual_key, down);
        }
    }

    debug_log::write(format_args!("independent input sampler stopped"));
}

fn is_down(virtual_key: u8) -> bool {
    unsafe { GetAsyncKeyState(i32::from(virtual_key)) < 0 }
}

fn log_transition(virtual_key: u8, down: bool) {
    let state = if down { "down" } else { "up" };
    let name = virtual_key_name(virtual_key);
    if is_mouse_button(virtual_key) {
        let mut point = POINT::default();
        let position = unsafe { GetCursorPos(&mut point) }
            .map(|()| format!("{},{}", point.x, point.y))
            .unwrap_or_else(|_| "unavailable".into());
        debug_log::write(format_args!(
            "sampler transition device=mouse key={name} vk=0x{virtual_key:02X} state={state} cursor={position}"
        ));
    } else {
        debug_log::write(format_args!(
            "sampler transition device=keyboard key={name} vk=0x{virtual_key:02X} state={state}"
        ));
    }
}

fn is_mouse_button(virtual_key: u8) -> bool {
    matches!(virtual_key, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
}

fn virtual_key_name(virtual_key: u8) -> String {
    match virtual_key {
        0x01 => "MouseLeft".into(),
        0x02 => "MouseRight".into(),
        0x04 => "MouseMiddle".into(),
        0x05 => "MouseX1".into(),
        0x06 => "MouseX2".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x10 => "Shift".into(),
        0x11 => "Control".into(),
        0x12 => "Alt".into(),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x30..=0x39 | 0x41..=0x5A => char::from(virtual_key).to_string(),
        _ => format!("VK_{virtual_key:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::virtual_key_name;

    #[test]
    fn names_common_debug_keys() {
        assert_eq!(virtual_key_name(0x01), "MouseLeft");
        assert_eq!(virtual_key_name(0x41), "A");
        assert_eq!(virtual_key_name(0x44), "D");
        assert_eq!(virtual_key_name(0x70), "VK_70");
    }
}
