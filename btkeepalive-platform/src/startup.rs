//! Launch-at-startup via the per-user Run key.
//!
//! `is_enabled` reads the value, `set_enabled` writes or removes it.

use std::io;
use std::path::Path;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const VALUE_NAME: &str = "BTKeepAlive";

/// True when the Run value points at `exe`.
#[cfg(target_os = "windows")]
#[must_use]
pub fn is_enabled(exe: &Path) -> bool {
    use windows_registry::CURRENT_USER;
    match CURRENT_USER.open(RUN_SUBKEY) {
        Ok(key) => match key.get_string(VALUE_NAME) {
            Ok(current) => Path::new(&current) == exe,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Add or remove the Run value. Removing a missing value counts as done.
#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool, exe: &Path) -> io::Result<()> {
    use windows_registry::CURRENT_USER;
    let key = CURRENT_USER
        .create(RUN_SUBKEY)
        .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e))?;
    if enabled {
        let path = exe.to_string_lossy().into_owned();
        key.set_string(VALUE_NAME, &path)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e))?;
    } else {
        // Missing value is fine; anything else surfaces.
        match key.remove_value(VALUE_NAME) {
            Ok(()) => {}
            Err(e) => {
                if is_enabled(exe) {
                    return Err(io::Error::other(e));
                }
            }
        }
    }
    Ok(())
}

/// Non-Windows stubs: startup is unsupported, never enabled.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn is_enabled(_exe: &Path) -> bool {
    false
}

/// Non-Windows stub: always reports unsupported.
#[cfg(not(target_os = "windows"))]
pub fn set_enabled(_enabled: bool, _exe: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "launch at startup is Windows-only",
    ))
}
