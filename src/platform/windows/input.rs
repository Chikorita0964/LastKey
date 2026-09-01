use std::{cell::RefCell, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
                KEYEVENTF_SCANCODE, SendInput,
            },
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
                LLKHF_INJECTED, MSG, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
                WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
            },
        },
    },
    core::Result,
};

use crate::core::{
    EventDisposition, InputRouter, KeyAction, LogicalKey, OutputEmitter, PhysicalKey,
};

const INJECTION_TAG: usize = 0x4C41_5354_4B45_5931; // "LASTKEY1"

const DEFAULT_KEYS: [(LogicalKey, PhysicalKey); 4] = [
    (
        LogicalKey::VerticalFirst,
        PhysicalKey {
            scan_code: 0x11,
            extended: false,
        },
    ), // W
    (
        LogicalKey::VerticalSecond,
        PhysicalKey {
            scan_code: 0x1F,
            extended: false,
        },
    ), // S
    (
        LogicalKey::HorizontalFirst,
        PhysicalKey {
            scan_code: 0x1E,
            extended: false,
        },
    ), // A
    (
        LogicalKey::HorizontalSecond,
        PhysicalKey {
            scan_code: 0x20,
            extended: false,
        },
    ), // D
];

thread_local! {
    static ROUTER: RefCell<InputRouter> = const { RefCell::new(InputRouter::new()) };
}

struct WindowsEmitter;

impl OutputEmitter for WindowsEmitter {
    fn emit(&mut self, key: LogicalKey, action: KeyAction) -> bool {
        let physical = physical_for(key);
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

pub fn run() -> Result<()> {
    let module = unsafe { GetModuleHandleW(None)? };
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            Some(HINSTANCE(module.0)),
            0,
        )?
    };

    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    ROUTER.with(|router| router.borrow_mut().release_all(&mut WindowsEmitter));
    unsafe { UnhookWindowsHookEx(hook)? };
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
    if event.flags.0 & LLKHF_INJECTED.0 != 0 && event.dwExtraInfo == INJECTION_TAG {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    }

    let Some(key) = logical_key_for(PhysicalKey {
        scan_code: event.scanCode as u16,
        extended: event.flags.0 & LLKHF_EXTENDED.0 != 0,
    }) else {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    };
    let Some(action) = action_for(message) else {
        return unsafe { CallNextHookEx(None, code, message, l_param) };
    };

    let disposition = ROUTER.with(|router| {
        router
            .borrow_mut()
            .process(key, action, &mut WindowsEmitter)
    });
    if disposition == EventDisposition::PassThrough {
        unsafe { CallNextHookEx(None, code, message, l_param) }
    } else {
        LRESULT(1)
    }
}

fn logical_key_for(physical: PhysicalKey) -> Option<LogicalKey> {
    DEFAULT_KEYS
        .iter()
        .find_map(|(logical, candidate)| (*candidate == physical).then_some(*logical))
}

fn physical_for(key: LogicalKey) -> PhysicalKey {
    DEFAULT_KEYS
        .iter()
        .find_map(|(logical, physical)| (*logical == key).then_some(*physical))
        .expect("every logical key has a default Windows binding")
}

fn action_for(message: WPARAM) -> Option<KeyAction> {
    match message.0 as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(KeyAction::Down),
        WM_KEYUP | WM_SYSKEYUP => Some(KeyAction::Up),
        _ => None,
    }
}
