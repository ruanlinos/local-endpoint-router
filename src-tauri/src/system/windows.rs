use crate::error::AppError;
use crate::system::model::{windows_proxy_override, windows_proxy_server, WindowsSnapshot};

pub fn read() -> Result<WindowsSnapshot, AppError> {
    let key = internet_settings(false)?;
    Ok(WindowsSnapshot {
        enable: key.get_value("ProxyEnable").unwrap_or(0),
        server: key.get_value("ProxyServer").unwrap_or_default(),
        bypass: key.get_value("ProxyOverride").unwrap_or_default(),
    })
}

pub fn apply(host: &str, port: u16, previous: &WindowsSnapshot) -> Result<(), AppError> {
    let key = internet_settings(true)?;
    key.set_value("ProxyEnable", &1u32).map_err(io)?;
    key.set_value("ProxyServer", &windows_proxy_server(host, port)).map_err(io)?;
    key.set_value("ProxyOverride", &windows_proxy_override(&previous.bypass)).map_err(io)?;
    notify();
    Ok(())
}

pub fn restore(previous: &WindowsSnapshot) -> Result<(), AppError> {
    let key = internet_settings(true)?;
    key.set_value("ProxyEnable", &previous.enable).map_err(io)?;
    key.set_value("ProxyServer", &previous.server).map_err(io)?;
    key.set_value("ProxyOverride", &previous.bypass).map_err(io)?;
    notify();
    Ok(())
}

fn internet_settings(write: bool) -> Result<winreg::RegKey, AppError> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;
    let flags = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            flags,
        )
        .map_err(io)
}

fn notify() {
    unsafe {
        InternetSetOptionW(std::ptr::null_mut(), 39, std::ptr::null_mut(), 0);
        InternetSetOptionW(std::ptr::null_mut(), 37, std::ptr::null_mut(), 0);
    }
}

fn io(err: std::io::Error) -> AppError {
    AppError::message(format!("Windows proxy setting failed: {err}"))
}

#[link(name = "wininet")]
extern "system" {
    fn InternetSetOptionW(internet: *mut core::ffi::c_void, option: u32, buffer: *mut core::ffi::c_void, length: u32) -> i32;
}
