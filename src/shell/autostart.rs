//! "Start with Windows": a value under the current user's Run key
//! (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`). No admin rights needed.

use super::wide;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW,
};

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "VoxShift";

/// The command Windows runs at logon: this executable, quoted.
pub fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(format!("\"{}\"", exe.display()))
}

pub fn is_enabled() -> bool {
    read(RUN_KEY, VALUE_NAME).is_some()
}

/// True if the Run entry points at a different copy of VoxShift than this one.
pub fn points_elsewhere() -> bool {
    match (read(RUN_KEY, VALUE_NAME), command()) {
        (Some(v), Ok(c)) => !v.eq_ignore_ascii_case(&c),
        _ => false,
    }
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    if on {
        write(RUN_KEY, VALUE_NAME, &command()?)
    } else {
        delete(RUN_KEY, VALUE_NAME)
    }
}

/// Reads a string value under HKCU.
pub fn read(subkey: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(subkey), wide(name));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            n.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1); // size includes the terminating NUL
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn write(subkey: &str, name: &str, value: &str) -> Result<(), String> {
    let (k, n, v) = (wide(subkey), wide(name), wide(value));
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot open registry key (error {rc})"));
        }
        let rc = RegSetValueExW(
            key,
            n.as_ptr(),
            0,
            REG_SZ,
            v.as_ptr().cast(),
            (v.len() * 2) as u32,
        );
        RegCloseKey(key);
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot write registry value (error {rc})"));
        }
    }
    Ok(())
}

/// Deletes a value; deleting one that does not exist is fine.
pub fn delete(subkey: &str, name: &str) -> Result<(), String> {
    let (k, n) = (wide(subkey), wide(name));
    let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("cannot delete registry value (error {rc})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::RegDeleteTreeW;

    /// Never touches the real Run key: works under a throwaway key, removed afterwards.
    #[test]
    fn write_read_delete_round_trip_on_a_scratch_key() {
        let key = format!(r"Software\VoxShiftTest\{}", std::process::id());
        assert_eq!(read(&key, "Entry"), None);
        let cmd = command().unwrap();
        assert!(cmd.starts_with('"') && cmd.ends_with('"'));
        write(&key, "Entry", &cmd).unwrap();
        assert_eq!(read(&key, "Entry").as_deref(), Some(cmd.as_str()));
        delete(&key, "Entry").unwrap();
        assert_eq!(read(&key, "Entry"), None);
        delete(&key, "Entry").unwrap(); // already gone: still fine
        let w = wide(r"Software\VoxShiftTest");
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, w.as_ptr()) };
    }
}
