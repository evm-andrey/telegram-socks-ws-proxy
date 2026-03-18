use once_cell::sync::Lazy;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelegramIpEntry {
    pub dc: u8,
    pub is_media: bool,
}

static IP_TO_DC: Lazy<Vec<(u32, u32, TelegramIpEntry)>> = Lazy::new(|| {
    vec![
        range("149.154.175.50", "149.154.175.50", 1, false),
        range("149.154.175.51", "149.154.175.51", 1, false),
        range("149.154.175.53", "149.154.175.53", 1, false),
        range("149.154.175.54", "149.154.175.54", 1, false),
        range("149.154.175.52", "149.154.175.52", 1, true),
        range("149.154.167.220", "149.154.167.220", 2, false),
        range("149.154.167.41", "149.154.167.41", 2, false),
        range("149.154.167.50", "149.154.167.50", 2, false),
        range("149.154.167.51", "149.154.167.51", 2, false),
        range("95.161.76.100", "95.161.76.100", 2, false),
        range("149.154.167.35", "149.154.167.35", 2, true),
        range("149.154.167.222", "149.154.167.222", 2, true),
        range("149.154.167.151", "149.154.167.151", 2, true),
        range("149.154.167.223", "149.154.167.223", 2, true),
        range("149.154.162.123", "149.154.162.123", 2, true),
        range("149.154.175.100", "149.154.175.100", 3, false),
        range("149.154.175.101", "149.154.175.101", 3, false),
        range("149.154.175.102", "149.154.175.102", 3, true),
        range("149.154.167.91", "149.154.167.91", 4, false),
        range("149.154.167.92", "149.154.167.92", 4, false),
        range("149.154.164.250", "149.154.164.250", 4, true),
        range("149.154.166.120", "149.154.166.120", 4, true),
        range("149.154.166.121", "149.154.166.121", 4, true),
        range("149.154.167.118", "149.154.167.118", 4, true),
        range("149.154.165.111", "149.154.165.111", 4, true),
        range("91.108.56.100", "91.108.56.100", 5, false),
        range("91.108.56.101", "91.108.56.101", 5, false),
        range("91.108.56.116", "91.108.56.116", 5, false),
        range("91.108.56.126", "91.108.56.126", 5, false),
        range("149.154.171.5", "149.154.171.5", 5, false),
        range("91.108.56.102", "91.108.56.102", 5, true),
        range("91.108.56.128", "91.108.56.128", 5, true),
        range("91.108.56.151", "91.108.56.151", 5, true),
    ]
});

static TG_IPV4_RANGES: Lazy<Vec<(u32, u32)>> = Lazy::new(|| {
    vec![
        (ip_to_u32("185.76.151.0"), ip_to_u32("185.76.151.255")),
        (ip_to_u32("149.154.160.0"), ip_to_u32("149.154.175.255")),
        (ip_to_u32("91.105.192.0"), ip_to_u32("91.105.193.255")),
        (ip_to_u32("91.108.0.0"), ip_to_u32("91.108.255.255")),
    ]
});

static TG_IPV6_RANGES: Lazy<Vec<(u128, u128)>> = Lazy::new(|| {
    vec![
        (
            ipv6_to_u128("2001:67c:4e8::"),
            ipv6_to_u128("2001:67c:4e8:ffff:ffff:ffff:ffff:ffff"),
        ),
        (
            ipv6_to_u128("2001:b28:f23d::"),
            ipv6_to_u128("2001:b28:f23d:ffff:ffff:ffff:ffff:ffff"),
        ),
        (
            ipv6_to_u128("2001:b28:f23f::"),
            ipv6_to_u128("2001:b28:f23f:ffff:ffff:ffff:ffff:ffff"),
        ),
    ]
});

pub fn is_telegram_ip(ip: &str) -> bool {
    match IpAddr::from_str(ip) {
        Ok(IpAddr::V4(ip)) => {
            let value = u32::from(ip);
            TG_IPV4_RANGES
                .iter()
                .any(|(start, end)| value >= *start && value <= *end)
        }
        Ok(IpAddr::V6(ip)) => {
            let value = u128::from(ip);
            TG_IPV6_RANGES
                .iter()
                .any(|(start, end)| value >= *start && value <= *end)
        }
        Err(_) => false,
    }
}

pub fn ip_to_dc(ip: &str) -> Option<TelegramIpEntry> {
    let parsed = Ipv4Addr::from_str(ip).ok()?;
    let value = u32::from(parsed);
    IP_TO_DC
        .iter()
        .find(|(start, end, _)| value >= *start && value <= *end)
        .map(|(_, _, entry)| *entry)
}

pub fn ws_domains(dc: u8, is_media: bool) -> Vec<String> {
    if is_media {
        vec![
            format!("kws{dc}-1.web.telegram.org"),
            format!("kws{dc}.web.telegram.org"),
        ]
    } else {
        vec![
            format!("kws{dc}.web.telegram.org"),
            format!("kws{dc}-1.web.telegram.org"),
        ]
    }
}

fn range(start: &str, end: &str, dc: u8, is_media: bool) -> (u32, u32, TelegramIpEntry) {
    (
        ip_to_u32(start),
        ip_to_u32(end),
        TelegramIpEntry { dc, is_media },
    )
}

fn ip_to_u32(ip: &str) -> u32 {
    let parsed = Ipv4Addr::from_str(ip).unwrap();
    u32::from(parsed)
}

fn ipv6_to_u128(ip: &str) -> u128 {
    let parsed = Ipv6Addr::from_str(ip).unwrap();
    u128::from(parsed)
}

#[cfg(test)]
mod tests {
    use super::{ip_to_dc, is_telegram_ip};

    #[test]
    fn telegram_ranges_work() {
        assert!(is_telegram_ip("91.108.0.1"));
        assert!(is_telegram_ip("2001:67c:4e8::1"));
        assert!(!is_telegram_ip("1.1.1.1"));
        assert!(!is_telegram_ip("2001:db8::1"));
    }

    #[test]
    fn dc_lookup_works() {
        let got = ip_to_dc("149.154.167.220").expect("must exist");
        assert_eq!(got.dc, 2);
    }

    #[test]
    fn extended_dc_lookup_works() {
        let got = ip_to_dc("149.154.166.120").expect("must exist");
        assert_eq!(got.dc, 4);
        assert!(got.is_media);
    }

    #[test]
    fn media_dc_lookup_works_for_167_35() {
        let got = ip_to_dc("149.154.167.35").expect("must exist");
        assert_eq!(got.dc, 2);
        assert!(got.is_media);
    }
}
