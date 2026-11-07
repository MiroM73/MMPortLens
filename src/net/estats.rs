//! Bytes and segments sent / received per TCP connection (#8), from the extended TCP statistics
//! of Windows (ESTATS): `SetPerTcp[6]ConnectionEStats` switches the collection of
//! `TcpConnectionEstatsData` on for a connection, `GetPerTcp[6]ConnectionEStats` reads its
//! counters. Windows allows this only to administrators; the counters start when the collection
//! is switched on (not when the connection was created).
//!
//! The row structures passed to the API ([`tcp_row`], [`tcp6_row`]) and the decoding of
//! `TCP_ESTATS_DATA_ROD_v0` ([`decode_data`]) are pure and tested on every platform.

// Used only by the Windows backend; tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use super::{ConnKey, Connection, Protocol, TcpState};
use std::net::{IpAddr, SocketAddr};

/// Counters of one connection since the collection was switched on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TcpStats {
    pub bytes_out: u64,
    pub bytes_in: u64,
    pub segs_out: u64,
    pub segs_in: u64,
}

/// Size of `TCP_ESTATS_DATA_ROD_v0`.
pub const DATA_ROD_SIZE: usize = 96;

fn qword(b: &[u8], at: usize) -> u64 {
    let mut q = [0u8; 8];
    q.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(q)
}

/// `TCP_ESTATS_DATA_ROD_v0`: DataBytesOut (0), DataSegsOut (8), DataBytesIn (16),
/// DataSegsIn (24), … – only the data counters are used.
pub fn decode_data(b: &[u8]) -> Option<TcpStats> {
    (b.len() >= DATA_ROD_SIZE).then(|| TcpStats {
        bytes_out: qword(b, 0),
        segs_out: qword(b, 8),
        bytes_in: qword(b, 16),
        segs_in: qword(b, 24),
    })
}

/// `MIB_TCPROW` (state, local addr, local port, remote addr, remote port; ports in network order
/// in the low 16 bits), or `None` for anything but IPv4.
pub fn tcp_row(state: u32, local: SocketAddr, remote: SocketAddr) -> Option<[u8; 20]> {
    let (IpAddr::V4(l), IpAddr::V4(r)) = (local.ip(), remote.ip()) else {
        return None;
    };
    let mut b = [0u8; 20];
    b[0..4].copy_from_slice(&state.to_le_bytes());
    b[4..8].copy_from_slice(&l.octets());
    b[8..10].copy_from_slice(&local.port().to_be_bytes());
    b[12..16].copy_from_slice(&r.octets());
    b[16..18].copy_from_slice(&remote.port().to_be_bytes());
    Some(b)
}

/// `MIB_TCP6ROW`: state, local addr `[u8;16]`, local scope, local port, remote addr, remote
/// scope, remote port (52 bytes), or `None` for anything but IPv6.
pub fn tcp6_row(state: u32, local: SocketAddr, remote: SocketAddr) -> Option<[u8; 52]> {
    let (SocketAddr::V6(l), SocketAddr::V6(r)) = (local, remote) else {
        return None;
    };
    let mut b = [0u8; 52];
    b[0..4].copy_from_slice(&state.to_le_bytes());
    b[4..20].copy_from_slice(&l.ip().octets());
    b[20..24].copy_from_slice(&l.scope_id().to_le_bytes());
    b[24..26].copy_from_slice(&l.port().to_be_bytes());
    b[28..44].copy_from_slice(&r.ip().octets());
    b[44..48].copy_from_slice(&r.scope_id().to_le_bytes());
    b[48..50].copy_from_slice(&r.port().to_be_bytes());
    Some(b)
}

/// Connections whose statistics can be read: TCP with a remote side, synchronized states.
pub fn has_stats(c: &Connection) -> bool {
    c.proto.is_tcp()
        && c.remote.is_some_and(|r| r.port() != 0)
        && !matches!(
            c.state,
            Some(TcpState::Listen | TcpState::TimeWait | TcpState::Closed | TcpState::DeleteTcb)
        )
}

/// The `MIB_TCP_STATE` value for the row passed to the API (Windows matches the addresses; the
/// state of an established connection is used, the real one may be invalid, #21).
const ESTABLISHED: u32 = 5;

/// Why the statistics cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsError {
    /// Not an administrator.
    AccessDenied,
    /// Not supported on this platform.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

/// Switches the collection on for new connections and reads the counters of all connections
/// with statistics. Remembers which connections are switched on; after an access error it stops
/// trying (the rights of a running program do not change).
#[derive(Default)]
pub struct Collector {
    enabled: std::collections::HashSet<(ConnKey, Option<u64>)>,
    denied: bool,
}

impl Collector {
    /// Fills `stats` of the TCP connections; `Err` when Windows refuses (not elevated).
    pub fn fill(&mut self, conns: &mut [Connection]) -> Result<(), StatsError> {
        if self.denied {
            return Err(StatsError::AccessDenied);
        }
        let mut alive = std::collections::HashSet::new();
        for c in conns.iter_mut().filter(|c| has_stats(c)) {
            let Some(remote) = c.remote else { continue };
            let key = (c.key(), c.created);
            alive.insert(key);
            let row = Row::new(c.proto, c.local, remote);
            let Some(row) = row else { continue };
            if !self.enabled.contains(&key) {
                match platform::enable(&row) {
                    Ok(()) => {
                        self.enabled.insert(key);
                    }
                    Err(StatsError::AccessDenied) => {
                        self.denied = true;
                        return Err(StatsError::AccessDenied);
                    }
                    Err(e) => return Err(e),
                }
            }
            c.stats = platform::read(&row);
        }
        self.enabled.retain(|k| alive.contains(k));
        Ok(())
    }
}

/// The row of a connection for the API.
pub enum Row {
    V4([u8; 20]),
    V6([u8; 52]),
}

impl Row {
    fn new(proto: Protocol, local: SocketAddr, remote: SocketAddr) -> Option<Self> {
        match proto {
            Protocol::Tcp => tcp_row(ESTABLISHED, local, remote).map(Row::V4),
            Protocol::Tcp6 => tcp6_row(ESTABLISHED, local, remote).map(Row::V6),
            _ => None,
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::{DATA_ROD_SIZE, Row, StatsError, TcpStats, decode_data};
    use std::ffi::c_void;

    /// `TcpConnectionEstatsData`.
    const ESTATS_DATA: i32 = 1;
    const ERROR_ACCESS_DENIED: u32 = 5;

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn SetPerTcpConnectionEStats(
            row: *const c_void,
            kind: i32,
            rw: *const u8,
            rw_version: u32,
            rw_size: u32,
            offset: u32,
        ) -> u32;
        fn SetPerTcp6ConnectionEStats(
            row: *const c_void,
            kind: i32,
            rw: *const u8,
            rw_version: u32,
            rw_size: u32,
            offset: u32,
        ) -> u32;
        fn GetPerTcpConnectionEStats(
            row: *const c_void,
            kind: i32,
            rw: *mut u8,
            rw_version: u32,
            rw_size: u32,
            ros: *mut u8,
            ros_version: u32,
            ros_size: u32,
            rod: *mut u8,
            rod_version: u32,
            rod_size: u32,
        ) -> u32;
        fn GetPerTcp6ConnectionEStats(
            row: *const c_void,
            kind: i32,
            rw: *mut u8,
            rw_version: u32,
            rw_size: u32,
            ros: *mut u8,
            ros_version: u32,
            ros_size: u32,
            rod: *mut u8,
            rod_version: u32,
            rod_size: u32,
        ) -> u32;
    }

    /// The row as aligned DWORDs (the API reads `MIB_TCPROW` / `MIB_TCP6ROW`).
    fn aligned(row: &Row) -> (Vec<u32>, bool) {
        let (bytes, v6): (&[u8], bool) = match row {
            Row::V4(b) => (b, false),
            Row::V6(b) => (b, true),
        };
        let words = bytes
            .chunks(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        (words, v6)
    }

    /// Switches the collection of the data counters on (TCP_ESTATS_DATA_RW_v0 = one BOOLEAN).
    pub fn enable(row: &Row) -> Result<(), StatsError> {
        let (r, v6) = aligned(row);
        let rw = [1u8];
        let f = if v6 {
            SetPerTcp6ConnectionEStats
        } else {
            SetPerTcpConnectionEStats
        };
        // SAFETY: `r` is a complete MIB_TCP(6)ROW, `rw` a TCP_ESTATS_DATA_RW_v0 of 1 byte.
        let rc = unsafe { f(r.as_ptr().cast(), ESTATS_DATA, rw.as_ptr(), 0, 1, 0) };
        match rc {
            0 => Ok(()),
            ERROR_ACCESS_DENIED => Err(StatsError::AccessDenied),
            // The connection ended meanwhile, or another error: it simply has no statistics.
            _ => Ok(()),
        }
    }

    pub fn read(row: &Row) -> Option<TcpStats> {
        let (r, v6) = aligned(row);
        let mut rw = [0u8; 1];
        let mut rod = [0u64; DATA_ROD_SIZE / 8];
        let f = if v6 {
            GetPerTcp6ConnectionEStats
        } else {
            GetPerTcpConnectionEStats
        };
        // SAFETY: `r` is a complete row; `rw` and `rod` are valid for the sizes given; no ROS.
        let rc = unsafe {
            f(
                r.as_ptr().cast(),
                ESTATS_DATA,
                rw.as_mut_ptr(),
                0,
                1,
                std::ptr::null_mut(),
                0,
                0,
                rod.as_mut_ptr().cast(),
                0,
                DATA_ROD_SIZE as u32,
            )
        };
        if rc != 0 || rw[0] == 0 {
            return None;
        }
        let bytes: Vec<u8> = rod.iter().flat_map(|q| q.to_le_bytes()).collect();
        decode_data(&bytes)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Row, StatsError, TcpStats};

    pub fn enable(_row: &Row) -> Result<(), StatsError> {
        Err(StatsError::Unsupported)
    }

    pub fn read(_row: &Row) -> Option<TcpStats> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_rod_decoding() {
        let mut b = vec![0u8; DATA_ROD_SIZE];
        b[0..8].copy_from_slice(&1000u64.to_le_bytes());
        b[8..16].copy_from_slice(&10u64.to_le_bytes());
        b[16..24].copy_from_slice(&5_000_000u64.to_le_bytes());
        b[24..32].copy_from_slice(&3500u64.to_le_bytes());
        assert_eq!(
            decode_data(&b),
            Some(TcpStats {
                bytes_out: 1000,
                segs_out: 10,
                bytes_in: 5_000_000,
                segs_in: 3500
            })
        );
        assert_eq!(decode_data(&b[..50]), None);
    }

    #[test]
    fn row_layouts() {
        let r = tcp_row(
            5,
            "10.0.0.1:443".parse().unwrap(),
            "10.0.0.2:51234".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(&r[0..4], &5u32.to_le_bytes());
        assert_eq!(&r[4..12], &[10, 0, 0, 1, 0x01, 0xBB, 0, 0]);
        assert_eq!(&r[12..20], &[10, 0, 0, 2, 0xC8, 0x22, 0, 0]);
        assert!(tcp_row(5, "[::1]:1".parse().unwrap(), "[::1]:2".parse().unwrap()).is_none());

        let r6 = tcp6_row(
            5,
            "[fe80::1%7]:443".parse().unwrap(),
            "[2001:db8::2]:80".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(r6[4], 0xfe);
        assert_eq!(&r6[20..24], &7u32.to_le_bytes());
        assert_eq!(&r6[24..26], &[0x01, 0xBB]);
        assert_eq!(r6[28], 0x20);
        assert_eq!(&r6[48..50], &[0, 80]);
    }

    #[test]
    fn which_connections_have_stats() {
        let mut c = Connection {
            proto: Protocol::Tcp,
            local: "10.0.0.1:5000".parse().unwrap(),
            remote: Some("10.0.0.2:443".parse().unwrap()),
            state: Some(TcpState::Established),
            pid: 1,
            created: None,
            module: None,
            stats: None,
        };
        assert!(has_stats(&c));
        c.state = Some(TcpState::Unknown(0));
        assert!(has_stats(&c));
        c.state = Some(TcpState::TimeWait);
        assert!(!has_stats(&c));
        c.state = Some(TcpState::Established);
        c.remote = None;
        assert!(!has_stats(&c));
        c.proto = Protocol::Udp;
        assert!(!has_stats(&c));
    }

    /// Elevated: 200 KB sent over an own loopback connection are counted. Not elevated: the
    /// collector reports AccessDenied (and nothing else).
    #[cfg(windows)]
    #[test]
    fn counts_own_traffic() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let local = client.local_addr().unwrap();
        let mine = |conns: &mut Vec<Connection>| {
            conns.retain(|c| c.local == local && c.remote == Some(addr));
        };
        let mut col = Collector::default();
        let mut conns = super::super::snapshot().unwrap();
        mine(&mut conns);
        match col.fill(&mut conns) {
            Err(StatsError::AccessDenied) => {
                eprintln!("counts_own_traffic: not elevated, only the access error checked");
                return;
            }
            Err(e) => panic!("{e:?}"),
            Ok(()) => {}
        }
        let data = vec![7u8; 200_000];
        client.write_all(&data).unwrap();
        let mut got = vec![0u8; data.len()];
        server.read_exact(&mut got).unwrap();
        let mut conns = super::super::snapshot().unwrap();
        mine(&mut conns);
        col.fill(&mut conns).unwrap();
        let s = conns[0].stats.expect("statistics of the own connection");
        assert!(s.bytes_out >= 200_000, "{s:?}");
    }
}
