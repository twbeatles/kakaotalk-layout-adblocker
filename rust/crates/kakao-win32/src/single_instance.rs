#![cfg(windows)]

use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WIN32_ERROR};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

const MUTEX_NAME: windows::core::PCWSTR = w!("Local\\KakaoTalkLayoutAdBlocker_v11");
const ERROR_ALREADY_EXISTS: u32 = 183;

pub struct InstanceMutex {
    handle: HANDLE,
}

impl InstanceMutex {
    pub fn acquire() -> Result<Self, AlreadyRunning> {
        Self::acquire_named(MUTEX_NAME)
    }

    pub fn acquire_named(name: windows::core::PCWSTR) -> Result<Self, AlreadyRunning> {
        let handle = unsafe { CreateMutexW(None, true, name) }.map_err(|_| AlreadyRunning)?;
        if unsafe { GetLastError() } == WIN32_ERROR(ERROR_ALREADY_EXISTS) {
            let _ = unsafe { CloseHandle(handle) };
            return Err(AlreadyRunning);
        }
        Ok(Self { handle })
    }
}

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[derive(Debug)]
pub struct AlreadyRunning;

/// Serialize configuration transactions across diagnostics and tray processes.
/// The OS releases ownership on process termination (including abandonment).
pub struct ConfigMutex(HANDLE);
impl ConfigMutex {
    pub fn acquire() -> std::io::Result<Self> {
        use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::WaitForSingleObject;
        let handle = unsafe {
            CreateMutexW(
                None,
                false,
                w!("Local\\KakaoTalkLayoutAdBlocker_Config_v11"),
            )
        }
        .map_err(std::io::Error::other)?;
        let result = unsafe { WaitForSingleObject(handle, 5000) };
        if result == WAIT_OBJECT_0 || result == WAIT_ABANDONED {
            Ok(Self(handle))
        } else {
            let _ = unsafe { CloseHandle(handle) };
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "configuration is busy",
            ))
        }
    }
}
impl Drop for ConfigMutex {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::System::Threading::ReleaseMutex(self.0) };
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;

    #[test]
    fn second_acquire_fails_until_guard_is_dropped() {
        let name = w!("Local\\KakaoTalkLayoutAdBlocker_v11_mutex_hold_test");
        let first = InstanceMutex::acquire_named(name).expect("first acquire");
        assert!(InstanceMutex::acquire_named(name).is_err());
        drop(first);
        let second = InstanceMutex::acquire_named(name).expect("acquire after drop");
        drop(second);
    }
}
