//! Decoding of the tables returned by `GetExtendedTcpTable` / `GetExtendedUdpTable`.
//!
//! The functions work on the raw byte buffer the API fills, so they are pure and tested on
//! every platform. Layouts (all fields 4-byte little-endian `DWORD`s unless noted):
//!
//! | table                          | row                     | row size |
//! |--------------------------------|-------------------------|----------|
//! | `MIB_TCPTABLE_OWNER_PID` (v4)  | state, local addr, local port, remote addr, remote port, pid | 24 |
//! | `MIB_TCP6TABLE_OWNER_PID` (v6) | local addr `[u8;16]`, local scope, local port, remote addr `[u8;16]`, remote scope, remote port, state, pid | 56 |
//! | `MIB_UDPTABLE_OWNER_PID` (v4)  | local addr, local port, pid | 12 |
//! | `MIB_UDP6TABLE_OWNER_PID` (v6) | local addr `[u8;16]`, local scope, local port, pid | 28 |
//!
//! Every table starts with `dwNumEntries` followed by the rows. IPv4 addresses are stored in
//! network byte order (the bytes in memory are the address octets), ports are in network byte
//! order in the low 16 bits of their `DWORD`.

// Used only by the Windows backend; tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use super::{Connection, Protocol, TcpState};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

pub const TCP4_ROW: usize = 24;
pub const TCP6_ROW: usize = 56;
pub const UDP4_ROW: usize = 12;
pub const UDP6_ROW: usize = 28;

fn dword(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Port from its `DWORD`: network byte order in the low 16 bits.
fn port(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

fn ipv4(b: &[u8], at: usize) -> Ipv4Addr {
    Ipv4Addr::new(b[at], b[at + 1], b[at + 2], b[at + 3])
}

fn ipv6(b: &[u8], at: usize) -> Ipv6Addr {
    let mut a = [0u8; 16];
    a.copy_from_slice(&b[at..at + 16]);
    Ipv6Addr::from(a)
}

fn sock6(ip: Ipv6Addr, port: u16, scope: u32) -> SocketAddr {
    SocketAddr::V6(SocketAddrV6::new(ip, port, 0, scope))
}

/// The rows of a table: the count from the header, limited to what the buffer really holds.
fn rows(buf: &[u8], row_size: usize) -> impl Iterator<Item = &[u8]> {
    let n = if buf.len() >= 4 {
        dword(buf, 0) as usize
    } else {
        0
    };
    let fit = buf.len().saturating_sub(4) / row_size;
    buf.get(4..)
        .unwrap_or_default()
        .chunks_exact(row_size)
        .take(n.min(fit))
}

/// Remote side of a TCP row: none for a listener (remote address and port 0).
fn remote_or_none(state: TcpState, addr: SocketAddr) -> Option<SocketAddr> {
    if state == TcpState::Listen && addr.port() == 0 && addr.ip().is_unspecified() {
        None
    } else {
        Some(addr)
    }
}

pub fn decode_tcp4(buf: &[u8]) -> Vec<Connection> {
    rows(buf, TCP4_ROW)
        .map(|r| {
            let state = TcpState::from_mib(dword(r, 0));
            let remote = SocketAddr::new(IpAddr::V4(ipv4(r, 12)), port(r, 16));
            Connection {
                proto: Protocol::Tcp,
                local: SocketAddr::new(IpAddr::V4(ipv4(r, 4)), port(r, 8)),
                remote: remote_or_none(state, remote),
                state: Some(state),
                pid: dword(r, 20),
            }
        })
        .collect()
}

pub fn decode_tcp6(buf: &[u8]) -> Vec<Connection> {
    rows(buf, TCP6_ROW)
        .map(|r| {
            let state = TcpState::from_mib(dword(r, 48));
            let remote = sock6(ipv6(r, 24), port(r, 44), dword(r, 40));
            Connection {
                proto: Protocol::Tcp6,
                local: sock6(ipv6(r, 0), port(r, 20), dword(r, 16)),
                remote: remote_or_none(state, remote),
                state: Some(state),
                pid: dword(r, 52),
            }
        })
        .collect()
}

pub fn decode_udp4(buf: &[u8]) -> Vec<Connection> {
    rows(buf, UDP4_ROW)
        .map(|r| Connection {
            proto: Protocol::Udp,
            local: SocketAddr::new(IpAddr::V4(ipv4(r, 0)), port(r, 4)),
            remote: None,
            state: None,
            pid: dword(r, 8),
        })
        .collect()
}

pub fn decode_udp6(buf: &[u8]) -> Vec<Connection> {
    rows(buf, UDP6_ROW)
        .map(|r| Connection {
            proto: Protocol::Udp6,
            local: sock6(ipv6(r, 0), port(r, 20), dword(r, 16)),
            remote: None,
            state: None,
            pid: dword(r, 24),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dw(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    /// A port `DWORD` as Windows fills it: network order in the low two bytes.
    fn pdw(p: u16) -> [u8; 4] {
        let b = p.to_be_bytes();
        [b[0], b[1], 0, 0]
    }

    fn table(n: u32, rows: &[Vec<u8>]) -> Vec<u8> {
        let mut v = dw(n).to_vec();
        for r in rows {
            v.extend_from_slice(r);
        }
        v
    }

    #[test]
    fn tcp4_rows() {
        let mut est = Vec::new();
        est.extend(dw(5)); // Established
        est.extend([192, 168, 1, 10]);
        est.extend(pdw(51234));
        est.extend([140, 82, 112, 3]);
        est.extend(pdw(443));
        est.extend(dw(4321));
        let mut lis = Vec::new();
        lis.extend(dw(2)); // Listen
        lis.extend([0, 0, 0, 0]);
        lis.extend(pdw(445));
        lis.extend([0, 0, 0, 0]);
        lis.extend(pdw(0));
        lis.extend(dw(4));
        let c = decode_tcp4(&table(2, &[est, lis]));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].local, "192.168.1.10:51234".parse().unwrap());
        assert_eq!(c[0].remote, Some("140.82.112.3:443".parse().unwrap()));
        assert_eq!(c[0].state, Some(TcpState::Established));
        assert_eq!(c[0].pid, 4321);
        assert_eq!(c[1].local, "0.0.0.0:445".parse().unwrap());
        assert_eq!(c[1].remote, None);
        assert_eq!(c[1].state, Some(TcpState::Listen));
    }

    #[test]
    fn tcp6_row() {
        let local: Ipv6Addr = "fe80::1".parse().unwrap();
        let remote: Ipv6Addr = "2001:db8::2".parse().unwrap();
        let mut r = Vec::new();
        r.extend(local.octets());
        r.extend(dw(7)); // scope
        r.extend(pdw(50000));
        r.extend(remote.octets());
        r.extend(dw(0));
        r.extend(pdw(80));
        r.extend(dw(11)); // TimeWait
        r.extend(dw(99));
        assert_eq!(r.len(), TCP6_ROW);
        let c = decode_tcp6(&table(1, &[r]));
        assert_eq!(c[0].proto, Protocol::Tcp6);
        assert_eq!(c[0].local, sock6(local, 50000, 7));
        assert_eq!(c[0].remote.unwrap().port(), 80);
        assert_eq!(c[0].state, Some(TcpState::TimeWait));
        assert_eq!(c[0].pid, 99);
    }

    #[test]
    fn udp_rows() {
        let mut r4 = Vec::new();
        r4.extend([127, 0, 0, 1]);
        r4.extend(pdw(53));
        r4.extend(dw(1000));
        let c = decode_udp4(&table(1, &[r4]));
        assert_eq!(c[0].local, "127.0.0.1:53".parse().unwrap());
        assert_eq!((c[0].remote, c[0].state, c[0].pid), (None, None, 1000));

        let mut r6 = Vec::new();
        r6.extend(Ipv6Addr::UNSPECIFIED.octets());
        r6.extend(dw(0));
        r6.extend(pdw(5353));
        r6.extend(dw(2000));
        assert_eq!(r6.len(), UDP6_ROW);
        let c = decode_udp6(&table(1, &[r6]));
        assert_eq!(c[0].local.port(), 5353);
        assert_eq!(c[0].pid, 2000);
    }

    #[test]
    fn short_or_bogus_buffers_do_not_panic() {
        assert!(decode_tcp4(&[]).is_empty());
        assert!(decode_tcp4(&[1, 0]).is_empty());
        // Header claims 100 rows, buffer holds none.
        assert!(decode_udp4(&table(100, &[])).is_empty());
    }
}
