//! Decoding of the tables returned by `GetExtendedTcpTable` / `GetExtendedUdpTable` with the
//! `*_OWNER_MODULE` classes (#14): the endpoint, its PID, its creation time and the opaque owner
//! module information that `GetOwnerModuleFrom*Entry` turns into a module / service name.
//!
//! The functions work on the raw byte buffer the API fills, so they are pure and tested on every
//! platform. The rows contain 8-byte fields, so every table is `dwNumEntries` + 4 bytes padding
//! and the rows start at offset 8. Layouts (offsets in bytes; `DWORD`s little-endian):
//!
//! | row                        | size | fields |
//! |----------------------------|------|--------|
//! | `MIB_TCPROW_OWNER_MODULE`  | 160  | 0 state, 4 local addr, 8 local port, 12 remote addr, 16 remote port, 20 pid, 24 create time, 32 module info `[u64; 16]` |
//! | `MIB_TCP6ROW_OWNER_MODULE` | 192  | 0 local addr `[u8;16]`, 16 local scope, 20 local port, 24 remote addr, 40 remote scope, 44 remote port, 48 state, 52 pid, 56 create time, 64 module info |
//! | `MIB_UDPROW_OWNER_MODULE`  | 160  | 0 local addr, 4 local port, 8 pid, 16 create time, 24 flags, 32 module info |
//! | `MIB_UDP6ROW_OWNER_MODULE` | 176  | 0 local addr `[u8;16]`, 16 local scope, 20 local port, 24 pid, 32 create time, 40 flags, 48 module info |
//!
//! IPv4 addresses are stored in network byte order (the bytes in memory are the address octets),
//! ports are in network byte order in the low 16 bits of their `DWORD`. The create time is a
//! FILETIME (100 ns since 1601), 0 when Windows does not know it.

// Used only by the Windows backend; tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use super::{Connection, Protocol, TcpState};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

pub const TCP4_ROW: usize = 160;
pub const TCP6_ROW: usize = 192;
pub const UDP4_ROW: usize = 160;
pub const UDP6_ROW: usize = 176;
/// Where the rows start (after `dwNumEntries` and the padding to 8 bytes).
pub const ROWS_AT: usize = 8;

fn dword(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn qword(b: &[u8], at: usize) -> u64 {
    let mut q = [0u8; 8];
    q.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(q)
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

/// Creation time in ms since the Unix epoch; `None` for 0 / before 1970.
fn created(b: &[u8], at: usize) -> Option<u64> {
    crate::winproc::filetime_to_unix_ms(qword(b, at)).filter(|&ms| ms > 0)
}

/// The rows of a table: the count from the header, limited to what the buffer really holds.
pub fn rows(buf: &[u8], row_size: usize) -> impl Iterator<Item = &[u8]> {
    let n = if buf.len() >= 4 {
        dword(buf, 0) as usize
    } else {
        0
    };
    let fit = buf.len().saturating_sub(ROWS_AT) / row_size;
    buf.get(ROWS_AT..)
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
                created: created(r, 24),
                module: None,
                stats: None,
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
                created: created(r, 56),
                module: None,
                stats: None,
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
            created: created(r, 16),
            module: None,
            stats: None,
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
            created: created(r, 32),
            module: None,
            stats: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FILETIME of 2023-11-14T22:13:20.123Z.
    const FT: u64 = 116_444_736_000_000_000 + 17_000_000_001_230_000;
    const MS: u64 = 1_700_000_000_123;

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
        v.extend([0u8; 4]); // padding to the 8-byte rows
        for r in rows {
            v.extend_from_slice(r);
        }
        v
    }

    /// Fills a row up to `size` with the create time at `ts` and module info bytes (0xAB).
    fn finish(mut r: Vec<u8>, ts: usize, ft: u64, size: usize) -> Vec<u8> {
        r.resize(ts, 0);
        r.extend(ft.to_le_bytes());
        r.resize(size, 0xAB);
        r
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
        let est = finish(est, 24, FT, TCP4_ROW);
        let mut lis = Vec::new();
        lis.extend(dw(2)); // Listen
        lis.extend([0, 0, 0, 0]);
        lis.extend(pdw(445));
        lis.extend([0, 0, 0, 0]);
        lis.extend(pdw(0));
        lis.extend(dw(4));
        let lis = finish(lis, 24, 0, TCP4_ROW);
        let c = decode_tcp4(&table(2, &[est, lis]));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].local, "192.168.1.10:51234".parse().unwrap());
        assert_eq!(c[0].remote, Some("140.82.112.3:443".parse().unwrap()));
        assert_eq!(c[0].state, Some(TcpState::Established));
        assert_eq!(c[0].pid, 4321);
        assert_eq!(c[0].created, Some(MS));
        assert_eq!(c[1].local, "0.0.0.0:445".parse().unwrap());
        assert_eq!(c[1].remote, None);
        assert_eq!(c[1].state, Some(TcpState::Listen));
        assert_eq!(c[1].created, None);
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
        let r = finish(r, 56, FT, TCP6_ROW);
        assert_eq!(r.len(), TCP6_ROW);
        let c = decode_tcp6(&table(1, &[r]));
        assert_eq!(c[0].proto, Protocol::Tcp6);
        assert_eq!(c[0].local, sock6(local, 50000, 7));
        assert_eq!(c[0].remote.unwrap().port(), 80);
        assert_eq!(c[0].state, Some(TcpState::TimeWait));
        assert_eq!(c[0].pid, 99);
        assert_eq!(c[0].created, Some(MS));
    }

    #[test]
    fn udp_rows() {
        let mut r4 = Vec::new();
        r4.extend([127, 0, 0, 1]);
        r4.extend(pdw(53));
        r4.extend(dw(1000));
        let r4 = finish(r4, 16, FT, UDP4_ROW);
        let c = decode_udp4(&table(1, &[r4]));
        assert_eq!(c[0].local, "127.0.0.1:53".parse().unwrap());
        assert_eq!((c[0].remote, c[0].state, c[0].pid), (None, None, 1000));
        assert_eq!(c[0].created, Some(MS));

        let mut r6 = Vec::new();
        r6.extend(Ipv6Addr::UNSPECIFIED.octets());
        r6.extend(dw(0));
        r6.extend(pdw(5353));
        r6.extend(dw(2000));
        let r6 = finish(r6, 32, FT, UDP6_ROW);
        assert_eq!(r6.len(), UDP6_ROW);
        let c = decode_udp6(&table(1, &[r6]));
        assert_eq!(c[0].local.port(), 5353);
        assert_eq!(c[0].pid, 2000);
        assert_eq!(c[0].created, Some(MS));
    }

    #[test]
    fn rows_start_after_the_padding() {
        let r = finish(vec![1; 24], 24, 0, TCP4_ROW);
        let t = table(1, &[r]);
        let rows: Vec<&[u8]> = rows(&t, TCP4_ROW).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].as_ptr(), t[ROWS_AT..].as_ptr());
    }

    #[test]
    fn short_or_bogus_buffers_do_not_panic() {
        assert!(decode_tcp4(&[]).is_empty());
        assert!(decode_tcp4(&[1, 0]).is_empty());
        // Header claims 100 rows, buffer holds none.
        assert!(decode_udp4(&table(100, &[])).is_empty());
        // Half a row.
        let mut t = table(1, &[]);
        t.extend([0u8; TCP4_ROW / 2]);
        assert!(decode_tcp4(&t).is_empty());
    }
}
