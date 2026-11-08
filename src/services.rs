//! Names of well-known ports (#5): a built-in table of the ports an administrator commonly meets
//! on Windows and Linux machines, completed by the system services database
//! (`%SystemRoot%\System32\drivers\etc\services`, `/etc/services`, #34), which [`load_system`]
//! reads once at start-up on its own thread. The built-in names win where both exist (they are
//! tuned for Windows); until the file is read, or without it, only the built-in table is used.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::net::Protocol;

/// The system services database: (port, TCP?) → name.
static SYSTEM: OnceLock<HashMap<(u16, bool), String>> = OnceLock::new();

/// The service name of a port, e.g. 443 → "https": the built-in table, then the system services
/// database. `proto` separates the ports whose name differs between TCP and UDP.
pub fn name(port: u16, proto: Protocol) -> Option<&'static str> {
    builtin(port, proto).or_else(|| {
        SYSTEM
            .get()?
            .get(&(port, proto.is_tcp()))
            .map(String::as_str)
    })
}

/// Reads the system services database on a background thread (once; later calls do nothing).
pub fn load_system() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("services".into())
        .spawn(|| {
            let text = system_file().and_then(|p| std::fs::read_to_string(p).ok());
            let _ = SYSTEM.set(text.as_deref().map(parse).unwrap_or_default());
        });
    if spawned.is_err() {
        let _ = SYSTEM.set(HashMap::new());
    }
}

/// Where the operating system keeps its services database.
fn system_file() -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        Some(
            std::path::PathBuf::from(root)
                .join("System32")
                .join("drivers")
                .join("etc")
                .join("services"),
        )
    } else {
        Some("/etc/services".into())
    }
}

/// Parses a services file: `name  port/protocol  [aliases…]  [# comment]` per line. Only `tcp`
/// and `udp` entries count; the first name of a port and protocol wins.
pub fn parse(text: &str) -> HashMap<(u16, bool), String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut fields = line.split_whitespace();
        let (Some(name), Some(port_proto)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Some((port, proto)) = port_proto.split_once('/') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let tcp = match proto.to_ascii_lowercase().as_str() {
            "tcp" => true,
            "udp" => false,
            _ => continue,
        };
        map.entry((port, tcp)).or_insert_with(|| name.to_owned());
    }
    map
}

/// The built-in names.
fn builtin(port: u16, proto: Protocol) -> Option<&'static str> {
    let tcp = proto.is_tcp();
    Some(match port {
        20 => "ftp-data",
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 => "smtp",
        53 => "dns",
        67 => "dhcp-server",
        68 => "dhcp-client",
        69 => "tftp",
        80 => "http",
        88 => "kerberos",
        110 => "pop3",
        119 => "nntp",
        123 => "ntp",
        135 => "msrpc",
        137 => "netbios-ns",
        138 => "netbios-dgm",
        139 => "netbios-ssn",
        143 => "imap",
        161 => "snmp",
        162 => "snmp-trap",
        389 => "ldap",
        443 if tcp => "https",
        443 => "quic",
        445 => "microsoft-ds",
        464 => "kpasswd",
        465 => "smtps",
        500 => "isakmp",
        514 if tcp => "shell",
        514 => "syslog",
        546 => "dhcpv6-client",
        547 => "dhcpv6-server",
        587 => "submission",
        593 => "http-rpc-epmap",
        636 => "ldaps",
        853 => "dns-over-tls",
        873 => "rsync",
        902 => "vmware-auth",
        993 => "imaps",
        995 => "pop3s",
        1194 => "openvpn",
        1433 => "ms-sql",
        1434 => "ms-sql-monitor",
        1701 => "l2tp",
        1723 => "pptp",
        1812 => "radius",
        1813 => "radius-acct",
        1900 => "ssdp",
        2049 => "nfs",
        2179 => "vmrdp",
        3268 => "ldap-gc",
        3269 => "ldaps-gc",
        3306 => "mysql",
        3389 => "rdp",
        3478 => "stun",
        3702 => "ws-discovery",
        4500 => "ipsec-nat-t",
        5040 => "cdpsvc",
        5060 => "sip",
        5061 => "sips",
        5222 => "xmpp-client",
        5353 => "mdns",
        5355 => "llmnr",
        5357 => "wsd",
        5432 => "postgresql",
        5671 => "amqps",
        5672 => "amqp",
        5900 => "vnc",
        5985 => "winrm-http",
        5986 => "winrm-https",
        6379 => "redis",
        7680 => "delivery-optimization",
        8000 | 8008 | 8080 => "http-alt",
        8443 => "https-alt",
        9090 => "websm",
        9100 => "jetdirect",
        9200 => "elasticsearch",
        11211 => "memcached",
        27017 => "mongodb",
        47001 => "winrm",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_unknown_ports() {
        assert_eq!(name(443, Protocol::Tcp), Some("https"));
        assert_eq!(name(443, Protocol::Udp6), Some("quic"));
        assert_eq!(name(53, Protocol::Udp), Some("dns"));
        assert_eq!(name(3389, Protocol::Tcp6), Some("rdp"));
        assert_eq!(name(49_152, Protocol::Tcp), None);
        assert_eq!(name(0, Protocol::Tcp), None);
        assert_eq!(name(8008, Protocol::Tcp), Some("http-alt"));
        assert_eq!(name(47001, Protocol::Tcp), Some("winrm"));
    }

    #[test]
    fn services_file() {
        let text = "# Copyright (c) Microsoft Corp.\n\
            echo                7/tcp\n\
            echo                7/udp\n\
            http               80/tcp    www www-http      #World Wide Web\n\
            www-alt            80/tcp\n\
            kerberos           88/tcp    krb5 kerberos-sec #Kerberos\n\
            ddp                 2/ddp\n\
            broken            x/tcp\n\
            nfsd-status      1110/TCP\n";
        let map = parse(text);
        assert_eq!(map.get(&(7, true)).map(String::as_str), Some("echo"));
        assert_eq!(map.get(&(7, false)).map(String::as_str), Some("echo"));
        // The first name of a port wins; aliases and comments are ignored.
        assert_eq!(map.get(&(80, true)).map(String::as_str), Some("http"));
        assert_eq!(
            map.get(&(1110, true)).map(String::as_str),
            Some("nfsd-status")
        );
        assert!(!map.contains_key(&(2, true)) && !map.contains_key(&(80, false)));
        assert_eq!(map.len(), 5);
    }
}
