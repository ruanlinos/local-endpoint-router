use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Addresses the proxy must never dial, including explicit rule destinations.
pub fn is_always_blocked(ip: IpAddr) -> bool {
    let ip = canonical(ip);
    is_metadata(ip) || is_unspecified(ip) || ip.is_multicast()
}

/// Passthrough and unmatched CONNECT targets. Explicit rule destinations are not passed through here.
pub fn is_passthrough_blocked(ip: IpAddr) -> bool {
    let ip = canonical(ip);
    is_always_blocked(ip) || is_loopback(ip) || is_private(ip) || is_link_local(ip)
}

pub fn is_metadata(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => v4 == Ipv4Addr::new(169, 254, 169, 254),
        IpAddr::V6(v6) => v6 == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254),
    }
}

pub fn is_unspecified(ip: IpAddr) -> bool {
    canonical(ip).is_unspecified()
}

pub fn is_loopback(ip: IpAddr) -> bool {
    canonical(ip).is_loopback()
}

pub fn is_private(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => is_unique_local(v6),
    }
}

pub fn is_link_local(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => is_v6_link_local(v6),
    }
}

pub fn is_loopback_bind(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(ip) => is_loopback(ip),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        other => other,
    }
}

fn is_unique_local(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

fn is_v6_link_local(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn classifies_restricted_addresses() {
        assert!(is_loopback(v4(127, 0, 0, 1)));
        assert!(is_private(v4(10, 1, 2, 3)));
        assert!(is_private(v4(192, 168, 1, 9)));
        assert!(is_private(v4(172, 16, 0, 4)));
        assert!(!is_private(v4(172, 15, 0, 1)));
        assert!(is_link_local(v4(169, 254, 1, 1)));
        assert!(is_metadata(v4(169, 254, 169, 254)));
        assert!(is_unspecified(v4(0, 0, 0, 0)));
        assert!(!is_passthrough_blocked(v4(1, 1, 1, 1)));
        assert!(is_passthrough_blocked(v4(127, 0, 0, 2)));
        assert!(is_passthrough_blocked(v4(10, 0, 0, 1)));
        assert!(is_passthrough_blocked(v4(169, 254, 169, 254)));
        assert!(!is_always_blocked(v4(127, 0, 0, 1)));
        assert!(is_always_blocked(v4(169, 254, 169, 254)));
        assert!(is_always_blocked(v4(0, 0, 0, 0)));

        let meta_v6: IpAddr = "fd00:ec2::254".parse().unwrap();
        assert!(is_metadata(meta_v6));
        assert!(is_always_blocked(meta_v6));
        assert!(is_loopback("::1".parse().unwrap()));
        assert!(is_private("fc00::1".parse().unwrap()));
        assert!(is_link_local("fe80::1".parse().unwrap()));
        assert!(is_loopback("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_passthrough_blocked("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_loopback_bind("127.0.0.1"));
        assert!(is_loopback_bind("::1"));
        assert!(!is_loopback_bind("0.0.0.0"));
    }
}
