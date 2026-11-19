pub const REQUIRED_BYPASS: &[&str] = &["localhost", "127.0.0.1", "::1"];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub web_enabled: bool,
    pub web_server: String,
    pub web_port: u16,
    pub secure_enabled: bool,
    pub secure_server: String,
    pub secure_port: u16,
    pub bypass: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowsSnapshot {
    pub enable: u32,
    pub server: String,
    pub bypass: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProxyBackup {
    pub version: u32,
    pub macos: Vec<ServiceSnapshot>,
    pub windows: Option<WindowsSnapshot>,
}

pub fn parse_services(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.to_ascii_lowercase().contains("asterisk"))
        .filter(|line| !line.starts_with('*'))
        .map(str::to_string)
        .collect()
}

pub fn parse_enabled_server_port(output: &str) -> (bool, String, u16) {
    let mut enabled = false;
    let mut server = String::new();
    let mut port = 0u16;
    for line in output.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Enabled:") {
            enabled = value.trim().eq_ignore_ascii_case("yes");
        } else if let Some(value) = line.strip_prefix("Server:") {
            server = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Port:") {
            port = value.trim().parse().unwrap_or(0);
        }
    }
    (enabled, server, port)
}

pub fn parse_bypass(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !line.is_empty() && !lower.contains("aren't") && !lower.starts_with("there ")
        })
        .map(str::to_string)
        .collect()
}

pub fn merge_bypass(existing: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for domain in REQUIRED_BYPASS {
        merged.push((*domain).to_string());
    }
    for domain in existing {
        let domain = domain.trim();
        if domain.is_empty() {
            continue;
        }
        if merged.iter().any(|item| item.eq_ignore_ascii_case(domain)) {
            continue;
        }
        merged.push(domain.to_string());
    }
    merged
}

pub fn windows_proxy_server(host: &str, port: u16) -> String {
    format!("http={host}:{port};https={host}:{port}")
}

pub fn windows_proxy_override(existing: &str) -> String {
    let mut parts = merge_bypass(&split_override(existing));
    if !parts.iter().any(|part| part == "<local>") {
        parts.push("<local>".into());
    }
    parts.join(";")
}

fn split_override(existing: &str) -> Vec<String> {
    existing
        .split([';', ','])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_networksetup_and_keeps_localhost_bypass() {
        let services = parse_services(
            "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*Thunderbolt Bridge\nEthernet\n",
        );
        assert_eq!(services, vec!["Wi-Fi".to_string(), "Ethernet".to_string()]);
        let (enabled, server, port) = parse_enabled_server_port("Enabled: Yes\nServer: 10.0.0.1\nPort: 8080\nAuthenticated Proxy Enabled: 0\n");
        assert!(enabled);
        assert_eq!(server, "10.0.0.1");
        assert_eq!(port, 8080);
        let bypass = merge_bypass(&parse_bypass("*.local\n169.254/16\n"));
        assert_eq!(bypass[0], "localhost");
        assert_eq!(bypass[1], "127.0.0.1");
        assert_eq!(bypass[2], "::1");
        assert!(bypass.iter().any(|item| item == "*.local"));
        assert_eq!(windows_proxy_server("127.0.0.1", 8787), "http=127.0.0.1:8787;https=127.0.0.1:8787");
        let over = windows_proxy_override("example.local;<local>");
        assert!(over.split(';').any(|part| part == "localhost"));
        assert!(over.split(';').any(|part| part == "127.0.0.1"));
        assert!(over.split(';').any(|part| part == "::1"));
        assert_eq!(over.matches("<local>").count(), 1);
    }
}
