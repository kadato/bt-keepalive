//! Single-instance guard.
//!
//! Windows uses a global named mutex so only one copy runs at a time.
//! Other platforms always report success; the app is Windows-only and
//! CI covers the real path.

/// Try to become the running instance. Returns true when acquired.
#[cfg(target_os = "windows")]
#[must_use]
pub fn acquire() -> bool {
    acquire_windows()
}

#[cfg(target_os = "windows")]
fn acquire_windows() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    unsafe {
        match CreateMutexW(None, true, w!("Global\\BTKeepAlive_SingleInstance_Mutex")) {
            Ok(handle) => {
                let already = GetLastError() == ERROR_ALREADY_EXISTS;
                if already {
                    let _ = CloseHandle(handle);
                    return false;
                }
                // `HANDLE` is a plain pointer with no `Drop` closer: not
                // closing keeps the mutex alive for the process lifetime.
                let _ = handle;
                true
            }
            Err(_) => true,
        }
    }
}

/// Non-Windows stub: always succeeds.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn acquire() -> bool {
    true
}
