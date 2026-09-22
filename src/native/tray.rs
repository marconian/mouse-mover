use std::{io, mem::size_of, ptr};

use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject,
    },
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
    },
};

use super::TRAY_MESSAGE;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enabled,
    Paused,
    SessionInactive,
    Blocked,
}

impl Status {
    fn tooltip(self) -> &'static str {
        match self {
            Self::Enabled => "Mouse Mover - enabled (idle only)",
            Self::Paused => "Mouse Mover - paused",
            Self::SessionInactive => "Mouse Mover - waiting for unlocked session",
            Self::Blocked => "Mouse Mover - input unavailable; will retry",
        }
    }
}

struct Icon(HICON);

impl Icon {
    fn new(color: u32, paused: bool) -> io::Result<Self> {
        const SIZE: usize = 32;
        let mut pixels = [0u32; SIZE * SIZE];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as i32 * 2 - 31;
                let dy = y as i32 * 2 - 31;
                if dx * dx + dy * dy <= 29 * 29 {
                    pixels[y * SIZE + x] = color;
                }
                let glyph = if paused {
                    (10..=13).contains(&x) || (18..=21).contains(&x)
                } else {
                    // A simple original mouse silhouette; no third-party artwork.
                    let mx = x as i32 - 16;
                    let my = y as i32 - 16;
                    mx * mx * 2 + my * my < 75
                };
                if glyph && (9..=23).contains(&y) {
                    pixels[y * SIZE + x] = 0xFFF8_FAFC;
                }
                if !paused && (15..=16).contains(&x) && (11..=15).contains(&y) {
                    pixels[y * SIZE + x] = color;
                }
            }
        }

        // SAFETY: Initialized bitmap descriptors and correctly sized pixel/mask buffers.
        // CreateIconIndirect copies both bitmaps; they are deleted on all paths.
        unsafe {
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: SIZE as i32,
                    biHeight: -(SIZE as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..std::mem::zeroed()
                },
                ..std::mem::zeroed()
            };
            let mut bits = ptr::null_mut();
            let bitmap = CreateDIBSection(
                ptr::null_mut(),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                return Err(io::Error::last_os_error());
            }
            ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
            let mask_bits = [0u8; SIZE * SIZE / 8];
            let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, mask_bits.as_ptr().cast());
            if mask.is_null() {
                let error = io::Error::last_os_error();
                DeleteObject(bitmap);
                return Err(error);
            }
            let icon = CreateIconIndirect(&ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: bitmap,
            });
            let error = io::Error::last_os_error();
            DeleteObject(mask);
            DeleteObject(bitmap);
            if icon.is_null() {
                Err(error)
            } else {
                Ok(Self(icon))
            }
        }
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: This is a privately owned icon, not a shared system icon.
        unsafe { DestroyIcon(self.0) };
    }
}

pub struct Tray {
    hwnd: HWND,
    enabled: Icon,
    paused: Icon,
    blocked: Icon,
    pub installed: bool,
    status: Status,
}

impl Tray {
    pub fn new(hwnd: HWND) -> io::Result<Self> {
        Ok(Self {
            hwnd,
            enabled: Icon::new(0xFF0D_9488, false)?,
            paused: Icon::new(0xFFD9_7706, true)?,
            blocked: Icon::new(0xFFDC_2626, true)?,
            installed: false,
            status: Status::Enabled,
        })
    }

    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: Zero is a valid initial value for unused NOTIFYICONDATAW fields.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = match self.status {
            Status::Enabled => self.enabled.0,
            Status::Paused | Status::SessionInactive => self.paused.0,
            Status::Blocked => self.blocked.0,
        };
        for (target, character) in data
            .szTip
            .iter_mut()
            .zip(self.status.tooltip().encode_utf16())
        {
            *target = character;
        }
        data
    }

    pub fn add(&mut self) -> bool {
        let mut data = self.data();
        // SAFETY: The hidden window and icon handles remain alive for the tray lifetime.
        unsafe {
            self.installed = Shell_NotifyIconW(NIM_ADD, &data) != 0;
            if self.installed {
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                if Shell_NotifyIconW(NIM_SETVERSION, &data) == 0 {
                    Shell_NotifyIconW(NIM_DELETE, &data);
                    self.installed = false;
                }
            }
        }
        self.installed
    }

    pub fn status(&mut self, status: Status) {
        if self.status == status {
            return;
        }
        self.status = status;
        if self.installed {
            // SAFETY: The notification structure contains live window and icon handles.
            self.installed = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data()) } != 0;
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: The owner window outlives Tray. Removing an absent icon is harmless.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data()) };
    }
}
