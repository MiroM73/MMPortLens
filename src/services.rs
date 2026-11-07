//! Names of well-known ports (#5): a built-in table, so nothing depends on a `services` file.
//! Only the ports an administrator commonly meets on Windows and Linux machines.

use crate::net::Protocol;

/// The service name of a port, e.g. 443 → "https". `proto` separates the few ports whose name
/// differs between TCP and UDP.
pub fn name(port: u16, proto: Protocol) -> Option<&'static str> {
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
        3702 => "ws-discovery",
        4500 => "ipsec-nat-t",
        5040 => "cdpsvc",
        5060 => "sip",
        5061 => "sips",
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
        8080 => "http-alt",
        8443 => "https-alt",
        9090 => "websm",
        9200 => "elasticsearch",
        11211 => "memcached",
        27017 => "mongodb",
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
    }
}
