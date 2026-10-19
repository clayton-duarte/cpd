//! The review host (contract §6.5, "The review host"): the host Plan review
//! links name, and the value of `PLANNOTATOR_URL_HOST` in every Lead's pi. The
//! cockpit's review link uses the same function.

use std::io::{self, Write};
use std::net::{IpAddr, UdpSocket};

/// Plannotator's own setting, and cpd's override of the review host.
pub const URL_HOST: &str = "PLANNOTATOR_URL_HOST";

/// An address outside every local network (RFC 5737, TEST-NET-1): the OS picks the source address it would use toward
/// it. A UDP `connect` sends no packet.
const OUTSIDE: &str = "192.0.2.1:9";

/// The review host: `PLANNOTATOR_URL_HOST` from cpd's environment, else the primary IPv4, else `localhost`. The
/// fallback's one line goes to `warn`.
pub fn review_host(warn: &mut dyn Write) -> String {
    choose(std::env::var(URL_HOST).ok(), primary_ipv4, warn)
}

/// The source address of the route toward [`OUTSIDE`].
pub fn primary_ipv4() -> io::Result<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(OUTSIDE)?;
    Ok(socket.local_addr()?.ip())
}

/// `var` (the override) when set and not empty, else what `detect` finds; `localhost`, with one line to `warn`, when
/// `detect` fails or finds a loopback or unspecified address.
pub fn choose(var: Option<String>, detect: impl FnOnce() -> io::Result<IpAddr>, warn: &mut dyn Write) -> String {
    if let Some(host) = var.filter(|v| !v.is_empty()) {
        return host;
    }
    let reason = match detect() {
        Ok(ip) if !ip.is_loopback() && !ip.is_unspecified() => return ip.to_string(),
        Ok(ip) => format!("it found {ip}"),
        Err(e) => format!("it failed: {e}"),
    };
    // A failing warning cannot make the host wrong.
    let _ = writeln!(warn, "cpd: no primary IPv4 address ({reason}); Plan review links name localhost. Set {URL_HOST} to choose.");
    "localhost".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> io::Result<IpAddr> {
        Ok(text.parse().unwrap())
    }

    fn chosen(var: Option<&str>, detect: io::Result<IpAddr>) -> (String, String) {
        let mut warn = Vec::new();
        let host = choose(var.map(str::to_owned), || detect, &mut warn);
        (host, String::from_utf8(warn).unwrap())
    }

    #[test]
    fn the_environment_override_wins_without_detecting() {
        let mut warn = Vec::new();
        let host = choose(Some("homeserver.ts.net".into()), || panic!("detected"), &mut warn);
        assert_eq!((host.as_str(), warn.len()), ("homeserver.ts.net", 0));
    }

    #[test]
    fn a_detected_address_is_the_host_when_there_is_no_override() {
        assert_eq!(chosen(None, ip("192.168.3.3")), ("192.168.3.3".into(), String::new()));
        assert_eq!(chosen(Some(""), ip("10.0.0.7")), ("10.0.0.7".into(), String::new()), "an empty override is none");
    }

    #[test]
    fn a_failed_or_loopback_detection_falls_back_to_localhost_with_one_line() {
        for detect in [ip("127.0.0.1"), ip("0.0.0.0"), Err(io::Error::other("no route"))] {
            let (host, warn) = chosen(None, detect);
            assert_eq!(host, "localhost");
            assert_eq!(warn.lines().count(), 1, "{warn:?}");
            assert!(warn.starts_with("cpd: no primary IPv4 address ("), "{warn:?}");
        }
    }

    #[test]
    fn detection_reads_the_local_address_of_an_unsent_udp_connect() {
        // Either an address (never a hang, no packet) or an error on a machine without a route.
        if let Ok(ip) = primary_ipv4() {
            assert!(ip.is_ipv4());
        }
    }
}
