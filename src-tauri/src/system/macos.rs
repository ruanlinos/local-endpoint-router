use std::process::Command;

use crate::error::AppError;
use crate::system::model::{parse_bypass, parse_enabled_server_port, parse_services, merge_bypass, ServiceSnapshot};

pub fn read_services() -> Result<Vec<ServiceSnapshot>, AppError> {
    let output = run(&["-listallnetworkservices"])?;
    let mut services = Vec::new();
    for name in parse_services(&output) {
        let web = run(&["-getwebproxy", &name])?;
        let secure = run(&["-getsecurewebproxy", &name])?;
        let bypass = run(&["-getproxybypassdomains", &name])?;
        let (web_enabled, web_server, web_port) = parse_enabled_server_port(&web);
        let (secure_enabled, secure_server, secure_port) = parse_enabled_server_port(&secure);
        services.push(ServiceSnapshot {
            name,
            web_enabled,
            web_server,
            web_port,
            secure_enabled,
            secure_server,
            secure_port,
            bypass: parse_bypass(&bypass),
        });
    }
    Ok(services)
}

pub fn apply(host: &str, port: u16, previous: &[ServiceSnapshot]) -> Result<(), AppError> {
    for service in previous {
        set_proxy(&service.name, host, port)?;
        let bypass = merge_bypass(&service.bypass);
        let mut args = vec!["-setproxybypassdomains".to_string(), service.name.clone()];
        args.extend(bypass);
        run_owned(&args)?;
    }
    Ok(())
}

pub fn restore(previous: &[ServiceSnapshot]) -> Result<(), AppError> {
    for service in previous {
        restore_one("-setwebproxy", "-setwebproxystate", &service.name, service.web_enabled, &service.web_server, service.web_port)?;
        restore_one(
            "-setsecurewebproxy",
            "-setsecurewebproxystate",
            &service.name,
            service.secure_enabled,
            &service.secure_server,
            service.secure_port,
        )?;
        let mut args = vec!["-setproxybypassdomains".to_string(), service.name.clone()];
        if service.bypass.is_empty() {
            args.push("Empty".into());
        } else {
            args.extend(service.bypass.clone());
        }
        run_owned(&args)?;
    }
    Ok(())
}

fn set_proxy(service: &str, host: &str, port: u16) -> Result<(), AppError> {
    let port = port.to_string();
    run(&["-setwebproxy", service, host, &port])?;
    run(&["-setwebproxystate", service, "on"])?;
    run(&["-setsecurewebproxy", service, host, &port])?;
    run(&["-setsecurewebproxystate", service, "on"])?;
    Ok(())
}

fn restore_one(set_cmd: &str, state_cmd: &str, service: &str, enabled: bool, server: &str, port: u16) -> Result<(), AppError> {
    if enabled && !server.is_empty() && port != 0 {
        let port = port.to_string();
        run(&[set_cmd, service, server, &port])?;
        run(&[state_cmd, service, "on"])?;
    } else {
        run(&[state_cmd, service, "off"])?;
    }
    Ok(())
}

fn run(args: &[&str]) -> Result<String, AppError> {
    let output = Command::new("networksetup")
        .args(args)
        .output()
        .map_err(|err| AppError::message(format!("networksetup failed to start: {err}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::message(format!("networksetup {} failed: {stderr}", args.first().unwrap_or(&""))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_owned(args: &[String]) -> Result<String, AppError> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs)
}
