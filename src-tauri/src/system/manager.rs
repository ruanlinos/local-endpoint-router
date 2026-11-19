use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;

use crate::error::AppError;
use crate::system::model::ProxyBackup;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProxyStatus {
    pub enabled: bool,
    pub address: String,
    pub error: Option<String>,
    pub manual_hint: String,
}

pub struct SystemProxyManager {
    path: PathBuf,
    enabled: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl SystemProxyManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            path: data_dir.join("system-proxy-backup.json"),
            enabled: AtomicBool::new(false),
            last_error: Mutex::new(None),
        }
    }

    pub fn backup_path(&self) -> &Path {
        &self.path
    }

    pub fn status(&self, address: &str) -> SystemProxyStatus {
        SystemProxyStatus {
            enabled: self.enabled.load(Ordering::Relaxed),
            address: address.to_string(),
            error: self.last_error.lock().expect("system proxy error").clone(),
            manual_hint: format!("HTTP and HTTPS proxy {address}. Bypass localhost, 127.0.0.1, and ::1."),
        }
    }

    pub fn enable(&self, host: &str, port: u16) -> Result<(), AppError> {
        if !self.path.exists() {
            let backup = read_backup()?;
            write_backup(&self.path, &backup)?;
        }
        let backup = load_backup(&self.path)?;
        let result = apply_ours(host, port, &backup);
        match result {
            Ok(()) => {
                self.enabled.store(true, Ordering::Relaxed);
                *self.last_error.lock().expect("system proxy error") = None;
                Ok(())
            }
            Err(err) => {
                self.enabled.store(false, Ordering::Relaxed);
                let restore_error = restore_backup(&backup).err();
                if restore_error.is_none() {
                    let _ = std::fs::remove_file(&self.path);
                }
                let message = match restore_error {
                    Some(restore_error) => format!("{err}. Restoring the previous proxy also failed: {restore_error}"),
                    None => err.to_string(),
                };
                *self.last_error.lock().expect("system proxy error") = Some(message.clone());
                Err(AppError::message(message))
            }
        }
    }

    pub fn restore(&self) -> Result<(), AppError> {
        if !self.path.exists() {
            self.enabled.store(false, Ordering::Relaxed);
            return Ok(());
        }
        let backup = load_backup(&self.path)?;
        match restore_backup(&backup) {
            Ok(()) => {
                let _ = std::fs::remove_file(&self.path);
                self.enabled.store(false, Ordering::Relaxed);
                *self.last_error.lock().expect("system proxy error") = None;
                Ok(())
            }
            Err(err) => {
                *self.last_error.lock().expect("system proxy error") = Some(err.to_string());
                Err(err)
            }
        }
    }
}

pub fn restore_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let backup = load_backup(path).map_err(|err| err.to_string())?;
    restore_backup(&backup).map_err(|err| err.to_string())?;
    let _ = std::fs::remove_file(path);
    Ok(())
}

fn read_backup() -> Result<ProxyBackup, AppError> {
    #[cfg(target_os = "macos")]
    {
        Ok(ProxyBackup {
            version: 1,
            macos: crate::system::macos::read_services()?,
            windows: None,
        })
    }
    #[cfg(target_os = "windows")]
    {
        Ok(ProxyBackup {
            version: 1,
            macos: Vec::new(),
            windows: Some(crate::system::windows::read()?),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err(AppError::message("system proxy is only implemented for macOS and Windows"))
    }
}

fn apply_ours(host: &str, port: u16, backup: &ProxyBackup) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        return crate::system::macos::apply(host, port, &backup.macos);
    }
    #[cfg(target_os = "windows")]
    {
        let previous = backup.windows.as_ref().ok_or_else(|| AppError::message("Windows proxy backup is missing"))?;
        return crate::system::windows::apply(host, port, previous);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (host, port, backup);
        Err(AppError::message("system proxy is only implemented for macOS and Windows"))
    }
}

fn restore_backup(backup: &ProxyBackup) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        return crate::system::macos::restore(&backup.macos);
    }
    #[cfg(target_os = "windows")]
    {
        let previous = backup.windows.as_ref().ok_or_else(|| AppError::message("Windows proxy backup is missing"))?;
        return crate::system::windows::restore(previous);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = backup;
        Err(AppError::message("system proxy is only implemented for macOS and Windows"))
    }
}

fn write_backup(path: &Path, backup: &ProxyBackup) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| AppError::io(path, err))?;
    }
    let bytes = serde_json::to_vec_pretty(backup)?;
    std::fs::write(path, bytes).map_err(|err| AppError::io(path, err))
}

fn load_backup(path: &Path) -> Result<ProxyBackup, AppError> {
    let bytes = std::fs::read(path).map_err(|err| AppError::io(path, err))?;
    serde_json::from_slice(&bytes).map_err(|err| AppError::message(format!("proxy backup is unreadable: {err}")))
}
