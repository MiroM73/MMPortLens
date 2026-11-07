//! Linux backend: `/proc/net/{tcp,tcp6,udp,udp6}` and the owning processes found through the
//! socket inodes in `/proc/<pid>/fd`. The parsing is pure and tested on every platform.
//!
//! A line of `/proc/net/tcp`:
//!
//! ```text
//!   sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
//!    0: 0100007F:0277 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22357 …
//! ```
//!
//! Addresses are hex words in host byte order (IPv6: four 32-bit words), ports are plain hex.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::{Connection, Protocol, TcpState};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// One parsed line: the connection (PID still 0) and the socket inode.
#[derive(Debug, PartialEq, Eq)]
pub struct ProcLine {
    pub conn: Connection,
    pub inode: u64,
}

fn parse_ip(hex: &str) -> Option<IpAddr> {
    match hex.len() {
        8 => {
            let w = u32::from_str_radix(hex, 16).ok()?;
            Some(IpAddr::V4(Ipv4Addr::from(w.to_ne_bytes())))
        }
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                let w = u32::from_str_radix(&hex[i * 8..i * 8 + 8], 16).ok()?;
                b[i * 4..i * 4 + 4].copy_from_slice(&w.to_ne_bytes());
            }
            Some(IpAddr::V6(Ipv6Addr::from(b)))
        }
        _ => None,
    }
}

fn parse_endpoint(s: &str) -> Option<SocketAddr> {
    let (ip, port) = s.split_once(':')?;
    Some(SocketAddr::new(
        parse_ip(ip)?,
        u16::from_str_radix(port, 16).ok()?,
    ))
}

/// Parses the content of one of the `/proc/net` files (the header line is skipped).
pub fn parse_table(text: &str, proto: Protocol) -> Vec<ProcLine> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                return None;
            }
            let local = parse_endpoint(f[1])?;
            let remote = parse_endpoint(f[2])?;
            let st = u32::from_str_radix(f[3], 16).ok()?;
            let inode = f[9].parse().ok()?;
            let (state, remote) = if proto.is_tcp() {
                let state = TcpState::from_linux(st);
                let none =
                    state == TcpState::Listen && remote.port() == 0 && remote.ip().is_unspecified();
                (Some(state), (!none).then_some(remote))
            } else {
                (None, None)
            };
            Some(ProcLine {
                conn: Connection {
                    proto,
                    local,
                    remote,
                    state,
                    pid: 0,
                    created: None,
                    module: None,
                    stats: None,
                },
                inode,
            })
        })
        .collect()
}

/// Inode of a `/proc/<pid>/fd/*` link target `socket:[12345]`.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

/// Socket inode → PID for all processes we may look into.
#[cfg(target_os = "linux")]
fn inode_owners() -> HashMap<u64, u32> {
    let mut map = HashMap::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return map;
    };
    for p in procs.flatten() {
        let Some(pid) = p.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Some(inode) = std::fs::read_link(fd.path())
                .ok()
                .and_then(|l| l.to_str().and_then(socket_inode))
            {
                map.entry(inode).or_insert(pid);
            }
        }
    }
    map
}

/// Fills the PIDs from the inode map (inode 0 = a socket in TIME_WAIT, owned by nobody).
pub fn assign_pids(lines: Vec<ProcLine>, owners: &HashMap<u64, u32>) -> Vec<Connection> {
    lines
        .into_iter()
        .map(|mut l| {
            if l.inode != 0 {
                l.conn.pid = owners.get(&l.inode).copied().unwrap_or(0);
            }
            l.conn
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn snapshot() -> Result<Vec<Connection>, super::NetError> {
    let files = [
        ("/proc/net/tcp", Protocol::Tcp),
        ("/proc/net/tcp6", Protocol::Tcp6),
        ("/proc/net/udp", Protocol::Udp),
        ("/proc/net/udp6", Protocol::Udp6),
    ];
    let mut lines = Vec::new();
    for (path, proto) in files {
        // A kernel without IPv6 has no tcp6 / udp6 file – not an error.
        if let Ok(text) = std::fs::read_to_string(path) {
            lines.extend(parse_table(&text, proto));
        }
    }
    Ok(assign_pids(lines, &inode_owners()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0277 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22357 1 0000000000000000 100 0 0 10 0
   1: 0A01A8C0:C822 0370528C:01BB 01 00000000:00000000 02:000A7D1E 00000000  1000        0 98765 2 0000000000000000 20 4 30 10 -1
";

    #[test]
    fn parse_tcp4() {
        if cfg!(target_endian = "big") {
            return; // the sample is from a little-endian machine
        }
        let l = parse_table(TCP, Protocol::Tcp);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].conn.local, "127.0.0.1:631".parse().unwrap());
        assert_eq!(l[0].conn.state, Some(TcpState::Listen));
        assert_eq!(l[0].conn.remote, None);
        assert_eq!(l[0].inode, 22357);
        assert_eq!(l[1].conn.local, "192.168.1.10:51234".parse().unwrap());
        assert_eq!(l[1].conn.remote, Some("140.82.112.3:443".parse().unwrap()));
        assert_eq!(l[1].conn.state, Some(TcpState::Established));
    }

    #[test]
    fn parse_tcp6_and_udp() {
        if cfg!(target_endian = "big") {
            return;
        }
        let tcp6 = "header\n   0: 00000000000000000000000001000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1111 1\n";
        let l = parse_table(tcp6, Protocol::Tcp6);
        assert_eq!(l[0].conn.local, "[::1]:22".parse().unwrap());
        let udp = "header\n  10: 00000000:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000   100        0 2222 2\n";
        let l = parse_table(udp, Protocol::Udp);
        assert_eq!(l[0].conn.local, "0.0.0.0:5353".parse().unwrap());
        assert_eq!((l[0].conn.remote, l[0].conn.state), (None, None));
    }

    #[test]
    fn inodes_and_pids() {
        assert_eq!(socket_inode("socket:[4242]"), Some(4242));
        assert_eq!(socket_inode("/dev/null"), None);
        let lines = parse_table(TCP, Protocol::Tcp);
        let owners = HashMap::from([(98765u64, 777u32)]);
        let c = assign_pids(lines, &owners);
        assert_eq!((c[0].pid, c[1].pid), (0, 777));
    }

    #[test]
    fn bad_lines_are_skipped() {
        assert!(parse_table("header\ngarbage\n  0: zz:zz\n", Protocol::Tcp).is_empty());
    }
}
