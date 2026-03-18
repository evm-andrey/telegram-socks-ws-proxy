use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use tokio::net::TcpListener;

pub fn bind_host(host: &str, port: u16) -> io::Result<Vec<(TcpListener, SocketAddr)>> {
    let normalized = normalize_host(host);
    let specs = if host_is_unspecified(normalized) {
        vec![
            (SocketAddr::from(([0, 0, 0, 0], port)), false),
            (SocketAddr::from(([0u16; 8], port)), true),
        ]
    } else {
        let mut resolved = Vec::new();
        for addr in (normalized, port).to_socket_addrs()? {
            resolved.push((addr, addr.is_ipv6()));
        }
        resolved
    };

    if specs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("no addresses resolved for {host}:{port}"),
        ));
    }

    let mut seen = HashSet::new();
    let mut listeners = Vec::new();
    for (addr, v6_only) in specs {
        if !seen.insert(addr) {
            continue;
        }
        let std_listener = bind_socket(addr, v6_only)?;
        let listener = TcpListener::from_std(std_listener)?;
        listeners.push((listener, addr));
    }
    Ok(listeners)
}

pub fn split_host_port(addr: &str) -> io::Result<(String, u16)> {
    if let Ok(parsed) = addr.parse::<SocketAddr>() {
        return Ok((parsed.ip().to_string(), parsed.port()));
    }

    let (host, port) = addr.rsplit_once(':').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid socket address: {addr}"),
        )
    })?;
    let port = port.parse::<u16>().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid port in address {addr}: {err}"),
        )
    })?;
    Ok((normalize_host(host).to_string(), port))
}

pub fn normalize_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host)
}

pub fn host_is_unspecified(host: &str) -> bool {
    normalize_host(host)
        .parse::<IpAddr>()
        .map(|ip| ip.is_unspecified())
        .unwrap_or(false)
}

fn bind_socket(addr: SocketAddr, v6_only: bool) -> io::Result<std::net::TcpListener> {
    let domain = if addr.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    if addr.is_ipv6() {
        socket.set_only_v6(v6_only)?;
    }
    socket.bind(&addr.into())?;
    socket.listen(4096)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::{host_is_unspecified, normalize_host, split_host_port};

    #[test]
    fn normalize_ipv6_host_removes_brackets() {
        assert_eq!(normalize_host("[2001:db8::1]"), "2001:db8::1");
        assert_eq!(normalize_host("2001:db8::1"), "2001:db8::1");
    }

    #[test]
    fn split_socket_addr_supports_bracketed_ipv6() {
        let (host, port) = split_host_port("[2001:db8::1]:8080").unwrap();
        assert_eq!(host, "2001:db8::1");
        assert_eq!(port, 8080);
    }

    #[test]
    fn split_socket_addr_supports_ipv4() {
        let (host, port) = split_host_port("0.0.0.0:8080").unwrap();
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 8080);
    }

    #[test]
    fn wildcard_detection_supports_both_families() {
        assert!(host_is_unspecified("0.0.0.0"));
        assert!(host_is_unspecified("::"));
        assert!(host_is_unspecified("[::]"));
        assert!(!host_is_unspecified("2001:db8::1"));
    }
}
