use std::{ffi::OsString, io, os::windows::ffi::OsStrExt, ptr};

use windows_sys::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
            RegOpenKeyExW, RegSetValueExW,
        },
    },
    core::w,
};

const RUN_KEY: *const u16 = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: *const u16 = w!("MouseMover");

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: Key owns a successfully opened registry handle.
        unsafe { RegCloseKey(self.0) };
    }
}

fn command() -> io::Result<Vec<u16>> {
    let path = std::env::current_exe()?;
    let mut quoted = OsString::from("\"");
    quoted.push(path);
    quoted.push("\"");
    Ok(quoted.encode_wide().chain(Some(0)).collect())
}

pub fn enabled() -> io::Result<bool> {
    // SAFETY: Constant terminated strings and writable output buffers of stated sizes.
    unsafe {
        let mut handle = ptr::null_mut();
        let result = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, KEY_QUERY_VALUE, &mut handle);
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        if result != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let key = Key(handle);
        let mut bytes = 0;
        let result = RegGetValueW(
            key.0,
            ptr::null(),
            VALUE_NAME,
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut bytes,
        );
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        if result != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let mut value = vec![0u16; (bytes as usize).div_ceil(2)];
        let result = RegGetValueW(
            key.0,
            ptr::null(),
            VALUE_NAME,
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        );
        if result != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        value.truncate((bytes as usize).div_ceil(2));
        Ok(value == command()?)
    }
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    // SAFETY: All pointers reference terminated strings or buffers valid for the call.
    // Only this application's named value in the current user's Run key is modified.
    unsafe {
        let mut handle = ptr::null_mut();
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut handle,
            ptr::null_mut(),
        );
        if result != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        let key = Key(handle);
        let result = if enabled {
            let value = command()?;
            RegSetValueExW(
                key.0,
                VALUE_NAME,
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key.0, VALUE_NAME)
        };
        if result != ERROR_SUCCESS && !(result == ERROR_FILE_NOT_FOUND && !enabled) {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
    }
    Ok(())
}
