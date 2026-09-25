//! Default audio device monitoring.
//!
//! Windows uses `IMMNotificationClient` on a dedicated MTA thread, so
//! device switches arrive as events. No polling: the system pushes
//! endpoint changes. Other platforms get a
//! null watcher that never fires.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Device change events the app layer reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    /// Default render endpoint changed, or its state changed.
    DefaultOutputChanged,
    /// A device was added or removed.
    DeviceListChanged,
}

/// Handle that stops the watcher thread on drop.
#[derive(Debug)]
pub struct Shutdown {
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    event: Arc<win::StopEvent>,
}

impl Shutdown {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            event: Arc::new(win::StopEvent::new()),
        }
    }
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(target_os = "windows")]
        self.event.signal();
    }
}

/// Spawn the watcher thread. Returns events; drop `Shutdown` to stop.
#[cfg(target_os = "windows")]
pub fn spawn_watcher() -> (mpsc::Receiver<DeviceEvent>, Shutdown) {
    let (tx, rx) = mpsc::channel();
    let shutdown = Shutdown::new();
    let stop = Arc::clone(&shutdown.stop);
    let event = Arc::clone(&shutdown.event);
    std::thread::Builder::new()
        .name("device-watcher".to_string())
        .spawn(move || win::watcher_thread(tx, stop, event))
        .expect("device watcher thread spawns");
    (rx, shutdown)
}

/// Current default render endpoint ID, if queryable.
#[cfg(target_os = "windows")]
#[must_use]
pub fn default_output_id() -> Option<String> {
    win::default_output_id()
}

/// Null watcher for non-Windows builds. Never sends.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn null_watcher() -> mpsc::Receiver<DeviceEvent> {
    let (_tx, rx) = mpsc::channel();
    rx
}

/// Non-Windows endpoint query. Always `None`.
#[cfg(not(target_os = "windows"))]
#[must_use]
pub fn default_output_id() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::sync::Mutex;
    use windows::core::{implement, Result, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::{
        eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator,
        IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Threading::{CreateEventW, SetEvent};
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW, MSG, MWMO_INPUTAVAILABLE,
        PM_REMOVE, QS_ALLINPUT,
    };

    /// Manual-reset stop event shared with the watcher thread.
    #[derive(Debug)]
    pub(super) struct StopEvent {
        handle: HANDLE,
    }

    impl StopEvent {
        pub(super) fn new() -> Self {
            let handle =
                unsafe { CreateEventW(None, true, false, None).unwrap_or(HANDLE::default()) };
            Self { handle }
        }

        pub(super) fn signal(&self) {
            unsafe {
                let _ = SetEvent(self.handle);
            }
        }
    }

    // HANDLE is Send-safe to share here: only SetEvent/wait use it.
    unsafe impl Send for StopEvent {}
    unsafe impl Sync for StopEvent {}

    #[implement(IMMNotificationClient)]
    struct NotifyClient {
        tx: Mutex<mpsc::Sender<DeviceEvent>>,
    }

    impl NotifyClient {
        fn send(&self, event: DeviceEvent) {
            if let Ok(tx) = self.tx.lock() {
                let _ = tx.send(event);
            }
        }
    }

    impl IMMNotificationClient_Impl for NotifyClient {
        fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
            self.send(DeviceEvent::DefaultOutputChanged);
            Ok(())
        }
        fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
            self.send(DeviceEvent::DeviceListChanged);
            Ok(())
        }
        fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
            self.send(DeviceEvent::DeviceListChanged);
            Ok(())
        }
        fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> Result<()> {
            if flow == eRender && (role == eConsole || role == eMultimedia) {
                self.send(DeviceEvent::DefaultOutputChanged);
            }
            Ok(())
        }
        fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
            Ok(())
        }
    }

    // windows 0.58 builds each interface vtable for the generated
    // `NotifyClient_Impl` wrapper, so the trait needs a forwarding impl
    // on the wrapper too. Verified by compile test; without it the
    // `Vtbl::new` bound is unsatisfiable.
    impl IMMNotificationClient_Impl for NotifyClient_Impl {
        fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> Result<()> {
            use windows_core::IUnknownImpl;
            self.get_impl().OnDeviceStateChanged(id, state)
        }
        fn OnDeviceAdded(&self, id: &PCWSTR) -> Result<()> {
            use windows_core::IUnknownImpl;
            self.get_impl().OnDeviceAdded(id)
        }
        fn OnDeviceRemoved(&self, id: &PCWSTR) -> Result<()> {
            use windows_core::IUnknownImpl;
            self.get_impl().OnDeviceRemoved(id)
        }
        fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, id: &PCWSTR) -> Result<()> {
            use windows_core::IUnknownImpl;
            self.get_impl().OnDefaultDeviceChanged(flow, role, id)
        }
        fn OnPropertyValueChanged(&self, id: &PCWSTR, key: &PROPERTYKEY) -> Result<()> {
            use windows_core::IUnknownImpl;
            self.get_impl().OnPropertyValueChanged(id, key)
        }
    }

    pub(super) fn watcher_thread(
        tx: mpsc::Sender<DeviceEvent>,
        stop: Arc<AtomicBool>,
        stop_event: Arc<StopEvent>,
    ) {
        unsafe {
            if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                return;
            }
            let enumerator: IMMDeviceEnumerator =
                match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    Ok(e) => e,
                    Err(_) => {
                        CoUninitialize();
                        return;
                    }
                };
            let client: IMMNotificationClient = NotifyClient { tx: Mutex::new(tx) }.into();
            if enumerator
                .RegisterEndpointNotificationCallback(&client)
                .is_err()
            {
                CoUninitialize();
                return;
            }

            let handles = [stop_event.handle];
            loop {
                let wait = MsgWaitForMultipleObjectsEx(
                    Some(&handles),
                    u32::MAX,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
                if wait == WAIT_OBJECT_0 || stop.load(Ordering::Relaxed) {
                    break;
                }
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = DispatchMessageW(&msg);
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            let _ = enumerator.UnregisterEndpointNotificationCallback(&client);
            CoUninitialize();
        }
    }

    pub(super) fn default_output_id() -> Option<String> {
        unsafe {
            // Best effort: another mode may already own COM on this thread.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let id = device.GetId().ok()?;
            let text = id.to_string().ok();
            CoTaskMemFree(Some(id.as_ptr() as _));
            CoUninitialize();
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_compare() {
        assert_eq!(
            DeviceEvent::DefaultOutputChanged,
            DeviceEvent::DefaultOutputChanged
        );
        assert_ne!(
            DeviceEvent::DefaultOutputChanged,
            DeviceEvent::DeviceListChanged
        );
    }

    #[test]
    fn shutdown_drops_cleanly() {
        let _s = Shutdown::new();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn null_watcher_never_fires() {
        use std::time::Duration;
        let rx = null_watcher();
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        assert_eq!(default_output_id(), None);
    }
}
