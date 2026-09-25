fn main() {
    // NOTE: do not use `#[cfg(target_os = "windows")]` here. Build scripts
    // run on the host, so that cfg is false when cross-compiling from Linux
    // to Windows and tauri-build would silently skip the Windows manifest,
    // icon, and WebView2Loader staging. That missing Common-Controls v6
    // manifest is what causes "TaskDialogIndirect could not be located"
    // against comctl32 v5 at startup.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        tauri_build::build();
    }
}
