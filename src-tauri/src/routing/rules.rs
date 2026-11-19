use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AppError;
use crate::routing::guard::is_always_blocked;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPattern {
    Exact(String),
    Suffix(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub source_host: String,
    pub source_port: Option<u16>,
    pub methods: Vec<String>,
    pub path_prefix: Option<String>,
    pub rewrite_from: Option<String>,
    pub rewrite_to: Option<String>,
    pub destination: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_priority")]
    pub priority: i32,
    pub source_host: String,
    #[serde(default)]
    pub source_port: Option<u16>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub rewrite_from: Option<String>,
    #[serde(default)]
    pub rewrite_to: Option<String>,
    pub destination: String,
}

fn default_enabled() -> bool {
    true
}

fn default_priority() -> i32 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDestination {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub base_path: String,
}

impl ParsedDestination {
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else if (self.scheme == "http" && self.port == 80) || (self.scheme == "https" && self.port == 443)
        {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn origin(&self) -> String {
        format!("{}://{}", self.scheme, self.authority())
    }
}

pub fn parse_host_pattern(input: &str) -> Result<HostPattern, AppError> {
    let host = input.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.contains('/') || host.contains(':') || host.contains('@') {
        return Err(AppError::message("source host must be a hostname"));
    }
    if let Some(suffix) = host.strip_prefix("*.") {
        if suffix.is_empty() || suffix.contains('*') || !is_dns_name(suffix) {
            return Err(AppError::message("wildcard source host must look like *.example.com"));
        }
        return Ok(HostPattern::Suffix(suffix.to_string()));
    }
    if host.contains('*') || !is_dns_name(&host) && host.parse::<IpAddr>().is_err() {
        return Err(AppError::message(format!("invalid source host `{host}`")));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_always_blocked(ip) {
            return Err(AppError::message("source host is not allowed"));
        }
    }
    Ok(HostPattern::Exact(host))
}

pub fn host_matches(pattern: &HostPattern, host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match pattern {
        HostPattern::Exact(expected) => host == *expected,
        HostPattern::Suffix(suffix) => {
            host.len() > suffix.len()
                && host.ends_with(suffix)
                && host.as_bytes().get(host.len() - suffix.len() - 1) == Some(&b'.')
        }
    }
}

pub fn validate_rule_input(input: &RuleInput) -> Result<NormalizedRule, AppError> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::message("rule name must be 1-120 characters"));
    }
    let pattern = parse_host_pattern(&input.source_host)?;
    if let Some(port) = input.source_port {
        if port == 0 {
            return Err(AppError::message("source port must be between 1 and 65535"));
        }
    }
    let methods = normalize_methods(&input.methods)?;
    let path_prefix = normalize_path_prefix(input.path_prefix.as_deref())?;
    let (rewrite_from, rewrite_to) = normalize_rewrite(
        input.rewrite_from.as_deref(),
        input.rewrite_to.as_deref(),
    )?;
    let destination = normalize_destination(&input.destination)?;
    let source_host = match &pattern {
        HostPattern::Exact(host) => host.clone(),
        HostPattern::Suffix(suffix) => format!("*.{suffix}"),
    };
    Ok(NormalizedRule {
        name: name.to_string(),
        enabled: input.enabled,
        priority: input.priority,
        source_host,
        source_port: input.source_port,
        methods,
        path_prefix,
        rewrite_from,
        rewrite_to,
        destination: destination_url(&destination),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRule {
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub source_host: String,
    pub source_port: Option<u16>,
    pub methods: Vec<String>,
    pub path_prefix: Option<String>,
    pub rewrite_from: Option<String>,
    pub rewrite_to: Option<String>,
    pub destination: String,
}

pub fn parse_destination(raw: &str) -> Result<ParsedDestination, AppError> {
    let url = Url::parse(raw.trim()).map_err(|_| AppError::message("destination must be an http(s) URL"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(AppError::message("destination scheme must be http or https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::message("destination must not include userinfo"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::message("destination must not include a query or fragment"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::message("destination host is required"))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(AppError::message("destination host is required"));
    }
    if is_metadata_name(&host) {
        return Err(AppError::message("destination host is a metadata address"));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_always_blocked(ip) {
            return Err(AppError::message("destination host is not allowed"));
        }
    } else if !is_dns_name(&host) {
        return Err(AppError::message("destination host is invalid"));
    }
    let port = url.port().unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    let base_path = if url.path() == "/" { String::new() } else { url.path().trim_end_matches('/').to_string() };
    Ok(ParsedDestination {
        scheme: url.scheme().to_string(),
        host,
        port,
        base_path,
    })
}

pub fn normalize_destination(raw: &str) -> Result<ParsedDestination, AppError> {
    parse_destination(raw)
}

fn destination_url(dest: &ParsedDestination) -> String {
    if dest.base_path.is_empty() {
        dest.origin()
    } else {
        format!("{}{}", dest.origin(), dest.base_path)
    }
}

fn normalize_methods(methods: &[String]) -> Result<Vec<String>, AppError> {
    let mut out = Vec::new();
    for method in methods {
        let method = method.trim().to_ascii_uppercase();
        if method.is_empty() {
            continue;
        }
        if method != "*" && !method.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return Err(AppError::message(format!("invalid method `{method}`")));
        }
        if !out.iter().any(|existing: &String| existing == &method) {
            out.push(method);
        }
    }
    if out.iter().any(|method| method == "*") {
        return Ok(Vec::new());
    }
    Ok(out)
}

fn normalize_path_prefix(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !value.starts_with('/') || value.contains('?') || value.contains(' ') {
        return Err(AppError::message("path prefix must start with /"));
    }
    Ok(Some(value.to_string()))
}

fn normalize_rewrite(from: Option<&str>, to: Option<&str>) -> Result<(Option<String>, Option<String>), AppError> {
    let from = from.map(str::trim).filter(|value| !value.is_empty());
    let to = to.map(str::trim).filter(|value| !value.is_empty());
    match (from, to) {
        (None, None) => Ok((None, None)),
        (Some(from), Some(to)) => {
            if !from.starts_with('/') || from.contains('?') {
                return Err(AppError::message("rewrite from must start with /"));
            }
            if !to.starts_with('/') || to.contains('?') {
                return Err(AppError::message("rewrite to must start with /"));
            }
            Ok((Some(from.to_string()), Some(to.to_string())))
        }
        _ => Err(AppError::message("rewrite from and rewrite to must both be set")),
    }
}

fn is_metadata_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("metadata.google.internal")
        || host.eq_ignore_ascii_case("metadata.google.com")
}

fn is_dns_name(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RuleInput {
        RuleInput {
            name: "payments".into(),
            enabled: true,
            priority: 10,
            source_host: "payments.api.company.com".into(),
            source_port: None,
            methods: vec![],
            path_prefix: None,
            rewrite_from: None,
            rewrite_to: None,
            destination: "http://localhost:5002".into(),
        }
    }

    #[test]
    fn accepts_loopback_destination_and_wildcard_host() {
        let mut input = sample();
        input.source_host = "*.company.com".into();
        input.destination = "http://127.0.0.1:5002/base/".into();
        let rule = validate_rule_input(&input).unwrap();
        assert_eq!(rule.source_host, "*.company.com");
        assert_eq!(rule.destination, "http://127.0.0.1:5002/base");
        assert!(host_matches(&parse_host_pattern("*.company.com").unwrap(), "payments.api.company.com"));
        assert!(!host_matches(&parse_host_pattern("*.company.com").unwrap(), "company.com"));
        assert!(!host_matches(&parse_host_pattern("*.company.com").unwrap(), "evilcompany.com"));
    }

    #[test]
    fn rejects_bad_destinations() {
        let mut input = sample();
        input.destination = "ftp://localhost/x".into();
        assert!(validate_rule_input(&input).is_err());
        input.destination = "http://user:pass@localhost:5002".into();
        assert!(validate_rule_input(&input).is_err());
        input.destination = "http://169.254.169.254/latest".into();
        assert!(validate_rule_input(&input).is_err());
        input.destination = "http://0.0.0.0:5002".into();
        assert!(validate_rule_input(&input).is_err());
        input.destination = "http://[fd00:ec2::254]/".into();
        assert!(validate_rule_input(&input).is_err());
    }
}
