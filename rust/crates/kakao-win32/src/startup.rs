#![cfg(windows)]

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run");
const APPROVED_KEY: PCWSTR =
    w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const VALUE_NAME: PCWSTR = w!("KakaoTalkAdBlockerLayout");

pub fn startup_approved_is_enabled(data: &[u8]) -> bool {
    match data.first() {
        Some(flag) => flag & 1 == 0,
        None => true,
    }
}

pub fn startup_approved_enable_blob() -> [u8; 12] {
    let mut blob = [0u8; 12];
    blob[0] = 0x02;
    blob
}

/// A missing Run key is not an access problem: a fresh Windows profile may not
/// have `HKCU\...\Run` yet, and `set_run_command` creates it on demand. Only a
/// real failure (permissions, corruption) counts as inaccessible.
pub fn run_key_status_is_accessible(status: WIN32_ERROR) -> bool {
    status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
}

/// Whether the HKCU Run key can be read (or legitimately does not exist yet).
pub fn probe_run_key_readable() -> bool {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_READ, &mut key) };
    if status == ERROR_SUCCESS {
        let _ = unsafe { RegCloseKey(key) };
    }
    run_key_status_is_accessible(status)
}

/// Whether the HKCU Run key can be written. Opening with KEY_SET_VALUE checks
/// the ACL without touching any value; a missing key is fine because
/// `set_run_command` creates it.
pub fn probe_run_key_writable() -> bool {
    let mut key = HKEY::default();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_SET_VALUE, &mut key) };
    if status == ERROR_SUCCESS {
        let _ = unsafe { RegCloseKey(key) };
    }
    run_key_status_is_accessible(status)
}

pub fn get_run_command() -> Option<String> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_READ, &mut key) };
    if status != ERROR_SUCCESS {
        return None;
    }
    // Ask for the size first: a fixed buffer reported ERROR_MORE_DATA as
    // "missing", which made the startup repair path overwrite a longer command.
    let mut size = 0u32;
    let mut kind = REG_VALUE_TYPE::default();
    let status = unsafe {
        RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        let _ = unsafe { RegCloseKey(key) };
        return None;
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2).max(1)];
    let mut size = (data.len() * 2) as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return None;
    }
    let chars = (size as usize) / 2;
    let end = data[..chars.min(data.len())]
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(chars.min(data.len()));
    Some(String::from_utf16_lossy(&data[..end]))
}

pub fn set_run_command(command: &str) -> bool {
    let mut key = HKEY::default();
    // RegCreateKeyExW opens the key when it exists and creates it when it does
    // not. A fresh profile can be missing HKCU\...\Run entirely, and the old
    // open-only call made the startup toggle fail on such a machine.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    let mut wide: Vec<u16> = command.encode_utf16().collect();
    wide.push(0);
    let bytes = unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
    let status = unsafe { RegSetValueExW(key, VALUE_NAME, Some(0), REG_SZ, Some(bytes)) };
    let _ = unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return false;
    }
    let _ = set_startup_approved_enabled();
    true
}

pub fn delete_run_command() -> bool {
    let mut key = HKEY::default();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), KEY_SET_VALUE, &mut key) };
    if status != ERROR_SUCCESS {
        return false;
    }
    let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    let _ = unsafe { RegCloseKey(key) };
    let _ = delete_startup_approved();
    status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
}

pub fn ensure_startup_approved_enabled() -> bool {
    if startup_approved_is_enabled(&read_startup_approved()) {
        return true;
    }
    set_startup_approved_enabled()
}

fn read_startup_approved() -> Vec<u8> {
    let mut key = HKEY::default();
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, APPROVED_KEY, Some(0), KEY_READ, &mut key) };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut data = vec![0u8; 16];
    let mut size = data.len() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let status = unsafe {
        RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    data.truncate(size as usize);
    data
}

fn set_startup_approved_enabled() -> bool {
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            APPROVED_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    let blob = startup_approved_enable_blob();
    let status = unsafe { RegSetValueExW(key, VALUE_NAME, Some(0), REG_BINARY, Some(&blob)) };
    let _ = unsafe { RegCloseKey(key) };
    status == ERROR_SUCCESS
}

fn delete_startup_approved() -> bool {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            APPROVED_KEY,
            Some(0),
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return true;
    }
    let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    let _ = unsafe { RegCloseKey(key) };
    status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_approved_odd_status_is_disabled() {
        assert!(startup_approved_is_enabled(&[0x02, 0, 0, 0]));
        assert!(startup_approved_is_enabled(&[0x06, 0, 0, 0]));
        assert!(startup_approved_is_enabled(&[]));
        assert!(!startup_approved_is_enabled(&[0x03, 0, 0, 0]));
        assert!(!startup_approved_is_enabled(&[0x07, 0, 0, 0]));
    }

    #[test]
    fn missing_run_key_is_accessible_but_real_errors_are_not() {
        // A fresh Windows profile (and every GitHub hosted runner) can be
        // missing HKCU\...\Run entirely. Treating that as an access failure
        // made --self-check report a core failure and broke --strict-self-check
        // on CI while passing on any developer machine that had the key.
        assert!(run_key_status_is_accessible(ERROR_SUCCESS));
        assert!(run_key_status_is_accessible(ERROR_FILE_NOT_FOUND));
        assert!(!run_key_status_is_accessible(WIN32_ERROR(5))); // ERROR_ACCESS_DENIED
        assert!(!run_key_status_is_accessible(WIN32_ERROR(1018))); // ERROR_KEY_DELETED
    }

    #[test]
    fn probes_do_not_depend_on_the_run_key_existing() {
        // Whatever this host looks like, both probes must agree that the key is
        // reachable; only a permission/corruption error may report false.
        assert!(probe_run_key_readable());
        assert!(probe_run_key_writable());
    }

    #[test]
    fn startup_approved_enable_blob_is_enabled() {
        let blob = startup_approved_enable_blob();
        assert_eq!(blob.len(), 12);
        assert!(startup_approved_is_enabled(&blob));
    }
}
