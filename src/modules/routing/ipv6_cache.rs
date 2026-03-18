use crate::modules::telegram::TelegramIpEntry;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::sync::Mutex;

static LEARNED_IPV6_DC: Lazy<Mutex<HashMap<Ipv6Addr, TelegramIpEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(super) fn learned_ipv6_entry(host: &str) -> Option<TelegramIpEntry> {
    let ip = host.parse::<Ipv6Addr>().ok()?;
    LEARNED_IPV6_DC
        .lock()
        .expect("ipv6 dc cache lock")
        .get(&ip)
        .copied()
}

pub(super) fn remember_ipv6_entry(host: &str, entry: TelegramIpEntry) {
    let Ok(ip) = host.parse::<Ipv6Addr>() else {
        return;
    };
    LEARNED_IPV6_DC
        .lock()
        .expect("ipv6 dc cache lock")
        .insert(ip, entry);
}
