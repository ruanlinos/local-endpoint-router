use std::path::Path;
use std::process::Command;

use crate::error::AppError;

/// True only when this installation's CA is trusted in the user store.
/// macOS checks the login keychain. Windows checks CurrentUser\Root.
/// A failed or missing trust stays pending; this never installs anything.
pub fn certificate_trusted(path: &Path, fingerprint: &str) -> bool {
    if !path.is_file() || fingerprint.is_empty() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = fingerprint;
        return trusted_macos(path);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = path;
        return trusted_windows(fingerprint);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, fingerprint);
        false
    }
}

#[cfg(target_os = "macos")]
fn trusted_macos(path: &Path) -> bool {
    let Ok(home) = std::env::var("HOME") else {
        return false;
    };
    let keychain = std::path::PathBuf::from(home).join("Library/Keychains/login.keychain-db");
    let output = Command::new("security")
        .args(["verify-cert", "-q", "-L", "-l", "-p", "basic", "-k"])
        .arg(&keychain)
        .arg("-c")
        .arg(path)
        .output();
    matches!(output, Ok(output) if output.status.success())
}

#[cfg(target_os = "windows")]
fn trusted_windows(fingerprint: &str) -> bool {
    let output = match Command::new("certutil").args(["-user", "-store", "Root"]).output() {
        Ok(output) => output,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let needle = fingerprint.to_ascii_lowercase();
    text.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("sha256") && !lower.contains("hash") {
            return false;
        }
        let compact: String = lower.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
        compact.contains(&needle)
    })
}

pub fn trust_cert(path: &Path) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        return trust_macos(path);
    }
    #[cfg(target_os = "windows")]
    {
        return trust_windows(path);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        Err(AppError::message("trusting the local CA is implemented for macOS and Windows"))
    }
}

#[cfg(target_os = "macos")]
fn trust_macos(path: &Path) -> Result<(), AppError> {
    let home = std::env::var("HOME").map_err(|_| AppError::message("HOME is not set"))?;
    let keychain = std::path::PathBuf::from(home).join("Library/Keychains/login.keychain-db");
    let output = Command::new("security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
        .arg(&keychain)
        .arg(path)
        .output()
        .map_err(|err| AppError::message(format!("security failed to start: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(AppError::message(format!(
        "macOS did not trust the CA in the login keychain. {stderr}{stdout}"
    )))
}

#[cfg(target_os = "windows")]
fn trust_windows(path: &Path) -> Result<(), AppError> {
    let output = Command::new("certutil")
        .args(["-user", "-addstore", "Root"])
        .arg(path)
        .output()
        .map_err(|err| AppError::message(format!("certutil failed to start: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(AppError::message(format!(
        "Windows did not add the CA to CurrentUser Root. {stderr}{stdout}"
    )))
}
