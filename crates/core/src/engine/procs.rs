//! Cheap check for running processes (a single Toolhelp snapshot).

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

/// G HUB's background agent. Real TRUEFORCE games stream their haptics through it; the
/// always-running `lghub_updater.exe` doesn't count.
pub const GHUB_AGENT: &str = "lghub_agent.exe";

/// True if a process with this executable name (case-insensitive) is running.
pub fn is_running(exe: &str) -> bool {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_own_process_and_not_a_made_up_one() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().to_uppercase();
        assert!(super::is_running(&name));
        assert!(!super::is_running("definitely-not-running-gclone-test.exe"));
    }
}
