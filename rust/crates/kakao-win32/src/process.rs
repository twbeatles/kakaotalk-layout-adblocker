#![cfg(windows)]

use std::collections::HashSet;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

const STILL_ACTIVE: u32 = 259;

pub fn is_process_alive(pid: i64) -> bool {
    if pid <= 0 || pid > u32::MAX as i64 {
        return false;
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) else {
            return false;
        };
        let mut exit_code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
        let _ = CloseHandle(handle);
        ok && exit_code == STILL_ACTIVE
    }
}

pub fn kakaotalk_pids() -> HashSet<i64> {
    process_ids("kakaotalk.exe")
}

pub fn process_ids(image_name: &str) -> HashSet<i64> {
    try_process_ids(image_name).unwrap_or_default()
}

/// Like `process_ids`, but distinguishes "no matching process" (`Some(empty)`)
/// from "could not enumerate processes" (`None`). `--self-check` needs that
/// difference: KakaoTalk simply not running is not a diagnostic failure.
pub fn try_process_ids(image_name: &str) -> Option<HashSet<i64>> {
    let mut pids = HashSet::new();
    let normalized = image_name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Some(pids);
    }
    let target_utf16: Vec<u16> = normalized.encode_utf16().collect();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snapshot) = snapshot else {
        return None;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while ok {
        if eq_wide_ascii_case(&entry.szExeFile, &target_utf16) {
            pids.insert(i64::from(entry.th32ProcessID));
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    Some(pids)
}

fn eq_wide_ascii_case(buf: &[u16], target: &[u16]) -> bool {
    let end = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
    let slice = &buf[..end];
    if slice.len() != target.len() {
        return false;
    }
    slice.iter().zip(target.iter()).all(|(&a, &b)| {
        let a_lower = if a <= 127 && (a as u8).is_ascii_uppercase() {
            a + 32
        } else {
            a
        };
        a_lower == b
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_is_alive() {
        let my_pid = i64::from(std::process::id());
        assert!(is_process_alive(my_pid));
    }

    #[test]
    fn invalid_pid_is_not_alive() {
        assert!(!is_process_alive(-1));
        assert!(!is_process_alive(0));
        assert!(!is_process_alive(i64::MAX));
    }

    #[test]
    fn eq_wide_matches_case_insensitively() {
        let buf: Vec<u16> = "KakaoTalk.exe\0extra".encode_utf16().collect();
        let target: Vec<u16> = "kakaotalk.exe".encode_utf16().collect();
        assert!(eq_wide_ascii_case(&buf, &target));

        let mismatch: Vec<u16> = "other.exe".encode_utf16().collect();
        assert!(!eq_wide_ascii_case(&buf, &mismatch));
    }
}
