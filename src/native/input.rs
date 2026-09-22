use std::{io, mem::size_of, ptr};

use windows_sys::Win32::{
    System::{
        RemoteDesktop::{
            WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTSActive, WTSConnectState,
            WTSFreeMemory, WTSQuerySessionInformationW,
        },
        StationsAndDesktops::{
            CloseDesktop, DESKTOP_READOBJECTS, GetUserObjectInformationW, OpenInputDesktop,
            UOI_NAME,
        },
        SystemInformation::GetTickCount64,
    },
    UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetLastInputInfo, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO,
        MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
    },
};

use crate::scheduler::idle_elapsed;

pub fn uptime() -> u64 {
    // SAFETY: GetTickCount64 has no preconditions and owns no resources.
    unsafe { GetTickCount64() }
}

pub fn idle_ms() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: info is initialized and writable with the required size field.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    idle_elapsed(uptime() as u32, info.dwTime)
}

pub fn interactive_desktop() -> bool {
    // SAFETY: Output parameters are valid. WTS allocates the result; it is freed below.
    unsafe {
        let mut buffer = ptr::null_mut();
        let mut bytes = 0;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            WTS_CURRENT_SESSION,
            WTSConnectState,
            &mut buffer,
            &mut bytes,
        ) == 0
        {
            return false;
        }
        let active = !buffer.is_null()
            && bytes as usize >= size_of::<i32>()
            && ptr::read_unaligned(buffer.cast::<i32>()) == WTSActive;
        WTSFreeMemory(buffer.cast());
        if !active {
            return false;
        }

        // Never switch desktops or elevate; locked/UAC desktops must fail closed.
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return false;
        }
        let mut name = [0u16; 64];
        let mut needed = 0;
        let success = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            size_of_val(&name) as u32,
            &mut needed,
        ) != 0;
        CloseDesktop(desktop);
        success && name[..8] == [68, 101, 102, 97, 117, 108, 116, 0] // "Default"
    }
}

pub fn any_key_or_button_down() -> bool {
    // Check only immediately before a potential pulse, not through hooks or polling.
    (1..=254).any(|key| {
        // SAFETY: These are valid virtual-key codes; only the current down bit is used.
        unsafe { GetAsyncKeyState(key) < 0 }
    })
}

fn pulse_input() -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0x4D4D_4F56, // "MMOV": openly tagged synthetic input.
            },
        },
    }
}

/// Relative zero movement: no cursor warping, restoration race, buttons, or keys.
pub fn send_pulse() -> io::Result<()> {
    let input = pulse_input();
    // SAFETY: One initialized INPUT, correct ABI size, valid pointer for the call.
    if unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) } == 1 {
        Ok(())
    } else {
        // UIPI does not reliably set GetLastError. Do not invent an OS error code.
        Err(io::Error::other("Windows blocked the keep-alive input"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_cannot_move_click_scroll_or_type() {
        let input = pulse_input();
        assert_eq!(input.r#type, INPUT_MOUSE);
        // SAFETY: pulse_input sets the mi union member and the matching INPUT_MOUSE tag.
        let mouse = unsafe { input.Anonymous.mi };
        assert_eq!((mouse.dx, mouse.dy, mouse.mouseData), (0, 0, 0));
        assert_eq!(mouse.dwFlags, MOUSEEVENTF_MOVE);
        assert_eq!(mouse.time, 0);
        assert_eq!(mouse.dwExtraInfo, 0x4D4D_4F56);
    }

    #[test]
    #[ignore = "Interactive desktop only; waits for 30 seconds idle; sends ONE zero-motion event"]
    fn live_zero_motion_resets_idle_without_moving_cursor() {
        use windows_sys::Win32::{
            Foundation::POINT,
            UI::WindowsAndMessaging::{
                DispatchMessageW, GetCursorPos, MSG, MsgWaitForMultipleObjectsEx, PM_REMOVE,
                PeekMessageW, QS_ALLINPUT, TranslateMessage,
            },
        };
        assert!(
            interactive_desktop(),
            "Requires an unlocked interactive desktop"
        );
        println!(
            "Waiting up to two minutes for 30 seconds without input; please release keys/buttons and leave the desktop idle."
        );
        let deadline = uptime() + 120_000;
        let before_idle = loop {
            let idle = idle_ms().expect("Read idle time");
            if idle >= crate::scheduler::IDLE_GRACE_MS && !any_key_or_button_down() {
                break idle;
            }
            let remaining = deadline.saturating_sub(uptime());
            assert!(remaining > 0, "Desktop remained active; no input was sent");
            let delay = crate::scheduler::IDLE_GRACE_MS
                .saturating_sub(idle)
                .max(1000);
            // SAFETY: This test thread waits on its own message queue, without hooks
            // or injection. Drain messages so they cannot turn the wait into a spin.
            unsafe {
                MsgWaitForMultipleObjectsEx(
                    0,
                    ptr::null(),
                    delay.min(remaining as u32),
                    QS_ALLINPUT,
                    0,
                );
                let mut message: MSG = std::mem::zeroed();
                while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        };
        assert!(
            interactive_desktop(),
            "Desktop changed while waiting; no input was sent"
        );
        let mut before = POINT { x: 0, y: 0 };
        let mut after = POINT { x: 0, y: 0 };
        // SAFETY: Valid POINT outputs. Bounded native message wait allows asynchronous
        // input delivery; this test neither moves the pointer nor fabricates key presses.
        unsafe {
            assert_ne!(GetCursorPos(&mut before), 0);
            assert!(idle_ms().is_some_and(|idle| idle >= crate::scheduler::IDLE_GRACE_MS));
            assert!(!any_key_or_button_down());
            send_pulse().expect("Send one zero-motion event");
            MsgWaitForMultipleObjectsEx(0, ptr::null(), 100, QS_ALLINPUT, 0);
            assert_ne!(GetCursorPos(&mut after), 0);
        }
        let after_idle = idle_ms().expect("Re-read Windows idle time");
        assert_eq!((before.x, before.y), (after.x, after.y));
        assert!(
            after_idle < 1_000,
            "Windows did not acknowledge the idle reset: {after_idle}ms"
        );
        println!(
            "Windows idle: {before_idle}ms -> {after_idle}ms; cursor unchanged at ({}, {})",
            after.x, after.y
        );
    }
}
