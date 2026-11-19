use crate::routing::rules::{host_matches, HostPattern, ParsedDestination};

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub name: String,
    pub priority: i32,
    pub host: HostPattern,
    pub source_port: Option<u16>,
    pub methods: Vec<String>,
    pub path_prefix: Option<String>,
    pub rewrite_from: Option<String>,
    pub rewrite_to: Option<String>,
    pub destination: ParsedDestination,
}

#[derive(Debug, Clone, Copy)]
pub struct MatchInput<'a> {
    pub host: &'a str,
    pub port: u16,
    pub method: &'a str,
    pub path: &'a str,
    pub query: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub rule_id: String,
    pub rule_name: String,
    pub destination_url: String,
}

pub fn compile_rule(rule: &crate::routing::rules::Rule) -> Result<CompiledRule, crate::error::AppError> {
    Ok(CompiledRule {
        id: rule.id.clone(),
        name: rule.name.clone(),
        priority: rule.priority,
        host: crate::routing::rules::parse_host_pattern(&rule.source_host)?,
        source_port: rule.source_port,
        methods: rule.methods.clone(),
        path_prefix: rule.path_prefix.clone(),
        rewrite_from: rule.rewrite_from.clone(),
        rewrite_to: rule.rewrite_to.clone(),
        destination: crate::routing::rules::parse_destination(&rule.destination)?,
    })
}

pub fn select<'a>(rules: &'a [CompiledRule], input: MatchInput<'_>) -> Option<&'a CompiledRule> {
    let mut best: Option<&CompiledRule> = None;
    for rule in rules {
        if !rule_matches(rule, input) {
            continue;
        }
        best = Some(match best {
            None => rule,
            Some(current) if prefer(rule, current) => rule,
            Some(current) => current,
        });
    }
    best
}

pub fn route_of(rule: &CompiledRule, input: MatchInput<'_>) -> Route {
    let path = apply_rewrite(normalize_path(input.path), rule.rewrite_from.as_deref(), rule.rewrite_to.as_deref());
    Route {
        rule_id: rule.id.clone(),
        rule_name: rule.name.clone(),
        destination_url: join_destination(&rule.destination, &path, input.query),
    }
}

pub fn host_mapped(rules: &[CompiledRule], host: &str, port: u16) -> bool {
    rules.iter().any(|rule| host_matches(&rule.host, host) && port_matches(rule.source_port, port))
}

fn rule_matches(rule: &CompiledRule, input: MatchInput<'_>) -> bool {
    if !host_matches(&rule.host, input.host) || !port_matches(rule.source_port, input.port) {
        return false;
    }
    if !rule.methods.is_empty()
        && !rule
            .methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case(input.method))
    {
        return false;
    }
    match &rule.path_prefix {
        Some(prefix) => path_prefix_matches(prefix, normalize_path(input.path)),
        None => true,
    }
}

fn prefer(candidate: &CompiledRule, current: &CompiledRule) -> bool {
    if candidate.priority != current.priority {
        return candidate.priority < current.priority;
    }
    let candidate_prefix = prefix_len(candidate);
    let current_prefix = prefix_len(current);
    if candidate_prefix != current_prefix {
        return candidate_prefix > current_prefix;
    }
    let candidate_exact = matches!(candidate.host, HostPattern::Exact(_));
    let current_exact = matches!(current.host, HostPattern::Exact(_));
    if candidate_exact != current_exact {
        return candidate_exact;
    }
    candidate.id < current.id
}

fn prefix_len(rule: &CompiledRule) -> usize {
    rule.path_prefix.as_ref().map(|prefix| prefix.trim_end_matches('/').len()).unwrap_or(0)
}

fn port_matches(expected: Option<u16>, actual: u16) -> bool {
    expected.is_none_or(|port| port == actual)
}

fn normalize_path(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

fn path_prefix_matches(prefix: &str, path: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() || prefix == "/" {
        return true;
    }
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

fn apply_rewrite(path: &str, from: Option<&str>, to: Option<&str>) -> String {
    let Some(from) = from else {
        return path.to_string();
    };
    let Some(to) = to else {
        return path.to_string();
    };
    let from = from.trim_end_matches('/');
    if from.is_empty() {
        return path.to_string();
    }
    if path == from {
        return to.to_string();
    }
    if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
        let to = to.trim_end_matches('/');
        return format!("{to}/{rest}");
    }
    path.to_string()
}

fn join_destination(dest: &ParsedDestination, path: &str, query: Option<&str>) -> String {
    let path = if dest.base_path.is_empty() {
        path.to_string()
    } else if path == "/" {
        dest.base_path.clone()
    } else {
        format!("{}{path}", dest.base_path.trim_end_matches('/'))
    };
    let mut url = format!("{}://{}{path}", dest.scheme, dest.authority());
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        url.push('?');
        url.push_str(query.trim_start_matches('?'));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::rules::{parse_destination, parse_host_pattern};

    fn rule(
        id: &str,
        priority: i32,
        host: &str,
        path: Option<&str>,
        destination: &str,
    ) -> CompiledRule {
        CompiledRule {
            id: id.into(),
            name: id.into(),
            priority,
            host: parse_host_pattern(host).unwrap(),
            source_port: None,
            methods: vec![],
            path_prefix: path.map(str::to_string),
            rewrite_from: None,
            rewrite_to: None,
            destination: parse_destination(destination).unwrap(),
        }
    }

    fn input<'a>(host: &'a str, path: &'a str) -> MatchInput<'a> {
        MatchInput { host, port: 80, method: "GET", path, query: None }
    }

    #[test]
    fn priority_then_prefix_then_exact_host() {
        let rules = vec![
            rule("wild", 10, "*.company.com", None, "http://127.0.0.1:5001"),
            rule("exact-short", 10, "payments.api.company.com", Some("/api"), "http://127.0.0.1:5002"),
            rule("exact-long", 10, "payments.api.company.com", Some("/api/payments"), "http://127.0.0.1:5003"),
            rule("urgent", 1, "*.company.com", None, "http://127.0.0.1:5004"),
        ];
        let picked = select(&rules, input("payments.api.company.com", "/api/payments/123")).unwrap();
        assert_eq!(picked.id, "urgent");

        let same_priority = &rules[..3];
        let picked = select(same_priority, input("payments.api.company.com", "/api/payments/123")).unwrap();
        assert_eq!(picked.id, "exact-long");

        let picked = select(same_priority, input("payments.api.company.com", "/api/other")).unwrap();
        assert_eq!(picked.id, "exact-short");

        let picked = select(&rules[0..2], input("billing.company.com", "/api")).unwrap();
        assert_eq!(picked.id, "wild");
    }

    #[test]
    fn method_port_and_path_boundaries() {
        let mut rule = rule("payments", 10, "payments.api.company.com", Some("/api"), "http://localhost:5002");
        rule.methods = vec!["GET".into()];
        rule.source_port = Some(443);
        let mut input = MatchInput {
            host: "payments.api.company.com",
            port: 443,
            method: "POST",
            path: "/api/payments/123",
            query: Some("x=1"),
        };
        assert!(select(std::slice::from_ref(&rule), input).is_none());
        input.method = "GET";
        input.port = 80;
        assert!(select(std::slice::from_ref(&rule), input).is_none());
        input.port = 443;
        input.path = "/apiv2";
        assert!(select(std::slice::from_ref(&rule), input).is_none());
        input.path = "/api/payments/123";
        let route = route_of(select(std::slice::from_ref(&rule), input).unwrap(), input);
        assert_eq!(route.destination_url, "http://localhost:5002/api/payments/123?x=1");
    }

    #[test]
    fn rewrite_keeps_the_rest_of_the_path() {
        let mut rule = rule("payments", 10, "payments.api.company.com", None, "http://127.0.0.1:5002/base");
        rule.rewrite_from = Some("/api".into());
        rule.rewrite_to = Some("/v2".into());
        let input = MatchInput {
            host: "payments.api.company.com",
            port: 80,
            method: "GET",
            path: "/api/payments/123",
            query: None,
        };
        let route = route_of(&rule, input);
        assert_eq!(route.destination_url, "http://127.0.0.1:5002/base/v2/payments/123");
    }
}
