pub mod macos;
pub mod manager;
pub mod model;

#[cfg(target_os = "windows")]
pub mod windows;

pub use manager::{restore_file, SystemProxyManager, SystemProxyStatus};
