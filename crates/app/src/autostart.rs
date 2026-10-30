//! "Start with Windows" via the per-user Run key (no admin rights needed).

use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "G-Clone";
/// Passed by the Run entry so boot-time launches go straight to the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {MINIMIZED_ARG}", exe.display()))
}

/// True when the Run entry exists and points at this exe.
pub fn is_enabled() -> bool {
    let (key, value) = (wide(RUN_KEY), wide(VALUE));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return false;
    }
    let stored = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]);
    Some(stored) == command()
}

pub fn set_enabled(enable: bool) -> Result<(), String> {
    let (key, value) = (wide(RUN_KEY), wide(VALUE));
    let rc = unsafe {
        if enable {
            let cmd = wide(&command().ok_or("cannot locate G-Clone executable")?);
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                cmd.as_ptr().cast(),
                (cmd.len() * 2) as u32,
            )
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr())
        }
    };
    if rc == ERROR_SUCCESS || (!enable && rc == 2 /* ERROR_FILE_NOT_FOUND */) {
        Ok(())
    } else {
        Err(format!("registry error {rc}"))
    }
}
