mod input;
mod startup;
mod tray;

use std::{cell::RefCell, collections::VecDeque, io, mem::size_of, ptr, sync::OnceLock};

use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::{
                NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
                WTSUnRegisterSessionNotification,
            },
            Threading::CreateMutexW,
        },
        UI::{
            Shell::{NIN_SELECT, NINF_KEY},
            WindowsAndMessaging::*,
        },
    },
    core::w,
};

use crate::scheduler::{Decision, IDLE_GRACE_MS, RETRY_MS, Scheduler};
use tray::{Status, Tray};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const TIMER_ID: usize = 1;
const MENU_TOGGLE: u32 = 1;
const MENU_STARTUP: u32 = 2;
const MENU_EXIT: u32 = 3;
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

#[derive(Clone, Copy)]
enum Event {
    Tick,
    Menu(i32, i32),
    Session(u32),
    Power(u32),
    ExplorerRestarted,
    Quit,
}

thread_local! {
    // Native modal menus may re-enter the window procedure. Only enqueue there:
    // never alias a mutable App reference across a reentrant Win32 call.
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
}

fn enqueue(event: Event) {
    EVENTS.with(|queue| queue.borrow_mut().push_back(event));
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if TASKBAR_CREATED.get().copied() == Some(message) {
        enqueue(Event::ExplorerRestarted);
        // SAFETY: Wake the outer loop even if Explorer used a synchronous broadcast
        // while paused (no timer exists to wake GetMessage in that state).
        unsafe { PostMessageW(hwnd, WM_NULL, 0, 0) };
        return 0;
    }
    match message {
        WM_TIMER if wparam == TIMER_ID => enqueue(Event::Tick),
        TRAY_MESSAGE => {
            let event = (lparam as u32) & 0xFFFF;
            if matches!(event, WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT) {
                enqueue(Event::Menu(
                    wparam as i16 as i32,
                    (wparam >> 16) as i16 as i32,
                ));
            }
        }
        WM_WTSSESSION_CHANGE => enqueue(Event::Session(wparam as u32)),
        WM_POWERBROADCAST => {
            enqueue(Event::Power(wparam as u32));
            return 1;
        }
        WM_CLOSE => enqueue(Event::Quit),
        WM_QUERYENDSESSION => return 1,
        WM_ENDSESSION if wparam != 0 => enqueue(Event::Quit),
        _ => {
            // SAFETY: Unhandled messages preserve the original window procedure ABI.
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
    }
    // Wake GetMessage if this event arrived via SendMessage, not the posted queue.
    // WM_NULL is handled by DefWindowProc, so this cannot feed itself indefinitely.
    // SAFETY: hwnd is the current native callback's live window.
    unsafe { PostMessageW(hwnd, WM_NULL, 0, 0) };
    0
}

struct Instance(HANDLE);

impl Instance {
    fn acquire() -> io::Result<Option<Self>> {
        // SAFETY: Null security attributes and a constant terminated, session-local name.
        unsafe {
            let handle = CreateMutexW(
                ptr::null(),
                0,
                w!("Local\\MouseMover.1B665A73-9B7D-41DA-9291-409BE4A88B48"),
            );
            let error = GetLastError();
            if handle.is_null() {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            let instance = Self(handle);
            if error == ERROR_ALREADY_EXISTS {
                Ok(None)
            } else {
                Ok(Some(instance))
            }
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: Instance owns the mutex handle; it did not acquire mutex ownership.
        unsafe { CloseHandle(self.0) };
    }
}

struct Window(HWND);

impl Window {
    fn new() -> io::Result<Self> {
        // SAFETY: A valid module, static class strings, and a callback with the Win32 ABI.
        // The window has no WS_VISIBLE flag, so it never appears or takes focus.
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            if instance.is_null() {
                return Err(io::Error::last_os_error());
            }
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: w!("MouseMover.TrayWindow"),
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(io::Error::last_os_error());
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.lpszClassName,
                w!("Mouse Mover"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if hwnd.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(hwnd))
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: This thread owns this window; Tray and App have already been dropped.
        unsafe { DestroyWindow(self.0) };
    }
}

struct Menu(HMENU);

impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: Menu owns a popup menu that is no longer being tracked.
        unsafe { DestroyMenu(self.0) };
    }
}

struct App {
    hwnd: HWND,
    tray: Tray,
    scheduler: Scheduler,
    session_registered: bool,
}

impl App {
    fn new(hwnd: HWND) -> io::Result<Self> {
        let mut app = Self {
            hwnd,
            tray: Tray::new(hwnd)?,
            scheduler: Scheduler::new(input::uptime()),
            session_registered: false,
        };
        app.tick()?;
        Ok(app)
    }

    fn arm(&self, delay_ms: u32) -> io::Result<()> {
        // SAFETY: This window belongs to this thread. Reset the same timer, with normal
        // scheduler coalescing rather than changing the global timer resolution.
        unsafe {
            KillTimer(self.hwnd, TIMER_ID);
            if SetCoalescableTimer(self.hwnd, TIMER_ID, delay_ms.max(100), None, 1000) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn stop_timer(&self) {
        // SAFETY: Removing this window's timer is safe even when none is armed.
        unsafe { KillTimer(self.hwnd, TIMER_ID) };
    }

    fn tick(&mut self) -> io::Result<()> {
        self.stop_timer();
        if !self.tray.installed && !self.tray.add() {
            // Explorer might not yet be ready at login. Never act without a tray control.
            return self.arm(5000);
        }
        if !self.session_registered {
            // SAFETY: Live hidden top-level window, notifications for this session only.
            self.session_registered =
                unsafe { WTSRegisterSessionNotification(self.hwnd, NOTIFY_FOR_THIS_SESSION) != 0 };
            if !self.session_registered {
                self.tray.status(Status::Blocked);
                return self.arm(RETRY_MS);
            }
        }
        if !self.scheduler.running() {
            self.tray.status(if self.scheduler.enabled {
                Status::SessionInactive
            } else {
                Status::Paused
            });
            return if self.tray.installed {
                Ok(())
            } else {
                // A failed icon update must not strand a paused app without controls.
                self.arm(5000)
            };
        }
        let idle = input::idle_ms();
        match self.scheduler.decide(input::uptime(), idle) {
            Decision::Stop => Ok(()),
            Decision::Wait(delay) => {
                self.tray.status(if idle.is_some() {
                    Status::Enabled
                } else {
                    Status::Blocked
                });
                self.arm(delay)
            }
            Decision::Pulse => {
                if !input::interactive_desktop() || input::any_key_or_button_down() {
                    return self.arm(RETRY_MS);
                }
                // Re-read activity after the desktop/key checks, as close to SendInput
                // as possible. Win32 offers no atomic "only inject if still idle" API.
                if input::idle_ms().is_none_or(|idle| idle < IDLE_GRACE_MS) {
                    return self.arm(IDLE_GRACE_MS);
                }
                let sent = input::send_pulse().is_ok();
                self.scheduler.attempted(input::uptime());
                self.tray.status(if sent {
                    Status::Enabled
                } else {
                    Status::Blocked
                });
                self.arm(crate::scheduler::PULSE_INTERVAL_MS)
            }
        }
    }

    fn menu(&mut self, x: i32, y: i32) -> io::Result<bool> {
        let startup = startup::enabled();
        // SAFETY: The menu is owned until tracking ends; strings are static UTF-16.
        // Reentrant callbacks only enqueue events and never access this App reference.
        let selected = unsafe {
            let menu = Menu(CreatePopupMenu());
            if menu.0.is_null() {
                return Err(io::Error::last_os_error());
            }
            let title = if self.scheduler.enabled {
                w!("&Pause")
            } else {
                w!("&Resume")
            };
            let startup_flags = match startup {
                Ok(true) => MF_STRING | MF_CHECKED,
                Ok(false) => MF_STRING,
                Err(_) => MF_STRING | MF_GRAYED,
            };
            if AppendMenuW(menu.0, MF_STRING, MENU_TOGGLE as usize, title) == 0
                || AppendMenuW(
                    menu.0,
                    startup_flags,
                    MENU_STARTUP as usize,
                    w!("Start with &Windows"),
                ) == 0
                || AppendMenuW(menu.0, MF_SEPARATOR, 0, ptr::null()) == 0
                || AppendMenuW(menu.0, MF_STRING, MENU_EXIT as usize, w!("E&xit")) == 0
            {
                return Err(io::Error::last_os_error());
            }
            SetForegroundWindow(self.hwnd);
            let choice = TrackPopupMenu(
                menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                x,
                y,
                0,
                self.hwnd,
                ptr::null(),
            );
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            choice as u32
        };
        match selected {
            MENU_TOGGLE => {
                self.scheduler.toggle(input::uptime());
                self.tick()?;
            }
            MENU_STARTUP => {
                if let Ok(enabled) = startup {
                    if let Err(error) = startup::set_enabled(!enabled) {
                        show_error(&format!("Could not change startup registration: {error}"));
                    }
                }
            }
            MENU_EXIT => return Ok(false),
            _ => {}
        }
        Ok(true)
    }

    fn handle(&mut self, event: Event) -> io::Result<bool> {
        match event {
            Event::Tick => self.tick()?,
            Event::Menu(x, y) => return self.menu(x, y),
            Event::Quit => return Ok(false),
            Event::ExplorerRestarted => {
                self.tray.installed = false;
                self.tick()?;
            }
            Event::Session(reason) => {
                match reason {
                    WTS_SESSION_LOCK
                    | WTS_SESSION_LOGOFF
                    | WTS_CONSOLE_DISCONNECT
                    | WTS_REMOTE_DISCONNECT => {
                        self.scheduler.session_changed(false, input::uptime());
                    }
                    WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                        self.scheduler.session_changed(true, input::uptime());
                    }
                    _ => return Ok(true),
                }
                self.tick()?;
            }
            Event::Power(reason) => {
                match reason {
                    PBT_APMSUSPEND => self.scheduler.power_changed(true, input::uptime()),
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                        self.scheduler.power_changed(false, input::uptime());
                    }
                    _ => return Ok(true),
                }
                self.tick()?;
            }
        }
        Ok(true)
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop_timer();
        if self.session_registered {
            // SAFETY: The window remains alive until after App/Tray destruction.
            unsafe { WTSUnRegisterSessionNotification(self.hwnd) };
        }
    }
}

pub fn run() -> io::Result<()> {
    let Some(_instance) = Instance::acquire()? else {
        return Ok(());
    };
    // SAFETY: Constant terminated registered-message name.
    let taskbar_message = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    if taskbar_message == 0 {
        return Err(io::Error::last_os_error());
    }
    let _ = TASKBAR_CREATED.set(taskbar_message);
    let window = Window::new()?;
    let mut app = App::new(window.0)?;
    // SAFETY: Zero-initialized MSG is a writable output for GetMessageW.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: This thread owns the window/message loop. GetMessage blocks when idle.
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        if result == 0 {
            return Ok(());
        }
        // SAFETY: Dispatch a successfully retrieved native message to our callback.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        while let Some(event) = EVENTS.with(|queue| queue.borrow_mut().pop_front()) {
            if !app.handle(event)? {
                return Ok(());
            }
        }
    }
}

pub fn show_error(text: &str) {
    let message: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: Terminated strings valid throughout the modal call; no App is accessed.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            w!("Mouse Mover"),
            MB_OK | MB_ICONERROR,
        )
    };
}
