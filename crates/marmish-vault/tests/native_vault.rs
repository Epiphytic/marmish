#[cfg(target_os = "linux")]
#[path = "support/native_linux.rs"]
mod linux;

#[cfg(target_os = "macos")]
#[path = "support/native_macos.rs"]
mod macos;
