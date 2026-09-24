#![cfg(windows)]

use std::collections::HashSet;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
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

/// Liveness of a fixed PID set through handles opened once.
///
/// The worker checks liveness every loop iteration. Re-opening each process
/// every time cost an `OpenProcess`/`CloseHandle` pair per PID per iteration,
/// and a held handle also pins the process object, so a recycled PID can never
/// be mistaken for the original KakaoTalk.
#[derive(Default)]
pub struct PidWatch {
    pids: Vec<i64>,
    /// `None` when the process could not be opened; such a PID is never
    /// reported alive, which keeps the old "rescan soon" behaviour.
    handles: Vec<Option<HANDLE>>,
}

impl PidWatch {
    /// Watch exactly `pids`, reusing nothing from the previous set.
    pub fn watch(&mut self, pids: &[i64]) {
        let mut sorted = pids.to_vec();
        sorted.sort_unstable();
        if sorted == self.pids {
            return;
        }
        self.close_all();
        self.handles = sorted.iter().map(|&pid| open_for_query(pid)).collect();
        self.pids = sorted;
    }

    /// True when at least one PID is watched and every one is still running.
    pub fn all_alive(&self) -> bool {
        !self.handles.is_empty()
            && self
                .handles
                .iter()
                .all(|handle| handle.is_some_and(handle_is_running))
    }

    fn close_all(&mut self) {
        for handle in self.handles.drain(..).flatten() {
            let _ = unsafe { CloseHandle(handle) };
        }
        self.pids.clear();
    }
}

impl Drop for PidWatch {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn open_for_query(pid: i64) -> Option<HANDLE> {
    if pid <= 0 || pid > u32::MAX as i64 {
        return None;
    }
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid as u32) }.ok()
}

fn handle_is_running(handle: HANDLE) -> bool {
    let mut exit_code = 0u32;
    unsafe { GetExitCodeProcess(handle, &mut exit_code) }.is_ok() && exit_code == STILL_ACTIVE
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
    fn pid_watch_tracks_the_current_process_and_rejects_bad_pids() {
        let mut watch = PidWatch::default();
        assert!(!watch.all_alive(), "an empty watch is never alive");
        watch.watch(&[i64::from(std::process::id())]);
        assert!(watch.all_alive());
        watch.watch(&[i64::from(std::process::id()), -1]);
        assert!(
            !watch.all_alive(),
            "an unopenable PID must not count as alive"
        );
        watch.watch(&[]);
        assert!(!watch.all_alive());
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
